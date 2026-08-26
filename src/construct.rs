//! Core construct definitions for the Positorium database engine.
//!
//! This module defines the fundamental building blocks used throughout the
//! engine. Most types follow a "keeper" pattern: keepers own canonical
//! instances (wrapped in `Arc`) and guarantee uniqueness by logical identity
//! (role name, appearance set contents, posit components, etc.).
//!
//! # Main Concepts
//! * [`Thing`]: Opaque identity (currently a `u64`).
//! * [`Role`]: A named semantic placeholder a thing can appear in.
//! * [`Appearance`]: A (Thing, Role) pairing.
//! * [`AppearanceSet`]: A sorted, duplicate-free collection of appearances, at
//!   most one per role.
//! * [`Posit`]: A proposition: (AppearanceSet, Value, Time) with its own
//!   identity (also a thing).
//!
//! # Example
//! Construct a database and mutate it through the high-level execution API.
//! ```
//! use positorium::{Database, Engine, PersistenceMode};
//! let db = Database::new(PersistenceMode::InMemory).unwrap();
//! Engine::new(&db)
//!     .execute("add role person; add posit [{(+person, person)}, \"Alice\", @NOW];")
//!     .unwrap();
//! ```
//!
//! # Unstable internals
//!
//! This hidden module carries no beta compatibility promise. Database mutation
//! is crate-private; beta callers use database construction and high-level
//! execution APIs.
use crate::datatype::{DataType, Time};
use crate::error::DatabaseError;
use crate::literal::LiteralValue;
#[cfg(feature = "persistence")]
use crate::storage::{
    LogStore, ReplayedStore, StoreBackend, posit_record_parts, raw_codec_record, role_record,
};
use bimap::BiMap;
use core::hash::{BuildHasher, BuildHasherDefault, Hasher};
use roaring::RoaringTreemap;
use seahash::SeaHasher;
use std::any::{Any, TypeId};
use std::cmp::Ordering;
use std::collections::hash_map::{Entry, RandomState};
use std::collections::hash_set::Iter;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use unicode_normalization::UnicodeNormalization;

/// Internal heterogeneous map keyed by `TypeId` used for storing per-value
/// type bimap indices for posits.
struct TypeMap {
    // Stored values must be Send + Sync so keepers using TypeMap can be shared across threads safely.
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn get_mut<T: 'static + Send + Sync>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut::<T>())
    }

    pub fn insert<T: 'static + Send + Sync>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }
}

// ------------- Thing -------------
// Mem check example:
// https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=5b196af2532ce0d3413e4523b17980a5
/// Opaque identity type used for every persisted construct.
pub type Thing = u64;
/// Reserved identity constant representing the initial lower bound for the generator.
pub const GENESIS: Thing = 0;
/// Fixed store-local identity of the built-in `posit` role.
pub const POSIT_ROLE_ID: Thing = 1;
/// Fixed store-local identity of the built-in `ascertains` role.
pub const ASCERTAINS_ROLE_ID: Thing = 2;

pub type ThingHasher = BuildHasherDefault<SeaHasher>;
pub type OtherHasher = BuildHasherDefault<SeaHasher>;

#[derive(Debug)]
pub struct ThingGenerator {
    lower_bound: Thing,
    retained: HashSet<Thing, ThingHasher>,
}

impl Default for ThingGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ThingGenerator {
    /// Creates a fresh generator with the lower bound set to [`GENESIS`].
    pub fn new() -> Self {
        Self {
            lower_bound: GENESIS,
            retained: HashSet::<Thing, ThingHasher>::default(),
        }
    }
    /// Things may be explicitly referenced (e.g. restored) but only implicitly
    /// created through the generator. Retaining teaches the generator about an
    /// externally observed identity (e.g. during persistence restore).
    pub fn retain(&mut self, t: Thing) {
        self.retained.insert(t);
        if t > self.lower_bound {
            self.lower_bound = t;
        }
    }
    /// Returns the supplied identity if it is currently retained.
    pub fn check(&self, t: Thing) -> Option<Thing> {
        self.retained.get(&t).cloned()
    }
    /// Releases an abandoned identity without making it available for reuse.
    ///
    /// Thing identities are monotonic and never recycled. Failed or duplicate
    /// commands may therefore leave gaps, as required by D020.
    pub fn release(&mut self, t: Thing) {
        self.retained.remove(&t);
    }
    /// Generates a new identity.
    pub fn generate(&mut self) -> Thing {
        self.lower_bound += 1;
        self.retained.insert(self.lower_bound);
        self.lower_bound
    }
    /// Iterates over currently retained identities (unordered).
    pub fn iter(&self) -> Iter<'_, Thing> {
        self.retained.iter()
    }
}

