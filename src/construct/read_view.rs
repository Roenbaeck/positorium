use super::{Appearance, AppearanceSet, Database, OtherHasher, Posit, Role, Thing};
use crate::error::DatabaseError;
use crate::literal::LiteralValue;
use roaring::RoaringTreemap;
use rpds::{HashTrieMapSync, HashTrieSetSync};
use std::hash::Hash;
use std::sync::{Arc, MutexGuard};
use unicode_normalization::UnicodeNormalization;

type Index<K, V> = HashTrieMapSync<K, V, OtherHasher>;

// Keep a single entry inline. Larger adjacency sets share their trie paths.
#[derive(Clone)]
pub(crate) enum Members<T: Eq + Hash> {
    One(T),
    Many(HashTrieSetSync<T, OtherHasher>),
}

impl<T: Clone + Eq + Hash> Members<T> {
    fn insert(&mut self, value: T) {
        match self {
            Self::One(first) if *first != value => {
                let mut members = HashTrieSetSync::default();
                members.insert_mut(first.clone());
                members.insert_mut(value);
                *self = Self::Many(members);
            }
            Self::One(_) => {}
            Self::Many(members) => members.insert_mut(value),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        let (one, many) = match self {
            Self::One(value) => (Some(value), None),
            Self::Many(members) => (None, Some(members)),
        };
        one.into_iter().chain(many.into_iter().flatten())
    }
}

// Most appearance sets can store their first posit without allocating a bitmap.
#[derive(Clone)]
pub(crate) enum SetPosting {
    One(Thing),
    Many(RoaringTreemap),
}

impl SetPosting {
    pub(crate) fn iter(&self) -> impl Iterator<Item = Thing> + '_ {
        let (one, many) = match self {
            Self::One(id) => (Some(*id), None),
            Self::Many(ids) => (None, Some(ids)),
        };
        one.into_iter().chain(many.into_iter().flatten())
    }

    fn insert(&mut self, identity: Thing) {
        match self {
            Self::One(first) => *self = Self::Many([*first, identity].into_iter().collect()),
            Self::Many(posting) => {
                posting.insert(identity);
            }
        }
    }

    pub(crate) fn len(&self) -> u64 {
        match self {
            Self::One(_) => 1,
            Self::Many(posting) => posting.len(),
        }
    }

    pub(crate) fn union_into(&self, bitmap: &mut RoaringTreemap) {
        match self {
            Self::One(identity) => {
                bitmap.insert(*identity);
            }
            Self::Many(posting) => *bitmap |= posting,
        }
    }
}

/// The catalog and indexes for one Traqula script. Edits to a writer's copy share
/// unchanged index paths and stored values with any readers of older views.
#[derive(Clone, Default)]
pub(crate) struct ReadView {
    roles: Index<String, Arc<Role>>,
    posits: Index<Thing, Arc<Posit<LiteralValue>>>,
    thing_appearances: Index<Thing, Members<Arc<Appearance>>>,
    appearance_sets: Index<Arc<Appearance>, Members<Arc<AppearanceSet>>>,
    set_posits: Index<Arc<AppearanceSet>, SetPosting>,
    role_posits: Index<Thing, RoaringTreemap>,
}

impl ReadView {
    pub(super) fn insert_role(&mut self, role: Arc<Role>) {
        self.roles.insert_mut(role.name().to_owned(), role);
    }

    pub(super) fn insert_posit(&mut self, posit: Arc<Posit<LiteralValue>>) {
        let identity = posit.posit();
        let set = posit.appearance_set();
        if let Some(posting) = self.set_posits.get_mut(&set) {
            posting.insert(identity);
        } else {
            self.set_posits
                .insert_mut(Arc::clone(&set), SetPosting::One(identity));
            for appearance in set.appearances() {
                insert_member(
                    &mut self.thing_appearances,
                    appearance.thing(),
                    Arc::clone(appearance),
                );
                insert_member(
                    &mut self.appearance_sets,
                    Arc::clone(appearance),
                    Arc::clone(&set),
                );
            }
        }
        for appearance in set.appearances() {
            let role = appearance.role().role();
            if let Some(posting) = self.role_posits.get_mut(&role) {
                posting.insert(identity);
            } else {
                self.role_posits
                    .insert_mut(role, std::iter::once(identity).collect());
            }
        }
        self.posits.insert_mut(identity, posit);
    }

