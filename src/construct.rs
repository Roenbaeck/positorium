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
//! Create a role, a thing, an appearance, an appearance set and finally a
//! posit with a string value and time.
//! ```
//! use positorium::construct::{Database, PersistenceMode};
//! use positorium::datatype::Time;
//! // Pure in-memory (no SQLite persistence)
//! let db = Database::new(PersistenceMode::InMemory).unwrap();
//! let (role, _) = db.create_role("person".to_string(), false);
//! let thing = db.create_thing();
//! let (appearance, _) = db.create_apperance(*thing, role);
//! let (appearance_set, _) = db.create_appearance_set(vec![appearance]);
//! let time = Time::new();
//! let posit = db.create_posit(appearance_set, String::from("Alice"), time.clone());
//! assert_eq!(posit.value(), &"Alice".to_string());
//! ```
use crate::datatype::{DataType, Time};
use crate::persist::Persistor;
use tracing::{warn};
use crate::error::DatabaseError;
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

pub type ThingHasher = BuildHasherDefault<SeaHasher>;
pub type OtherHasher = BuildHasherDefault<SeaHasher>;

#[derive(Debug)]
pub struct ThingGenerator {
    lower_bound: Thing,
    retained: HashSet<Thing, ThingHasher>,
    released: Vec<Thing>,
}