// ------------- Role -------------
#[derive(Eq, Debug)]
pub struct Role {
    role: Thing, // let it be a thing so we can "talk" about roles using posits
    name: String,
    reserved: bool,
}

impl Role {
    pub fn new(role: Thing, name: String, reserved: bool) -> Self {
        Self {
            role,
            name: name.nfc().collect(),
            reserved,
        }
    }
    // It's intentional to encapsulate the name in the struct
    // and only expose it using a "getter", because this yields
    // true immutability for objects after creation.
    pub fn role(&self) -> Thing {
        self.role
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn reserved(&self) -> bool {
        self.reserved
    }
}
impl Ord for Role {
    fn cmp(&self, other: &Self) -> Ordering {
        self.role.cmp(&other.role)
    }
}
impl PartialOrd for Role {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Role {
    fn eq(&self, other: &Self) -> bool {
        self.role == other.role
    }
}
impl Hash for Role {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.role.hash(state);
    }
}

#[derive(Debug)]
pub struct RoleKeeper {
    kept: HashMap<String, Arc<Role>, OtherHasher>,
    lookup: HashMap<Thing, Arc<Role>, ThingHasher>, // double indexing, but roles should be few so it's not a big deal
}
impl Default for RoleKeeper {
    fn default() -> Self {
        Self::new()
    }
}

impl RoleKeeper {
    pub fn new() -> Self {
        Self {
            kept: HashMap::default(),
            lookup: HashMap::default(),
        }
    }
    pub fn keep(&mut self, role: Role) -> (Arc<Role>, bool) {
        let thing = role.role();
        let keepsake = role.name().to_owned();
        let mut previously_kept = true;
        match self.kept.entry(keepsake.clone()) {
            Entry::Vacant(e) => {
                e.insert(Arc::new(role));
                previously_kept = false;
            }
            Entry::Occupied(_e) => (),
        };
        let kept_role = self.kept.get(&keepsake).unwrap();
        if !previously_kept {
            self.lookup.insert(thing, Arc::clone(kept_role));
        }
        (Arc::clone(kept_role), previously_kept)
    }
    pub fn get(&self, name: &str) -> Option<Arc<Role>> {
        let canonical_name: String = name.nfc().collect();
        self.kept.get(&canonical_name).map(Arc::clone)
    }
    pub fn lookup(&self, role: &Thing) -> Option<Arc<Role>> {
        self.lookup.get(role).map(Arc::clone)
    }
    pub fn len(&self) -> usize {
        self.kept.len()
    }
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

// ------------- Appearance -------------
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct Appearance {
    thing: Thing,
    role: Arc<Role>,
}
impl Appearance {
    pub fn new(thing: Thing, role: Arc<Role>) -> Self {
        Self { thing, role }
    }
    pub fn thing(&self) -> Thing {
        self.thing
    }
    pub fn role(&self) -> Arc<Role> {
        Arc::clone(&self.role)
    }
}
impl Ord for Appearance {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.role, &self.thing).cmp(&(&other.role, &other.thing))
    }
}
impl PartialOrd for Appearance {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Display for Appearance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.thing, self.role.name())
    }
}

#[derive(Debug)]
pub struct AppearanceKeeper {
    kept: HashSet<Arc<Appearance>, OtherHasher>,
}
impl Default for AppearanceKeeper {
    fn default() -> Self {
        Self::new()
    }
}

impl AppearanceKeeper {
    pub fn new() -> Self {
        Self {
            kept: HashSet::default(),
        }
    }
    pub fn keep(&mut self, appearance: Appearance) -> (Arc<Appearance>, bool) {
        let keepsake = Arc::new(appearance);
        let previously_kept = !self.kept.insert(Arc::clone(&keepsake));
        (
            Arc::clone(self.kept.get(&keepsake).unwrap()),
            previously_kept,
        )
    }
    pub fn len(&self) -> usize {
        self.kept.len()
    }
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

// ------------- AppearanceSet -------------
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AppearanceSet {
    appearances: Arc<Vec<Arc<Appearance>>>,
}
impl AppearanceSet {
    pub fn new(mut set: Vec<Arc<Appearance>>) -> Option<Self> {
        set.sort_unstable();
        if set.windows(2).any(|x| x[0].role == x[1].role) {
            return None;
        }
        Some(Self {
            appearances: Arc::new(set),
        })
    }
    pub fn appearances(&self) -> &Vec<Arc<Appearance>> {
        &self.appearances
    }
    pub fn roles(&self) -> Vec<String> {
        let mut roles = Vec::new();
        for appearance in self.appearances.iter() {
            roles.push(String::from(appearance.role.name()));
        }
        roles.sort();
        roles
    }
}
impl fmt::Display for AppearanceSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = String::new();
        for a in self.appearances() {
            s += &(a.to_string() + ",");
        }
        s.pop();
        write!(f, "{{{}}}", s)
    }
}