    pub(crate) fn role(&self, name: &str) -> Option<Arc<Role>> {
        let canonical_name: String = name.nfc().collect();
        self.normalized_role(&canonical_name)
    }

    pub(crate) fn normalized_role(&self, name: &str) -> Option<Arc<Role>> {
        self.roles.get(name).cloned()
    }

    pub(super) fn role_count(&self) -> usize {
        self.roles.size()
    }

    pub(crate) fn posit(&self, identity: Thing) -> Option<Arc<Posit<LiteralValue>>> {
        self.posits.get(&identity).cloned()
    }

    pub(crate) fn posit_count(&self) -> usize {
        self.posits.size()
    }

    pub(crate) fn posit_ids(&self) -> impl Iterator<Item = Thing> + '_ {
        self.posits.keys().copied()
    }

    pub(crate) fn role_posting(&self, role: Thing) -> Option<&RoaringTreemap> {
        self.role_posits.get(&role)
    }

    pub(crate) fn set_posting(&self, set: &Arc<AppearanceSet>) -> Option<&SetPosting> {
        self.set_posits.get(set)
    }

    pub(crate) fn appearances(&self, thing: Thing) -> Option<&Members<Arc<Appearance>>> {
        self.thing_appearances.get(&thing)
    }

    pub(crate) fn appearance_sets(
        &self,
        appearance: &Arc<Appearance>,
    ) -> Option<&Members<Arc<AppearanceSet>>> {
        self.appearance_sets.get(appearance)
    }
}

fn insert_member<K: Clone + Eq + Hash, T: Clone + Eq + Hash>(
    index: &mut Index<K, Members<T>>,
    key: K,
    value: T,
) {
    if let Some(members) = index.get_mut(&key) {
        members.insert(value);
    } else {
        index.insert_mut(key, Members::One(value));
    }
}

/// A reader pins the published view. A writer uses a private copy and publishes
/// its successfully committed commands before releasing the writer lock, even
/// when a later command returns an error.
pub(crate) struct ScriptView<'a> {
    view: Arc<ReadView>,
    writer: Option<(&'a Database, MutexGuard<'a, ()>)>,
}

impl<'a> ScriptView<'a> {
    pub(crate) fn new(database: &'a Database, owner: Option<MutexGuard<'a, ()>>) -> Self {
        Self {
            view: database.read_view(),
            writer: owner.map(|guard| (database, guard)),
        }
    }

    pub(crate) fn read(&self) -> &ReadView {
        &self.view
    }

    pub(crate) fn is_writer(&self) -> bool {
        self.writer.is_some()
    }

    /// Recheck an optimistic miss only when a different view was published.
    pub(crate) fn acquire_writer(
        &mut self,
        database: &'a Database,
        owner: MutexGuard<'a, ()>,
    ) -> bool {
        let current = database.read_view();
        let changed = !Arc::ptr_eq(&self.view, &current);
        self.view = current;
        self.writer = Some((database, owner));
        changed
    }

    pub(crate) fn write(&mut self) -> Result<&mut ReadView, DatabaseError> {
        if self.writer.is_none() {
            return Err(DatabaseError::Invariant(
                "mutation attempted in a read-only script".into(),
            ));
        }
        Ok(Arc::make_mut(&mut self.view))
    }
}

impl Drop for ScriptView<'_> {
    fn drop(&mut self) {
        if let Some((database, _)) = &self.writer {
            database.publish_read_view(Arc::clone(&self.view));
        }
    }
}