impl ThingGenerator {
    /// Creates a fresh generator with the lower bound set to [`GENESIS`].
    pub fn new() -> Self {
        Self {
            lower_bound: GENESIS,
            retained: HashSet::<Thing, ThingHasher>::default(),
            released: Vec::new(),
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
    /// Releases an identity making it available for reuse.
    pub fn release(&mut self, t: Thing) {
        if self.retained.remove(&t) {
            self.released.push(t);
        }
    }
    /// Generates a new (or recycled) identity.
    pub fn generate(&mut self) -> Thing {
        self.released.pop().unwrap_or_else(|| {
            self.lower_bound += 1;
            self.retained.insert(self.lower_bound);
            self.lower_bound
        })
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
            name,
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
        self.name.cmp(&other.name)
    }
}
impl PartialOrd for Role {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Role {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Hash for Role {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.to_uppercase().hash(state);
        self.reserved.hash(state);
    }
}

#[derive(Debug)]
pub struct RoleKeeper {
    kept: HashMap<String, Arc<Role>, OtherHasher>,
    lookup: HashMap<Thing, Arc<Role>, ThingHasher>, // double indexing, but roles should be few so it's not a big deal
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
    pub fn get(&self, name: &str) -> Arc<Role> {
        Arc::clone(self.kept.get(name).unwrap())
    }
    pub fn lookup(&self, role: &Thing) -> Arc<Role> {
        Arc::clone(self.lookup.get(role).unwrap())
    }
    pub fn len(&self) -> usize {
        self.kept.len()
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
}

// --------------- Posit ----------------
#[derive(Eq, PartialOrd, Ord, Debug)]
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
            self.posit,
            self.appearance_set,
            self.value.to_string() + "::<" + self.value.data_type() + ">",
            self.time.to_string() + "::<" + self.time.data_type() + ">"
        )
    }
}

// This key needs to be defined in order to store posits in a TypeMap.

pub struct PositKeeper {
    kept: TypeMap,
    length: usize,
}
impl PositKeeper {
    pub fn new() -> Self {
        Self {
            kept: TypeMap::new(),
            length: 0,
        }
    }
    pub fn keep<V: 'static + DataType>(&mut self, posit: Posit<V>) -> (Arc<Posit<V>>, bool) {
        // ensure the map can work with this particular type combo
        let map = if let Some(m) = self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>() {
            m
        } else {
            self.kept
                .insert::<BiMap<Arc<Posit<V>>, Thing>>(BiMap::<Arc<Posit<V>>, Thing>::new());
            self.kept.get_mut::<BiMap<Arc<Posit<V>>, Thing>>().unwrap()
        };
        let keepsake_thing = posit.posit();
        let keepsake = Arc::new(posit);
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
}

// ------------- Lookups -------------
#[derive(Debug)]
pub struct Lookup<K, V, H = RandomState> {
    index: HashMap<K, HashSet<V>, H>,
}
impl<K: Eq + Hash, V: Eq + Hash, H: BuildHasher + Default> Lookup<K, V, H> {
    pub fn new() -> Self {
        Self {
            index: HashMap::<K, HashSet<V>, H>::default(),
        }
    }
    pub fn insert(&mut self, key: K, value: V) {
        let map = self.index.entry(key).or_insert(HashSet::<V>::new());
        map.insert(value);
    }
    pub fn lookup(&self, key: &K) -> &HashSet<V> {
        self.index.get(key).unwrap()
    }
}

/// Lookup mapping a key to a set of Thing IDs, backed by a RoaringTreemap.
#[derive(Debug)]
pub struct ThingLookup<K, H = RandomState> {
    index: HashMap<K, RoaringTreemap, H>,
}
impl<K: Eq + Hash, H: BuildHasher + Default> ThingLookup<K, H> {
    pub fn new() -> Self {
        Self {
            index: HashMap::<K, RoaringTreemap, H>::default(),
        }
    }
    pub fn insert(&mut self, key: K, thing: Thing) {
        let set = self.index.entry(key).or_insert(RoaringTreemap::new());
        set.insert(thing);
    }
    pub fn remove(&mut self, key: &K, thing: Thing) {
        if let Some(set) = self.index.get_mut(key) {
            set.remove(thing);
        }
    }
    pub fn lookup(&self, key: &K) -> &RoaringTreemap {
        self.index.get(key).unwrap()
    }
}

// ------------- Database -------------
// This sets up the database with the necessary structures

/// Persistence configuration for the database engine.
pub enum PersistenceMode {
    /// Ephemeral engine: nothing is written, all data lost when dropped.
    InMemory,
    /// File-backed persistence using a SQLite database at the provided path.
    File(String),
}

impl PersistenceMode {
    /// Helper to derive a persistence mode from a flag + path string.
    /// If `enable` is false, returns InMemory regardless of path contents.
    pub fn from_config(enable: bool, path: impl Into<String>) -> Self {
        if enable {
            PersistenceMode::File(path.into())
        } else {
            PersistenceMode::InMemory
        }
    }
}

pub struct Database {
    // owns a thing generator
    pub thing_generator: Arc<Mutex<ThingGenerator>>,
    // owns keepers for the available constructs
    pub role_keeper: Arc<Mutex<RoleKeeper>>,
    pub appearance_keeper: Arc<Mutex<AppearanceKeeper>>,
    pub appearance_set_keeper: Arc<Mutex<AppearanceSetKeeper>>,
    pub posit_keeper: Arc<Mutex<PositKeeper>>,
    // owns lookups between constructs (similar to database indexes)
    pub thing_to_appearance_lookup: Arc<Mutex<Lookup<Thing, Arc<Appearance>, ThingHasher>>>,
    pub role_to_appearance_lookup: Arc<Mutex<Lookup<Arc<Role>, Arc<Appearance>, OtherHasher>>>,
    pub appearance_to_appearance_set_lookup:
        Arc<Mutex<Lookup<Arc<Appearance>, Arc<AppearanceSet>, OtherHasher>>>,
    pub appearance_set_to_posit_thing_lookup:
        Arc<Mutex<ThingLookup<Arc<AppearanceSet>, OtherHasher>>>,
    pub role_to_posit_thing_lookup: Arc<Mutex<ThingLookup<Thing, OtherHasher>>>,
    /// Reverse lookup: posit thing -> its appearance set (type-erased from value/time).
    pub posit_thing_to_appearance_set_lookup:
        Arc<Mutex<HashMap<Thing, Arc<AppearanceSet>, ThingHasher>>>,
    pub role_name_to_data_type_lookup: Arc<Mutex<Lookup<Vec<String>, String, OtherHasher>>>,
    /// Type-erased index: posit thing -> its time (for generic time filtering)
    pub posit_time_lookup: Arc<Mutex<HashMap<Thing, Time, ThingHasher>>>,
    // responsible for the the persistence layer
    pub persistor: Arc<Mutex<Persistor>>,
}

impl Database {
    pub fn new(mode: PersistenceMode) -> Result<Database, DatabaseError> {
        let persistor = match mode {
            PersistenceMode::InMemory => Persistor::new_no_persistence(),
            PersistenceMode::File(path) => Persistor::new_from_file(&path)?,
        };
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
        let role_name_to_data_type_lookup = Lookup::new();
        let posit_thing_to_appearance_set_lookup: HashMap<Thing, Arc<AppearanceSet>, ThingHasher> =
            HashMap::default();
        let posit_time_lookup: HashMap<Thing, Time, ThingHasher> = HashMap::default();
        let persistor = persistor;

        // Create the database so that we can prime it before returning it
        let database = Database {
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
            role_name_to_data_type_lookup: Arc::new(Mutex::new(role_name_to_data_type_lookup)),
            posit_time_lookup: Arc::new(Mutex::new(posit_time_lookup)),
            persistor: Arc::new(Mutex::new(persistor)),
        };

        // Restore the existing database
    if let Err(e) = database.persistor.lock().unwrap().restore_things(&database) { warn!(?e, "restore_things failed"); }
    if let Err(e) = database.persistor.lock().unwrap().restore_roles(&database) { warn!(?e, "restore_roles failed"); }
    if let Err(e) = database.persistor.lock().unwrap().restore_posits(&database) { warn!(?e, "restore_posits failed"); }
    if let Err(e) = database.persistor.lock().unwrap().verify_integrity() { warn!(?e, "verify_integrity reported issue"); }

        // Reserve some roles that will be necessary for implementing features
        // commonly found in many other (including non-tradtional) databases.
        database.create_role(String::from("posit"), true);
        database.create_role(String::from("ascertains"), true);
        database.create_role(String::from("thing"), true);
        database.create_role(String::from("classification"), true);

        Ok(database)
    }
    // functions to access the owned generator and keepers
    pub fn thing_generator(&self) -> Arc<Mutex<ThingGenerator>> {
        Arc::clone(&self.thing_generator)
    }
    pub fn role_keeper(&self) -> Arc<Mutex<RoleKeeper>> {
        Arc::clone(&self.role_keeper)
    }
    pub fn appearance_keeper(&self) -> Arc<Mutex<AppearanceKeeper>> {
        Arc::clone(&self.appearance_keeper)
    }
    pub fn appearance_set_keeper(&self) -> Arc<Mutex<AppearanceSetKeeper>> {
        Arc::clone(&self.appearance_set_keeper)
    }
    pub fn posit_keeper(&self) -> Arc<Mutex<PositKeeper>> {
        Arc::clone(&self.posit_keeper)
    }
    pub fn thing_to_appearance_lookup(
        &self,
    ) -> Arc<Mutex<Lookup<Thing, Arc<Appearance>, ThingHasher>>> {
        Arc::clone(&self.thing_to_appearance_lookup)
    }
    pub fn role_to_appearance_lookup(
        &self,
    ) -> Arc<Mutex<Lookup<Arc<Role>, Arc<Appearance>, OtherHasher>>> {
        Arc::clone(&self.role_to_appearance_lookup)
    }
    pub fn role_name_to_data_type_lookup(
        &self,
    ) -> Arc<Mutex<Lookup<Vec<String>, String, OtherHasher>>> {
        Arc::clone(&self.role_name_to_data_type_lookup)
    }
    pub fn appearance_to_appearance_set_lookup(
        &self,
    ) -> Arc<Mutex<Lookup<Arc<Appearance>, Arc<AppearanceSet>, OtherHasher>>> {
        Arc::clone(&self.appearance_to_appearance_set_lookup)
    }
    pub fn appearance_set_to_posit_thing_lookup(
        &self,
    ) -> Arc<Mutex<ThingLookup<Arc<AppearanceSet>, OtherHasher>>> {
        Arc::clone(&self.appearance_set_to_posit_thing_lookup)
    }
    pub fn role_to_posit_thing_lookup(&self) -> Arc<Mutex<ThingLookup<Thing, OtherHasher>>> {
        Arc::clone(&self.role_to_posit_thing_lookup)
    }
    pub fn posit_thing_to_appearance_set_lookup(
        &self,
    ) -> Arc<Mutex<HashMap<Thing, Arc<AppearanceSet>, ThingHasher>>> {
        Arc::clone(&self.posit_thing_to_appearance_set_lookup)
    }
    pub fn posit_time_lookup(&self) -> Arc<Mutex<HashMap<Thing, Time, ThingHasher>>> {
        Arc::clone(&self.posit_time_lookup)
    }
    pub fn create_thing(&self) -> Arc<Thing> {
        let thing = self.thing_generator.lock().unwrap().generate();
        if let Err(e) = self.persistor.lock().unwrap().persist_thing(&thing) { warn!(?e, "persist_thing failed"); }
        Arc::new(thing)
    }
    // functions to create constructs for the keepers to keep that also populate the lookups
    pub fn keep_role(&self, role: Role) -> (Arc<Role>, bool) {
        let (kept_role, previously_kept) = self.role_keeper.lock().unwrap().keep(role);
        (kept_role, previously_kept)
    }
    pub fn create_role(&self, role_name: String, reserved: bool) -> (Arc<Role>, bool) {
        let role_thing = self.thing_generator.lock().unwrap().generate();
        let (kept_role, previously_kept) =
            self.keep_role(Role::new(role_thing, role_name, reserved));
        if !previously_kept {
            if let Err(e) = self.persistor.lock().unwrap().persist_thing(&kept_role.role()) { warn!(?e, "persist_thing(role) failed"); }
            if let Err(e) = self.persistor.lock().unwrap().persist_role(&kept_role) { warn!(?e, "persist_role failed"); }
        } else {
            self.thing_generator.lock().unwrap().release(role_thing);
        }
        (kept_role, previously_kept)
    }
    pub fn keep_appearance(&self, appearance: Appearance) -> (Arc<Appearance>, bool) {
        let (kept_appearance, previously_kept) =
            self.appearance_keeper.lock().unwrap().keep(appearance);
        if !previously_kept {
            self.thing_to_appearance_lookup
                .lock()
                .unwrap()
                .insert(kept_appearance.thing(), Arc::clone(&kept_appearance));
            if kept_appearance.role().reserved {
                self.role_to_appearance_lookup
                    .lock()
                    .unwrap()
                    .insert(kept_appearance.role(), Arc::clone(&kept_appearance));
            }
        }
        (kept_appearance, previously_kept)
    }
    pub fn create_apperance(&self, thing: Thing, role: Arc<Role>) -> (Arc<Appearance>, bool) {
        self.keep_appearance(Appearance::new(thing, role))
    }
    pub fn keep_appearance_set(&self, appearance_set: AppearanceSet) -> (Arc<AppearanceSet>, bool) {
        let (kept_appearance_set, previously_kept) = self
            .appearance_set_keeper
            .lock()
            .unwrap()
            .keep(appearance_set);
        if !previously_kept {
            for appearance in kept_appearance_set.appearances().iter() {
                self.appearance_to_appearance_set_lookup
                    .lock()
                    .unwrap()
                    .insert(Arc::clone(appearance), Arc::clone(&kept_appearance_set));
            }
        }
        (kept_appearance_set, previously_kept)
    }
    pub fn create_appearance_set(
        &self,
        appearance_set: Vec<Arc<Appearance>>,
    ) -> (Arc<AppearanceSet>, bool) {
        self.keep_appearance_set(AppearanceSet::new(appearance_set).unwrap())
    }
    pub fn keep_posit<V: 'static + DataType>(&self, posit: Posit<V>) -> (Arc<Posit<V>>, bool) {
        let (kept_posit, previously_kept) = self.posit_keeper.lock().unwrap().keep(posit);
        if !previously_kept {
            self.role_name_to_data_type_lookup.lock().unwrap().insert(
                kept_posit.appearance_set().roles(),
                V::DATA_TYPE.to_string(),
            );
            self.appearance_set_to_posit_thing_lookup
                .lock()
                .unwrap()
                .insert(kept_posit.appearance_set(), kept_posit.posit());
            self.posit_thing_to_appearance_set_lookup
                .lock()
                .unwrap()
                .insert(kept_posit.posit(), kept_posit.appearance_set());
            // Record posit time in type-erased lookup for generic time filtering
            self.posit_time_lookup
                .lock()
                .unwrap()
                .insert(kept_posit.posit(), kept_posit.time().clone());
            // Index posit thing by each role in its appearance set
            for appearance in kept_posit.appearance_set().appearances().iter() {
                let role_thing = appearance.role().role();
                self.role_to_posit_thing_lookup
                    .lock()
                    .unwrap()
                    .insert(role_thing, kept_posit.posit());
            }
        }
        (kept_posit, previously_kept)
    }
    pub fn create_posit<V: 'static + DataType>(
        &self,
        appearance_set: Arc<AppearanceSet>,
        value: V,
        time: Time,
    ) -> Arc<Posit<V>> {
        let posit_thing = self.thing_generator.lock().unwrap().generate();
        let (kept_posit, previously_kept) =
            self.keep_posit(Posit::new(posit_thing, appearance_set, value, time));
        if !previously_kept {
            if let Err(e) = self.persistor.lock().unwrap().persist_thing(&kept_posit.posit()) { warn!(?e, "persist_thing(posit) failed"); }
            if let Err(e) = self.persistor.lock().unwrap().persist_posit(&kept_posit) { warn!(?e, "persist_posit failed"); }
        } else {
            self.thing_generator.lock().unwrap().release(posit_thing);
        }
        kept_posit
    }
}