#[derive(Debug)]
pub struct AppearanceSetKeeper {
    kept: HashSet<Arc<AppearanceSet>, OtherHasher>,
}
impl Default for AppearanceSetKeeper {
    fn default() -> Self {
        Self::new()
    }
}

impl AppearanceSetKeeper {
    pub fn new() -> Self {
        Self {
            kept: HashSet::default(),
        }
    }
    pub fn keep(&mut self, appearance_set: AppearanceSet) -> (Arc<AppearanceSet>, bool) {
        let keepsake = Arc::new(appearance_set);
        let previously_kept = !self.kept.insert(Arc::clone(&keepsake));
        (
            Arc::clone(self.kept.get(&keepsake).unwrap()),
            previously_kept,
        )
    }
    pub fn len(&self) -> usize {
        self.kept.len()
    }
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

// --------------- Posit ----------------
#[derive(Eq, Debug)]
pub struct Posit<V: DataType> {
    posit: Thing, // a posit is also a thing we can "talk" about
    appearance_set: Arc<AppearanceSet>,
    value: V,   // in theory any imprecise value
    time: Time, // in thoery any imprecise time (note that this must be a data type with a natural ordering)
}
impl<V: DataType> Posit<V> {
    pub fn new(posit: Thing, appearance_set: Arc<AppearanceSet>, value: V, time: Time) -> Posit<V> {
        Self {
            posit,
            appearance_set,
            value,
            time,
        }
    }
    pub fn posit(&self) -> Thing {
        self.posit
    }
    pub fn appearance_set(&self) -> Arc<AppearanceSet> {
        Arc::clone(&self.appearance_set)
    }
    pub fn value(&self) -> &V {
        &self.value
    }
    pub fn time(&self) -> &Time {
        &self.time
    }
}
impl<V: DataType> PartialEq for Posit<V> {
    fn eq(&self, other: &Self) -> bool {
        self.appearance_set == other.appearance_set
            && self.value == other.value
            && self.time == other.time
    }
}
impl<V: DataType> Ord for Posit<V> {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.appearance_set, &self.value, &self.time).cmp(&(
            &other.appearance_set,
            &other.value,
            &other.time,
        ))
    }
}
impl<V: DataType> PartialOrd for Posit<V> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<V: DataType> Hash for Posit<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.appearance_set.hash(state);
        self.value.hash(state);
        self.time.hash(state);
    }
}
impl<V: DataType> fmt::Display for Posit<V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} [{}, {}, {}]",
            self.posit, self.appearance_set, self.value, self.time
        )
    }
}

// This key needs to be defined in order to store posits in a TypeMap.

pub struct PositKeeper {
    kept: TypeMap,
    length: usize,
}
impl Default for PositKeeper {
    fn default() -> Self {
        Self::new()
    }
}

impl PositKeeper {
    pub fn new() -> Self {
        Self {
            kept: TypeMap::new(),
            length: 0,
        }
    }
    pub fn keep<V: 'static + DataType>(&mut self, posit: Posit<V>) -> (Arc<Posit<V>>, bool) {
        self.keep_arc(Arc::new(posit))
    }

    fn keep_arc<V: 'static + DataType>(
        &mut self,
        keepsake: Arc<Posit<V>>,
    ) -> (Arc<Posit<V>>, bool) {
        // ensure the map can work with this particular type combo
        let map = if let Some(m) = self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>() {
            m
        } else {
            self.kept
                .insert::<BiMap<Arc<Posit<V>>, Thing>>(BiMap::<Arc<Posit<V>>, Thing>::new());
            self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>().unwrap()
        };
        let keepsake_thing = keepsake.posit();
        let mut previously_kept = false;
        let thing = match map.get_by_left(&keepsake) {
            Some(kept_thing) => {
                previously_kept = true;
                kept_thing
            }
            None => {
                map.insert(Arc::clone(&keepsake), keepsake.posit());
                self.length += 1;
                &keepsake_thing
            }
        };
        (
            Arc::clone(map.get_by_right(thing).unwrap()),
            previously_kept,
        )
    }

    fn existing<V: 'static + DataType>(
        &mut self,
        candidate: &Arc<Posit<V>>,
    ) -> Option<Arc<Posit<V>>> {
        let identity = self
            .kept
            .get_mut::<BiMap<Arc<Posit<V>>, Thing>>()
            .and_then(|map| map.get_by_left(candidate))
            .copied()?;
        self.kept
            .get_mut::<BiMap<Arc<Posit<V>>, Thing>>()?
            .get_by_right(&identity)
            .map(Arc::clone)
    }
    /// Returns the Thing id for a kept posit, or None if not found in the keeper.
    pub fn thing<V: 'static + DataType>(&mut self, posit: Arc<Posit<V>>) -> Option<Thing> {
        let map = if let Some(m) = self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>() {
            m
        } else {
            self.kept
                .insert::<BiMap<Arc<Posit<V>>, Thing>>(BiMap::<Arc<Posit<V>>, Thing>::new());
            self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>().unwrap()
        };
        map.get_by_left(&posit).copied()
    }
    /// Returns the typed posit for a Thing id, or None if not a posit of type V.
    pub fn posit<V: 'static + DataType>(&mut self, thing: Thing) -> Option<Arc<Posit<V>>> {
        let map = if let Some(m) = self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>() {
            m
        } else {
            self.kept
                .insert::<BiMap<Arc<Posit<V>>, Thing>>(BiMap::<Arc<Posit<V>>, Thing>::new());
            self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>().unwrap()
        };
        map.get_by_right(&thing).map(Arc::clone)
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

