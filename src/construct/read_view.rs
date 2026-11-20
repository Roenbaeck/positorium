use super::{Appearance, AppearanceSet, Database, OtherHasher, Posit, Role, Thing};
use crate::error::DatabaseError;
use crate::literal::LiteralValue;
use roaring::RoaringTreemap;
use rpds::{HashTrieMapSync, HashTrieSetSync};
use seahash::SeaHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, MutexGuard, OnceLock};
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

    pub(crate) fn contains(&self, id: Thing) -> bool {
        match self {
            Self::One(first) => *first == id,
            Self::Many(ids) => ids.contains(id),
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

// All sets in a group have the same roles. One representative also serves as
// the single inline set for appearances that occur in just one proposition.
#[derive(Clone)]
struct RoleGroup {
    first: Arc<AppearanceSet>,
    others: Option<Arc<HashTrieSetSync<Arc<AppearanceSet>, OtherHasher>>>,
    posit_count: u64,
}

impl RoleGroup {
    fn new(set: &Arc<AppearanceSet>) -> Self {
        Self {
            first: Arc::clone(set),
            others: None,
            posit_count: 1,
        }
    }

    fn insert(&mut self, set: &Arc<AppearanceSet>, new_set: bool) {
        self.posit_count += 1;
        if new_set {
            Arc::make_mut(self.others.get_or_insert_with(Default::default))
                .insert_mut(Arc::clone(set));
        }
    }

    fn sets(&self) -> impl Iterator<Item = &Arc<AppearanceSet>> {
        std::iter::once(&self.first).chain(self.others.iter().flat_map(|sets| sets.iter()))
    }
}

fn role_ids(set: &AppearanceSet) -> impl Iterator<Item = Thing> + '_ {
    set.appearances().iter().map(|a| a.role.role())
}

#[derive(Clone)]
struct AppearanceSets {
    primary: RoleGroup,
    // Pay for a shape index only when this (Thing, Role) appears in multiple
    // role combinations, e.g. both {session, epoch} and {session, item}.
    others: Option<Arc<Index<Vec<Thing>, RoleGroup>>>,
}

impl AppearanceSets {
    fn insert(&mut self, set: &Arc<AppearanceSet>, new_set: bool) {
        if (!new_set && self.others.is_none()) || role_ids(&self.primary.first).eq(role_ids(set)) {
            self.primary.insert(set, new_set);
            return;
        }
        let roles: Vec<_> = role_ids(set).collect();
        let groups = Arc::make_mut(self.others.get_or_insert_with(Default::default));
        if let Some(group) = groups.get_mut(&roles) {
            group.insert(set, new_set);
        } else {
            groups.insert_mut(roles, RoleGroup::new(set));
        }
    }

    fn groups(&self) -> impl Iterator<Item = &RoleGroup> {
        std::iter::once(&self.primary).chain(self.others.iter().flat_map(|groups| groups.values()))
    }
}

/// The catalog and indexes for one Traqula script. Edits to a writer's copy share
/// unchanged index paths and stored values with any readers of older views.
#[derive(Clone, Default)]
pub(crate) struct ReadView {
    roles: Index<String, Arc<Role>>,
    posits: Index<Thing, Arc<Posit<LiteralValue>>>,
    thing_appearances: Index<Thing, Members<Arc<Appearance>>>,
    appearance_sets: Index<Arc<Appearance>, AppearanceSets>,
    set_posits: Index<Arc<AppearanceSet>, SetPosting>,
    role_posits: Index<Thing, RoaringTreemap>,
    value_posits: Index<u64, SetPosting>,
    role_families: Index<Thing, u8>,
    value_families: u8,
    highest_posit: Option<Thing>,
    manifest_hash: OnceLock<SeaHasher>,
}

impl ReadView {
    pub(super) fn insert_role(&mut self, role: Arc<Role>) {
        self.roles.insert_mut(role.name().to_owned(), role);
    }

    pub(super) fn insert_posit(&mut self, posit: Arc<Posit<LiteralValue>>) {
        let identity = posit.posit();
        // Normal commits extend the sorted manifest. Replay can supply older
        // identities out of order, in which case the next audit rebuilds it.
        if self.highest_posit.is_none() {
            self.manifest_hash = OnceLock::from(SeaHasher::new());
        }
        if self.highest_posit.is_some_and(|last| identity <= last) {
            self.manifest_hash.take();
        } else {
            self.highest_posit = Some(identity);
            if let Some(hash) = self.manifest_hash.get_mut() {
                hash.write(&identity.to_le_bytes());
            }
        }
        let fingerprint = posit.value().nominal_fingerprint();
        if let Some(posting) = self.value_posits.get_mut(&fingerprint) {
            posting.insert(identity);
        } else {
            self.value_posits
                .insert_mut(fingerprint, SetPosting::One(identity));
        }
        let family = posit.value().family().nominal_mask();
        self.value_families |= family;
        let set = posit.appearance_set();
        let new_set = self.set_posits.get(&set).is_none();
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
            }
        }
        for appearance in set.appearances() {
            if let Some(entry) = self.appearance_sets.get_mut(appearance) {
                entry.insert(&set, new_set);
            } else {
                self.appearance_sets.insert_mut(
                    Arc::clone(appearance),
                    AppearanceSets {
                        primary: RoleGroup::new(&set),
                        others: None,
                    },
                );
            }
            let role = appearance.role().role();
            let families = self.role_families.get(&role).copied().unwrap_or(0);
            if families & family == 0 {
                self.role_families.insert_mut(role, families | family);
            }
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

    pub(crate) fn manifest_digest(&self) -> Option<u64> {
        self.manifest_hash.get().map(Hasher::finish)
    }

    pub(crate) fn cache_manifest_digest(&self, hash: SeaHasher) -> u64 {
        self.manifest_hash.get_or_init(|| hash).finish()
    }

    pub(crate) fn value_families(&self, role: Option<&str>) -> u8 {
        match role {
            None => self.value_families,
            Some(name) => self
                .normalized_role(name)
                .and_then(|r| self.role_families.get(&r.role()).copied())
                .unwrap_or(0),
        }
    }

    pub(crate) fn value_posting(&self, fingerprint: u64) -> Option<&SetPosting> {
        self.value_posits.get(&fingerprint)
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

    fn matching_groups(
        &self,
        appearance: &Appearance,
        accepts_roles: impl Fn(&AppearanceSet) -> bool,
    ) -> impl Iterator<Item = &RoleGroup> {
        self.appearance_sets
            .get(appearance)
            .into_iter()
            .flat_map(AppearanceSets::groups)
            .filter(move |group| accepts_roles(&group.first))
    }

    pub(crate) fn thing_candidate_count(
        &self,
        thing: Thing,
        role: Option<&Arc<Role>>,
        accepts_roles: impl Fn(&AppearanceSet) -> bool,
    ) -> u64 {
        let count = |appearance: &Appearance| {
            self.matching_groups(appearance, &accepts_roles)
                .map(|group| group.posit_count)
                .sum::<u64>()
        };
        if let Some(role) = role {
            return count(&Appearance::new(thing, Arc::clone(role)));
        }
        self.appearances(thing)
            .into_iter()
            .flat_map(Members::iter)
            .map(|a| count(a))
            .sum()
    }

    pub(crate) fn matching_appearance_sets(
        &self,
        appearance: &Appearance,
        accepts_roles: impl Fn(&AppearanceSet) -> bool,
    ) -> impl Iterator<Item = &Arc<AppearanceSet>> {
        self.matching_groups(appearance, accepts_roles)
            .flat_map(RoleGroup::sets)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_order_replay_invalidates_only_the_new_views_manifest() {
        let db = Database::memory().unwrap();
        db.execute("add role entry; add posit [{(+a,entry)},1,@NOW], [{(a,entry)},2,@NOW], [{(a,entry)},3,@NOW];").unwrap();
        let original = db.read_view();
        let mut ids: Vec<_> = original.posit_ids().collect();
        ids.sort_unstable();
        let hash = |ids: &[Thing]| {
            let mut hash = SeaHasher::new();
            for id in ids {
                hash.write(&id.to_le_bytes());
            }
            hash
        };
        let mut replay = ReadView::default();
        replay.insert_posit(original.posit(ids[1]).unwrap());
        let pinned = replay.clone();
        replay.insert_posit(original.posit(ids[0]).unwrap());
        assert!(replay.manifest_digest().is_none());
        replay.cache_manifest_digest(hash(&ids[..2]));
        replay.insert_posit(original.posit(ids[2]).unwrap());
        assert_eq!(pinned.manifest_digest(), Some(hash(&ids[1..2]).finish()));
        assert_eq!(replay.manifest_digest(), Some(hash(&ids).finish()));
        assert_eq!(replay.manifest_digest(), original.manifest_digest());
    }
}