// ------------- Lookups -------------
#[derive(Debug)]
pub struct Lookup<K, V, H = RandomState> {
    index: HashMap<K, HashSet<V>, H>,
}
impl<K: Eq + Hash, V: Eq + Hash, H: BuildHasher + Default> Default for Lookup<K, V, H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash, V: Eq + Hash, H: BuildHasher + Default> Lookup<K, V, H> {
    pub fn new() -> Self {
        Self {
            index: HashMap::<K, HashSet<V>, H>::default(),
        }
    }
    pub fn insert(&mut self, key: K, value: V) {
        let map = self.index.entry(key).or_default();
        map.insert(value);
    }
    pub fn lookup(&self, key: &K) -> Option<&HashSet<V>> {
        self.index.get(key)
    }
}

/// Lookup mapping a key to a set of Thing IDs, backed by a RoaringTreemap.
#[derive(Debug)]
pub struct ThingLookup<K, H = RandomState> {
    index: HashMap<K, RoaringTreemap, H>,
}
impl<K: Eq + Hash, H: BuildHasher + Default> Default for ThingLookup<K, H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash, H: BuildHasher + Default> ThingLookup<K, H> {
    pub fn new() -> Self {
        Self {
            index: HashMap::<K, RoaringTreemap, H>::default(),
        }
    }
    pub fn insert(&mut self, key: K, thing: Thing) {
        let set = self.index.entry(key).or_default();
        set.insert(thing);
    }
    pub fn remove(&mut self, key: &K, thing: Thing) {
        if let Some(set) = self.index.get_mut(key) {
            set.remove(thing);
        }
    }
    pub fn lookup(&self, key: &K) -> Option<&RoaringTreemap> {
        self.index.get(key)
    }
}

// ------------- Database -------------
// This sets up the database with the necessary structures

pub type RoleAppearanceLookup = Lookup<Arc<Role>, Arc<Appearance>, OtherHasher>;
pub type AppearanceSetLookup = Lookup<Arc<Appearance>, Arc<AppearanceSet>, OtherHasher>;

/// Persistence configuration for the database engine.
pub enum PersistenceMode {
    /// Ephemeral engine: nothing is written, all data lost when dropped.
    InMemory,
    /// File-backed persistence using the append-only store directory at the provided path.
    #[cfg(feature = "persistence")]
    File(String),
}

impl PersistenceMode {
    /// Helper to derive a persistence mode from a flag + path string.
    /// If `enable` is false, returns InMemory regardless of path contents.
    pub fn from_config(enable: bool, _path: impl Into<String>) -> Self {
        #[cfg(feature = "persistence")]
        if enable {
            return PersistenceMode::File(_path.into());
        }
        #[cfg(not(feature = "persistence"))]
        let _ = enable;
        PersistenceMode::InMemory
    }
}

pub struct Database {
    pub(crate) execution_owner: Mutex<()>,
    // owns a thing generator
    pub(crate) thing_generator: Arc<Mutex<ThingGenerator>>,
    // owns keepers for the available constructs
    pub(crate) role_keeper: Arc<Mutex<RoleKeeper>>,
    pub(crate) appearance_keeper: Arc<Mutex<AppearanceKeeper>>,
    pub(crate) appearance_set_keeper: Arc<Mutex<AppearanceSetKeeper>>,
    pub(crate) posit_keeper: Arc<Mutex<PositKeeper>>,
    // owns lookups between constructs (similar to database indexes)
    pub(crate) thing_to_appearance_lookup: Arc<Mutex<Lookup<Thing, Arc<Appearance>, ThingHasher>>>,
    pub(crate) role_to_appearance_lookup: Arc<Mutex<RoleAppearanceLookup>>,
    pub(crate) appearance_to_appearance_set_lookup: Arc<Mutex<AppearanceSetLookup>>,
    pub(crate) appearance_set_to_posit_thing_lookup:
        Arc<Mutex<ThingLookup<Arc<AppearanceSet>, OtherHasher>>>,
    pub(crate) role_to_posit_thing_lookup: Arc<Mutex<ThingLookup<Thing, OtherHasher>>>,
    /// Reverse lookup: posit thing -> its appearance set (type-erased from value/time).
    pub(crate) posit_thing_to_appearance_set_lookup:
        Arc<Mutex<HashMap<Thing, Arc<AppearanceSet>, ThingHasher>>>,
    /// Type-erased index: posit thing -> its time (for generic time filtering)
    pub(crate) posit_time_lookup: Arc<Mutex<HashMap<Thing, Time, ThingHasher>>>,
    // Append-only persistence owner. `None` is the in-memory mode.
    #[cfg(feature = "persistence")]
    store: Arc<Mutex<Option<LogStore>>>,
}

impl Database {
    pub fn new(mode: PersistenceMode) -> Result<Database, DatabaseError> {
        #[cfg(feature = "persistence")]
        let (store, replayed) = match mode {
            PersistenceMode::InMemory => (None, None),
            PersistenceMode::File(path) => {
                let store = LogStore::open(&path)?;
                let replayed = store.replay_logical()?;
                (Some(store), Some(replayed))
            }
        };
        #[cfg(not(feature = "persistence"))]
        let _ = mode;
        // Create all the stuff that goes into a database engine
        let thing_generator = ThingGenerator::new();
        let role_keeper = RoleKeeper::new();
        let appearance_keeper = AppearanceKeeper::new();
        let appearance_set_keeper = AppearanceSetKeeper::new();
        let posit_keeper = PositKeeper::new();
        let thing_to_appearance_lookup = Lookup::new();
        let role_to_appearance_lookup = Lookup::new();
        let appearance_to_appearance_set_lookup = Lookup::new();
        let appearance_set_to_posit_thing_lookup = ThingLookup::new();
        let role_to_posit_thing_lookup = ThingLookup::new();
        let posit_thing_to_appearance_set_lookup: HashMap<Thing, Arc<AppearanceSet>, ThingHasher> =
            HashMap::default();
        let posit_time_lookup: HashMap<Thing, Time, ThingHasher> = HashMap::default();
        // Create the database so that we can prime it before returning it
        let database = Database {
            execution_owner: Mutex::new(()),
            thing_generator: Arc::new(Mutex::new(thing_generator)),
            role_keeper: Arc::new(Mutex::new(role_keeper)),
            appearance_keeper: Arc::new(Mutex::new(appearance_keeper)),
            appearance_set_keeper: Arc::new(Mutex::new(appearance_set_keeper)),
            posit_keeper: Arc::new(Mutex::new(posit_keeper)),
            thing_to_appearance_lookup: Arc::new(Mutex::new(thing_to_appearance_lookup)),
            role_to_appearance_lookup: Arc::new(Mutex::new(role_to_appearance_lookup)),
            appearance_to_appearance_set_lookup: Arc::new(Mutex::new(
                appearance_to_appearance_set_lookup,
            )),
            appearance_set_to_posit_thing_lookup: Arc::new(Mutex::new(
                appearance_set_to_posit_thing_lookup,
            )),
            role_to_posit_thing_lookup: Arc::new(Mutex::new(role_to_posit_thing_lookup)),
            posit_thing_to_appearance_set_lookup: Arc::new(Mutex::new(
                posit_thing_to_appearance_set_lookup,
            )),
            posit_time_lookup: Arc::new(Mutex::new(posit_time_lookup)),
            #[cfg(feature = "persistence")]
            store: Arc::new(Mutex::new(store)),
        };

        #[cfg(feature = "persistence")]
        if let Some(replayed) = replayed {
            if replayed.roles.is_empty() && replayed.posits.is_empty() && !replayed.has_raw_codec {
                database.bootstrap_append_store()?;
            } else {
                database.restore_append_store(replayed)?;
            }
        }
        #[cfg(not(feature = "persistence"))]
        {
            database.ensure_builtin_role(POSIT_ROLE_ID, "posit")?;
            database.ensure_builtin_role(ASCERTAINS_ROLE_ID, "ascertains")?;
        }
        #[cfg(feature = "persistence")]
        if database
            .role_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .is_empty()
        {
            database.ensure_builtin_role(POSIT_ROLE_ID, "posit")?;
            database.ensure_builtin_role(ASCERTAINS_ROLE_ID, "ascertains")?;
        }

        Ok(database)
    }

    #[cfg(feature = "persistence")]
    fn bootstrap_append_store(&self) -> Result<(), DatabaseError> {
        let posit = Role::new(POSIT_ROLE_ID, "posit".to_string(), true);
        let ascertains = Role::new(ASCERTAINS_ROLE_ID, "ascertains".to_string(), true);
        let records = vec![
            role_record(&posit),
            role_record(&ascertains),
            raw_codec_record(),
        ];
        self.store
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .as_mut()
            .ok_or_else(|| DatabaseError::Invariant("persistent store is absent".to_string()))?
            .append_batch(&records)?;
        self.thing_generator
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .retain(POSIT_ROLE_ID);
        self.thing_generator
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .retain(ASCERTAINS_ROLE_ID);
        self.keep_role(posit)?;
        self.keep_role(ascertains)?;
        Ok(())
    }

    #[cfg(feature = "persistence")]
    fn restore_append_store(&self, replayed: ReplayedStore) -> Result<(), DatabaseError> {
        if !replayed.has_raw_codec {
            return Err(DatabaseError::DataCorruption {
                message: "append-only store lacks the mandatory raw codec".to_string(),
            });
        }
        for role in replayed.roles {
            self.thing_generator
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .retain(role.identity);
            let (_, existed) =
                self.keep_role(Role::new(role.identity, role.name, role.reserved))?;
            if existed {
                return Err(DatabaseError::DataCorruption {
                    message: "duplicate Role survived logical replay".to_string(),
                });
            }
        }
        for posit in replayed.posits {
            let mut appearances = Vec::with_capacity(posit.appearances.len());
            for (role_identity, thing) in posit.appearances {
                let role = self
                    .role_keeper
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .lookup(&role_identity)
                    .ok_or_else(|| DatabaseError::DataCorruption {
                        message: format!("missing replayed Role {role_identity}"),
                    })?;
                self.thing_generator
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .retain(thing);
                appearances.push(self.keep_appearance(Appearance::new(thing, role))?.0);
            }
            let appearance_set = self.create_appearance_set(appearances)?.0;
            self.thing_generator
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .retain(posit.identity);
            let (_, existed) = self.keep_posit(Posit::new(
                posit.identity,
                appearance_set,
                posit.value,
                posit.time,
            ))?;
            if existed {
                return Err(DatabaseError::DataCorruption {
                    message: "duplicate Posit survived logical replay".to_string(),
                });
            }
        }
        self.ensure_builtin_role(POSIT_ROLE_ID, "posit")?;
        self.ensure_builtin_role(ASCERTAINS_ROLE_ID, "ascertains")?;
        Ok(())
    }
    // functions to access the owned generator and keepers
    pub(crate) fn thing_generator(&self) -> Arc<Mutex<ThingGenerator>> {
        Arc::clone(&self.thing_generator)
    }
    pub(crate) fn role_keeper(&self) -> Arc<Mutex<RoleKeeper>> {
        Arc::clone(&self.role_keeper)
    }
    pub(crate) fn posit_keeper(&self) -> Arc<Mutex<PositKeeper>> {
        Arc::clone(&self.posit_keeper)
    }
    pub(crate) fn role_to_posit_thing_lookup(&self) -> Arc<Mutex<ThingLookup<Thing, OtherHasher>>> {
        Arc::clone(&self.role_to_posit_thing_lookup)
    }
    pub(crate) fn posit_thing_to_appearance_set_lookup(
        &self,
    ) -> Arc<Mutex<HashMap<Thing, Arc<AppearanceSet>, ThingHasher>>> {
        Arc::clone(&self.posit_thing_to_appearance_set_lookup)
    }
    pub(crate) fn posit_time_lookup(&self) -> Arc<Mutex<HashMap<Thing, Time, ThingHasher>>> {
        Arc::clone(&self.posit_time_lookup)
    }

    pub fn flush(&self) -> Result<(), DatabaseError> {
        #[cfg(feature = "persistence")]
        if let Some(store) = self
            .store
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .as_mut()
        {
            store.flush()?;
        }
        Ok(())
    }

    /// Return whether the normalized, case-sensitive role catalog contains `name`.
    pub fn contains_role(&self, name: &str) -> Result<bool, DatabaseError> {
        let name: String = name.nfc().collect();
        Ok(self
            .role_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .get(&name)
            .is_some())
    }

    /// Return the number of roles in the catalog, including reserved roles.
    pub fn role_count(&self) -> Result<usize, DatabaseError> {
        Ok(self
            .role_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .len())
    }
    // functions to create constructs for the keepers to keep that also populate the lookups
    pub(crate) fn keep_role(&self, role: Role) -> Result<(Arc<Role>, bool), DatabaseError> {
        Ok(self
            .role_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .keep(role))
    }
    fn ensure_builtin_role(&self, identity: Thing, name: &str) -> Result<(), DatabaseError> {
        {
            let keeper = self
                .role_keeper
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?;
            if let Some(existing) = keeper.get(name) {
                if existing.role() != identity || !existing.reserved() {
                    return Err(DatabaseError::DataCorruption {
                        message: format!(
                            "built-in role '{name}' must be reserved identity {identity}, found identity {} (reserved={})",
                            existing.role(),
                            existing.reserved()
                        ),
                    });
                }
                return Ok(());
            }
            if let Some(existing) = keeper.lookup(&identity) {
                return Err(DatabaseError::DataCorruption {
                    message: format!(
                        "built-in role identity {identity} belongs to '{}', expected '{name}'",
                        existing.name()
                    ),
                });
            }
        }

        self.thing_generator
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .retain(identity);
        let (builtin_role, existed) =
            self.keep_role(Role::new(identity, name.to_string(), true))?;
        debug_assert!(!existed);
        let _ = builtin_role;
        Ok(())
    }
    pub(crate) fn create_roles(
        &self,
        roles: Vec<(String, bool)>,
    ) -> Result<Vec<(Arc<Role>, bool)>, DatabaseError> {
        enum Source {
            Existing(Arc<Role>),
            New(usize),
        }

        let mut prepared = Vec::new();
        let mut sources = Vec::with_capacity(roles.len());
        for (name, reserved) in roles {
            let canonical_name: String = name.nfc().collect();
            if canonical_name.is_empty() {
                return Err(DatabaseError::Execution(
                    "role name cannot be empty".to_string(),
                ));
            }
            if let Some(existing) = self
                .role_keeper
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .get(&canonical_name)
            {
                sources.push(Source::Existing(existing));
            } else if let Some(index) = prepared
                .iter()
                .position(|role: &Role| role.name() == canonical_name)
            {
                if prepared[index].reserved() != reserved {
                    return Err(DatabaseError::Execution(format!(
                        "role '{canonical_name}' has conflicting reserved status in one command"
                    )));
                }
                sources.push(Source::New(index));
            } else {
                let identity = self
                    .thing_generator
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .generate();
                let index = prepared.len();
                prepared.push(Role::new(identity, canonical_name, reserved));
                sources.push(Source::New(index));
            }
        }
        #[cfg(feature = "persistence")]
        if !prepared.is_empty() {
            let records: Vec<_> = prepared.iter().map(role_record).collect();
            let mut store = self
                .store
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?;
            if let Some(store) = store.as_mut() {
                store.append_batch(&records)?;
            }
        }
        let kept: Vec<Arc<Role>> = prepared
            .into_iter()
            .map(|role| {
                let (role, existed) = self.keep_role(role)?;
                debug_assert!(!existed);
                Ok(role)
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        Ok(sources
            .into_iter()
            .map(|source| match source {
                Source::Existing(role) => (role, true),
                Source::New(index) => (Arc::clone(&kept[index]), false),
            })
            .collect())
    }
    pub(crate) fn keep_appearance(
        &self,
        appearance: Appearance,
    ) -> Result<(Arc<Appearance>, bool), DatabaseError> {
        let (kept_appearance, previously_kept) = self
            .appearance_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .keep(appearance);
        if !previously_kept {
            self.thing_to_appearance_lookup
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .insert(kept_appearance.thing(), Arc::clone(&kept_appearance));
            if kept_appearance.role().reserved {
                self.role_to_appearance_lookup
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .insert(kept_appearance.role(), Arc::clone(&kept_appearance));
            }
        }
        Ok((kept_appearance, previously_kept))
    }
    pub(crate) fn create_appearance(
        &self,
        thing: Thing,
        role: Arc<Role>,
    ) -> Result<(Arc<Appearance>, bool), DatabaseError> {
        self.keep_appearance(Appearance::new(thing, role))
    }
    pub(crate) fn keep_appearance_set(
        &self,
        appearance_set: AppearanceSet,
    ) -> Result<(Arc<AppearanceSet>, bool), DatabaseError> {
        let (kept_appearance_set, previously_kept) = self
            .appearance_set_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .keep(appearance_set);
        if !previously_kept {
            for appearance in kept_appearance_set.appearances().iter() {
                self.appearance_to_appearance_set_lookup
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .insert(Arc::clone(appearance), Arc::clone(&kept_appearance_set));
            }
        }
        Ok((kept_appearance_set, previously_kept))
    }
    pub(crate) fn create_appearance_set(
        &self,
        appearance_set: Vec<Arc<Appearance>>,
    ) -> Result<(Arc<AppearanceSet>, bool), DatabaseError> {
        let appearance_set = AppearanceSet::new(appearance_set).ok_or_else(|| {
            DatabaseError::InvalidAppearanceSet(
                "an appearance set may contain at most one Thing for each Role".into(),
            )
        })?;
        self.keep_appearance_set(appearance_set)
    }
    #[cfg(feature = "persistence")]
    pub(crate) fn keep_posit<V: 'static + DataType>(
        &self,
        posit: Posit<V>,
    ) -> Result<(Arc<Posit<V>>, bool), DatabaseError> {
        self.keep_posit_arc(Arc::new(posit))
    }

    fn keep_posit_arc<V: 'static + DataType>(
        &self,
        posit: Arc<Posit<V>>,
    ) -> Result<(Arc<Posit<V>>, bool), DatabaseError> {
        let (kept_posit, previously_kept) = self
            .posit_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .keep_arc(posit);
        if !previously_kept {
            self.appearance_set_to_posit_thing_lookup
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .insert(kept_posit.appearance_set(), kept_posit.posit());
            self.posit_thing_to_appearance_set_lookup
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .insert(kept_posit.posit(), kept_posit.appearance_set());
            // Record posit time in type-erased lookup for generic time filtering
            self.posit_time_lookup
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .insert(kept_posit.posit(), kept_posit.time().clone());
            // Index posit thing by each role in its appearance set
            for appearance in kept_posit.appearance_set().appearances().iter() {
                let role_thing = appearance.role().role();
                self.role_to_posit_thing_lookup
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .insert(role_thing, kept_posit.posit());
            }
        }
        Ok((kept_posit, previously_kept))
    }
    pub(crate) fn create_literal_posits(
        &self,
        posits: Vec<(Arc<AppearanceSet>, LiteralValue, Time)>,
    ) -> Result<Vec<Arc<Posit<LiteralValue>>>, DatabaseError> {
        enum Source {
            Existing(Arc<Posit<LiteralValue>>),
            New(usize),
        }

        let mut prepared: Vec<Arc<Posit<LiteralValue>>> = Vec::new();
        let mut sources = Vec::with_capacity(posits.len());
        for (appearance_set, value, time) in posits {
            let identity = self
                .thing_generator
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .generate();
            let candidate = Arc::new(Posit::new(identity, appearance_set, value, time));
            if let Some(existing) = self
                .posit_keeper
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .existing(&candidate)
            {
                self.thing_generator
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .release(identity);
                sources.push(Source::Existing(existing));
            } else if let Some(index) = prepared.iter().position(|existing| existing == &candidate)
            {
                self.thing_generator
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))?
                    .release(identity);
                sources.push(Source::New(index));
            } else {
                let index = prepared.len();
                prepared.push(candidate);
                sources.push(Source::New(index));
            }
        }

        #[cfg(feature = "persistence")]
        if !prepared.is_empty() {
            let records = prepared
                .iter()
                .map(|posit| {
                    posit_record_parts(
                        posit.posit(),
                        &posit.appearance_set(),
                        posit.value(),
                        posit.time(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut store = self
                .store
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?;
            if let Some(store) = store.as_mut() {
                store.append_batch(&records)?;
            }
        }

        let kept: Vec<Arc<Posit<LiteralValue>>> = prepared
            .into_iter()
            .map(|posit| {
                let (posit, existed) = self.keep_posit_arc(posit)?;
                debug_assert!(!existed);
                Ok(posit)
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        Ok(sources
            .into_iter()
            .map(|source| match source {
                Source::Existing(posit) => posit,
                Source::New(index) => Arc::clone(&kept[index]),
            })
            .collect())
    }
}

#[cfg(all(test, feature = "persistence"))]
mod append_store_tests {
    use super::*;
    use crate::traqula::Engine;

    #[test]
    fn mutating_commands_append_one_atomic_batch_each() {
        let path = std::env::temp_dir().join(format!(
            "positorium-command-batches-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let database =
            Database::new(PersistenceMode::File(path.to_string_lossy().into_owned())).unwrap();
        Engine::new(&database)
            .execute_collect(
                "add role left, right; add posit [{(+a, left)}, 1, @NOW], [{(+b, right)}, 2, @NOW];",
            )
            .unwrap();

        let store = database.store.lock().unwrap();
        let batches = store.as_ref().unwrap().replay();
        assert_eq!(batches.len(), 3, "bootstrap plus two commands");
        assert_eq!(batches[0].len(), 3, "built-ins and raw codec");
        assert_eq!(batches[1].len(), 2, "both roles commit together");
        assert_eq!(batches[2].len(), 2, "both posits commit together");
        drop(store);
        drop(database);

        let restored =
            Database::new(PersistenceMode::File(path.to_string_lossy().into_owned())).unwrap();
        assert_eq!(restored.posit_keeper.lock().unwrap().len(), 2);
        drop(restored);
        let _ = std::fs::remove_dir_all(path);
    }
}
