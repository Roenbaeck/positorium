//! Traqula query & mutation language engine.
//!
//! Recent capabilities of note:
//! * Multi-result collection: `Engine::execute_collect_multi` returns one result set per `search` in a script.
//! * Variable–variable predicates:
//!   - Time vs time comparisons (e.g. `where t1 < t2`).
//!   - Value vs value comparisons for numbers, decimals, certainties, and strings (equality only for strings).
//! * Certainty literals are percent-only (`75%`, `-10%`); bare numbers like `0.75` no longer auto-convert.
//! * Ordering on certainty variables requires both sides to be certainties (percent forms); mixed certainty/numeric ordering yields an execution error mentioning a missing percent sign.
//! * Numeric ordering/equality supports `i64` and `Decimal` interop (coerced during comparison).
//! * Execution errors surface unknown variables and mismatched ordering types early, halting evaluation.
//!
//! These enhancements are intentionally conservative: unsupported comparisons are rejected with clear errors rather than coerced implicitly.
//!
//! This module provides a rudimentary parser (Pest based) and executor for a
//! domain specific language used to:
//! * add roles
//! * insert ("posit") propositions
//! * perform simple pattern based searches over existing posits
//!
//! The language is defined in the grammar file `traqula.pest`. Commands are
//! parsed into a tree of `Pair<Rule>` values which the [`Engine`] walks to
//! mutate the in-memory [`Database`].
//!
//! # Result Sets
//! Internally query evaluation uses a compact tri-state result representation
//! [`ResultSetMode`]:
//! * `Empty` – no hits
//! * `Thing` – exactly one identity
//! * `Multi` – a roaring bitmap of identities
//!
//! This allows set operations (intersection, union, difference, symmetric
//! difference) to be implemented efficiently without premature allocation.
//!
//! # Example (executing Traqula)
//! ```
//! use positorium::construct::{Database, PersistenceMode};
//! use positorium::traqula::Engine;
//! let db = Database::new(PersistenceMode::InMemory).unwrap();
//! let engine = Engine::new(&db);
//! engine
//!     .execute("add role person; add posit [{(+a, person)}, \"Alice\", @NOW];")
//!     .unwrap();
//! ```
//!
//! NOTE: The search functionality is still evolving; many captured variables
//! are currently parsed but not yet materialized into final query outputs.
//! Debug logging is gated behind `cfg(debug_assertions)` where appropriate.
use crate::construct::{Database, OtherHasher, Thing};
use crate::datatype::Time;
use crate::error::DatabaseError;
use crate::literal::{LiteralFamily, LiteralValue};
use chrono::NaiveDateTime; // needed for defensive datetime validation in parse_time
// (regex-based time parsing removed in favor of direct parsing)
use chrono::NaiveDate;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{
    Arc, MutexGuard, TryLockError,
    atomic::{AtomicBool, Ordering as AtomicOrdering},
};
use std::time::{Duration, Instant};

// used for internal result sets
use roaring::RoaringTreemap;
use tracing::info;
// (Bit*Assign imports previously used by legacy toggle-based set ops removed)

type Variables = HashMap<String, ResultSet, OtherHasher>;

/// Version of the accepted Traqula source grammar and semantics.
pub const TRAQULA_VERSION: u16 = 1;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum ResultSetMode {
    Empty,
    Thing,
    Multi,
}

/// Compact set abstraction used during query evaluation.
///
/// Public fields allow light‑weight pattern matching by the engine. External
/// crates should treat this as opaque and rely on future higher level APIs.
#[derive(Debug, Clone)]
pub struct ResultSet {
    pub mode: ResultSetMode,
    pub thing: Option<Thing>,
    pub multi: Option<RoaringTreemap>,
}
impl Default for ResultSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultSet {
    pub fn new() -> Self {
        Self {
            mode: ResultSetMode::Empty,
            thing: None,
            multi: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.mode == ResultSetMode::Empty
    }
    fn empty(&mut self) {
        self.mode = ResultSetMode::Empty;
        self.thing = None;
        self.multi = None;
    }
    fn thing(&mut self, thing: Thing) {
        self.mode = ResultSetMode::Thing;
        self.thing = Some(thing);
        self.multi = None;
    }
    fn multi(&mut self, multi: RoaringTreemap) {
        self.mode = ResultSetMode::Multi;
        self.thing = None;
        self.multi = Some(multi);
    }
    /// Insert a single thing into this result set (idempotent; preserves ordering semantics).
    pub fn insert(&mut self, thing: Thing) {
        match self.mode {
            ResultSetMode::Empty => self.thing(thing),
            ResultSetMode::Thing => {
                if self.thing.unwrap() == thing {
                    return;
                }
                let mut multi = RoaringTreemap::new();
                multi.insert(self.thing.unwrap());
                multi.insert(thing);
                self.multi(multi);
            }
            ResultSetMode::Multi => {
                self.multi.as_mut().unwrap().insert(thing);
            }
        }
    }
    /// Insert many things from a bitmap into this result set.
    pub fn insert_many(&mut self, bitmap: &RoaringTreemap) {
        match self.mode {
            ResultSetMode::Empty => match bitmap.len() {
                0 => {}
                1 => {
                    let t = bitmap.min().unwrap();
                    self.thing(t);
                }
                _ => {
                    let mut clone = RoaringTreemap::new();
                    clone.clone_from(bitmap);
                    self.multi(clone);
                }
            },
            ResultSetMode::Thing => {
                let existing = self.thing.unwrap();
                if bitmap.is_empty() {
                    return;
                }
                if bitmap.len() == 1 {
                    let only = bitmap.min().unwrap();
                    if only == existing {
                        return;
                    }
                    let mut multi = RoaringTreemap::new();
                    multi.insert(existing);
                    multi.insert(only);
                    self.multi(multi);
                } else {
                    let mut multi = RoaringTreemap::new();
                    multi.clone_from(bitmap);
                    multi.insert(existing); // harmless if already present
                    self.multi(multi);
                }
            }
            ResultSetMode::Multi => {
                if bitmap.is_empty() {
                    return;
                }
                let multi = self.multi.as_mut().unwrap();
                *multi |= bitmap;
            }
        }
    }
    fn intersect_with(&mut self, other: &ResultSet) {
        if self.mode == ResultSetMode::Empty || other.mode == ResultSetMode::Empty {
            self.empty();
            return;
        }
        match (self.mode, other.mode) {
            (ResultSetMode::Thing, ResultSetMode::Thing)
                if self.thing.unwrap() != other.thing.unwrap() =>
            {
                self.empty();
            }
            (ResultSetMode::Thing, ResultSetMode::Multi) => {
                let t = self.thing.unwrap();
                if !other.multi.as_ref().unwrap().contains(t) {
                    self.empty();
                }
            }
            (ResultSetMode::Multi, ResultSetMode::Thing) => {
                let t = other.thing.unwrap();
                if self.multi.as_ref().unwrap().contains(t) {
                    self.thing(t);
                } else {
                    self.empty();
                }
            }
            (ResultSetMode::Multi, ResultSetMode::Multi) => {
                let other_multi = other.multi.as_ref().unwrap();
                let multi = self.multi.as_mut().unwrap();
                *multi &= other_multi;
                match multi.len() {
                    0 => self.empty(),
                    1 => {
                        let t = multi.min().unwrap();
                        self.thing(t);
                    }
                    _ => (),
                }
            }
            _ => {}
        }
    }
    fn union_with(&mut self, other: &ResultSet) {
        if other.mode != ResultSetMode::Empty {
            match (&self.mode, &other.mode) {
                (ResultSetMode::Empty, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    self.thing(other_thing);
                }
                (ResultSetMode::Empty, ResultSetMode::Multi) => {
                    let other_multi = other.multi.as_ref().unwrap();
                    let mut multi = RoaringTreemap::new();
                    multi.clone_from(other_multi);
                    self.multi(multi);
                }
                (ResultSetMode::Thing, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    let mut multi = RoaringTreemap::new();
                    multi.insert(other_thing);
                    multi.insert(self.thing.unwrap());
                    self.multi(multi);
                }
                (ResultSetMode::Thing, ResultSetMode::Multi) => {
                    let other_multi = other.multi.as_ref().unwrap();
                    let mut multi = RoaringTreemap::new();
                    multi.clone_from(other_multi);
                    multi.insert(self.thing.unwrap());
                    self.multi(multi);
                }
                (ResultSetMode::Multi, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    self.multi.as_mut().unwrap().insert(other_thing);
                }
                (ResultSetMode::Multi, ResultSetMode::Multi) => {
                    let other_multi = other.multi.as_ref().unwrap();
                    *self.multi.as_mut().unwrap() |= other_multi;
                }
                (_, _) => (),
            }
        }
    }
    fn difference_with(&mut self, other: &ResultSet) {
        if other.mode != ResultSetMode::Empty && self.mode != ResultSetMode::Empty {
            match (&self.mode, &other.mode) {
                (ResultSetMode::Thing, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    if self.thing.unwrap() == other_thing {
                        self.empty();
                    }
                }
                (ResultSetMode::Thing, ResultSetMode::Multi) => {
                    let other_multi = other.multi.as_ref().unwrap();
                    if other_multi.contains(self.thing.unwrap()) {
                        self.empty();
                    }
                }
                (ResultSetMode::Multi, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    let multi = self.multi.as_mut().unwrap();
                    multi.remove(other_thing);
                    match multi.len() {
                        0 => {
                            self.empty();
                        }
                        1 => {
                            let thing = multi.min().unwrap();
                            self.thing(thing);
                        }
                        _ => (),
                    }
                }
                (ResultSetMode::Multi, ResultSetMode::Multi) => {
                    let other_multi = other.multi.as_ref().unwrap();
                    let multi = self.multi.as_mut().unwrap();
                    *multi -= other_multi;
                    match multi.len() {
                        0 => {
                            self.empty();
                        }
                        1 => {
                            let thing = multi.min().unwrap();
                            self.thing(thing);
                        }
                        _ => (),
                    }
                }
                (_, _) => (),
            }
        }
    }
    fn symmetric_difference_with(&mut self, other: &ResultSet) {
        if other.mode != ResultSetMode::Empty && self.mode != ResultSetMode::Empty {
            match (&self.mode, &other.mode) {
                (ResultSetMode::Thing, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    if self.thing.unwrap() == other_thing {
                        self.empty();
                    } else {
                        let mut multi = RoaringTreemap::new();
                        multi.insert(other_thing);
                        multi.insert(self.thing.unwrap());
                        self.multi(multi);
                    }
                }
                (ResultSetMode::Thing, ResultSetMode::Multi) => {
                    let other_multi = other.multi.as_ref().unwrap();
                    let mut multi = RoaringTreemap::new();
                    multi.clone_from(other_multi);
                    let thing = self.thing.unwrap();
                    if other_multi.contains(self.thing.unwrap()) {
                        multi.remove(thing);
                    } else {
                        multi.insert(thing);
                    }
                    match multi.len() {
                        0 => {
                            self.empty();
                        }
                        1 => {
                            let thing = multi.min().unwrap();
                            self.thing(thing);
                        }
                        _ => {
                            self.multi(multi);
                        }
                    }
                }
                (ResultSetMode::Multi, ResultSetMode::Thing) => {
                    let other_thing = other.thing.unwrap();
                    let multi = self.multi.as_mut().unwrap();
                    if multi.contains(other_thing) {
                        multi.remove(other_thing);
                    } else {
                        multi.insert(other_thing);
                    }
                    match multi.len() {
                        0 => {
                            self.empty();
                        }
                        1 => {
                            let thing = multi.min().unwrap();
                            self.thing(thing);
                        }
                        _ => (),
                    }
                }
                (ResultSetMode::Multi, ResultSetMode::Multi) => { /* unreachable legacy block retained removed */
                }
                (_, _) => (),
            }
        }
    }
}

// Operator-assign support to preserve public API used by benches
impl std::ops::BitAndAssign<&ResultSet> for ResultSet {
    fn bitand_assign(&mut self, rhs: &ResultSet) {
        self.intersect_with(rhs);
    }
}
impl std::ops::BitOrAssign<&ResultSet> for ResultSet {
    fn bitor_assign(&mut self, rhs: &ResultSet) {
        self.union_with(rhs);
    }
}
impl std::ops::SubAssign<&ResultSet> for ResultSet {
    fn sub_assign(&mut self, rhs: &ResultSet) {
        self.difference_with(rhs);
    }
}
impl std::ops::BitXorAssign<&ResultSet> for ResultSet {
    fn bitxor_assign(&mut self, rhs: &ResultSet) {
        self.symmetric_difference_with(rhs);
    }
}

// search functions in order to find posits matching certain circumstances
/// Collects the identities of all posits whose appearance sets involve the
/// supplied thing.
pub fn posits_involving_thing(database: &Database, thing: Thing) -> ResultSet {
    let mut result_set = ResultSet::new();
    let thing_lookup = database.thing_to_appearance_lookup.lock().unwrap();
    if let Some(appearances) = thing_lookup.lookup(&thing) {
        for appearance in appearances {
            let appearance_lookup = database.appearance_to_appearance_set_lookup.lock().unwrap();
            if let Some(appearance_sets) = appearance_lookup.lookup(appearance) {
                for appearance_set in appearance_sets {
                    let guard = database
                        .appearance_set_to_posit_thing_lookup
                        .lock()
                        .unwrap();
                    if let Some(bitmap) = guard.lookup(appearance_set) {
                        result_set.insert_many(bitmap);
                    }
                }
            }
        }
    }
    result_set
}

fn parse_lossless_literal(
    rule: Rule,
    token: &str,
    resolved_now: &Time,
) -> Result<LiteralValue, DatabaseError> {
    let (token, family) = match rule {
        Rule::string => (token.to_string(), LiteralFamily::String),
        Rule::int => (token.to_string(), LiteralFamily::Integer),
        Rule::decimal => (token.to_string(), LiteralFamily::Decimal),
        Rule::certainty => (token.to_string(), LiteralFamily::Certainty),
        Rule::json => (token.to_string(), LiteralFamily::Json),
        Rule::time => (token.to_string(), LiteralFamily::Time),
        Rule::constant => {
            let resolved = match token.trim() {
                "@NOW" => format!("'{}'", resolved_now),
                "@BOT" | "@EOT" => token.trim().to_string(),
                other => {
                    return Err(DatabaseError::Execution(format!(
                        "unknown value constant '{other}'"
                    )));
                }
            };
            (resolved, LiteralFamily::Time)
        }
        _ => {
            return Err(DatabaseError::Invariant(format!(
                "grammar rule {rule:?} is not a literal"
            )));
        }
    };
    LiteralValue::new(token, family).map_err(DatabaseError::Execution)
}
/// Parse a time literal or constant used in Traqula.
pub fn parse_time(value: &str) -> Option<Time> {
    parse_time_with_now(value, &Time::new())
}

fn parse_time_with_now(value: &str, resolved_now: &Time) -> Option<Time> {
    // 1. Fast path for constants (@NOW etc.)
    if let Some(t) = parse_time_constant_with_now(value, resolved_now) {
        return Some(t);
    }

    // 2. Sanitize: trim whitespace & trailing syntax punctuation, remove surrounding single quotes if any
    let mut stripped = value
        .trim()
        .trim_end_matches([',', ';', ']', ')'])
        .trim()
        .to_string();
    if stripped.starts_with('\'') && stripped.ends_with('\'') && stripped.len() >= 2 {
        stripped = stripped[1..stripped.len() - 1].to_string();
    }

    // 3. Attempt high‑precision datetime parse directly (chrono supports fractional seconds up to 9 digits)
    if stripped.contains(':') && stripped.contains('-') && stripped.contains(' ') {
        // Try parsing with space separator (e.g., "2025-06-18 21:05:00")
        // NaiveDateTime::from_str expects ISO 8601 with T, so we need to use parse_from_str
        let formats = [
            "%Y-%m-%d %H:%M:%S%.f", // With optional fractional seconds
            "%Y-%m-%d %H:%M:%S",    // Without fractional seconds
            "%Y-%m-%d %H:%M",       // Just hour and minute
        ];
        for fmt in &formats {
            if let Ok(dt) = NaiveDateTime::parse_from_str(&stripped, fmt) {
                return Some(Time::from_naive_datetime(dt));
            }
        }
    }

    // 4. Date (YYYY-MM-DD)
    if stripped.len() >= 8
        && stripped.matches('-').count() == 2
        && !stripped.contains(':')
        && stripped.parse::<NaiveDate>().is_ok()
    {
        return Time::new_date_from(&stripped).ok();
    }

    // 5. Year-month (YYYY-MM)
    if stripped.matches('-').count() == 1 && stripped.len() >= 6 && !stripped.contains(':') {
        // basic shape check: split and ensure month 1-12
        if let Some((y, m)) = stripped.split_once('-')
            && y.chars().all(|c| c == '-' || c.is_ascii_digit())
            && m.chars().all(|c| c.is_ascii_digit())
            && m.parse::<u8>()
                .ok()
                .filter(|mm| (1..=12).contains(mm))
                .is_some()
        {
            return Time::new_year_month_from(&stripped).ok();
        }
    }

    // 6. Year only
    if stripped.chars().all(|c| c == '-' || c.is_ascii_digit()) && (4..=8).contains(&stripped.len())
    {
        return Time::new_year_from(&stripped).ok();
    }

    None
}
fn parse_time_constant_with_now(value: &str, resolved_now: &Time) -> Option<Time> {
    match value.replace("@", "").as_str() {
        "NOW" => Some(resolved_now.clone()),
        "BOT" => Some(Time::new_beginning_of_time()),
        "EOT" => Some(Time::new_end_of_time()),
        _ => None,
    }
}

use pest::Parser;
use pest::error::ErrorVariant;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "traqula.pest"] // relative to src
struct TraqulaParser;

/// Return the number of top-level commands and searches in a validated script.
#[cfg(feature = "server")]
pub(crate) fn script_counts(traqula: &str) -> Result<(usize, usize), DatabaseError> {
    let pairs = TraqulaParser::parse(Rule::traqula, traqula.trim()).map_err(|error| {
        DatabaseError::Parse {
            message: error.to_string(),
            line: None,
            col: None,
        }
    })?;
    let mut commands = 0;
    let mut searches = 0;
    for pair in pairs {
        if pair.as_rule() != Rule::EOI {
            commands += 1;
        }
        if pair.as_rule() == Rule::search {
            searches += 1;
        }
    }
    Ok((commands, searches))
}

/// Execution engine binding a parsed Traqula script to a concrete database.
pub struct Engine<'en> {
    database: &'en Database,
}
/// Control flow returned by a sink after receiving a row.
pub enum SinkFlow {
    Continue,
    Stop,
}
/// Stable kind tag for a projected result cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCellKind {
    Thing,
    Literal,
    Time,
}

/// One lossless projected value shared by Rust, HTTP, SSE, and WASM.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct ResultCell {
    pub kind: ResultCellKind,
    pub text: String,
}

impl ResultCell {
    pub fn new(kind: ResultCellKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for ResultCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.text)
    }
}

impl PartialEq<String> for ResultCell {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

impl PartialEq<&str> for ResultCell {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}
/// Simple sink trait for capturing projected result rows. Returning Stop requests the engine to halt emission early.
pub trait RowSink {
    /// Called for each execution warning before rows are emitted.
    fn on_warning(&mut self, _warning: &ExecutionWarning) -> SinkFlow {
        SinkFlow::Continue
    }
    /// Called once when column names become available (return clause parsed) before any rows.
    /// Default is no-op. Returning Stop aborts the search early.
    fn on_meta(&mut self, _columns: &[String]) -> SinkFlow {
        SinkFlow::Continue
    }
    /// Called for each projected row.
    fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow;
}

/// Callback interface for multi-search streaming. Implementors receive framing events for each result set.
pub trait MultiStreamCallbacks {
    /// Called once for each script-level execution warning.
    fn on_warning(&mut self, _warning: &ExecutionWarning) {}
    /// Called once at the beginning of a result set with its index (0-based), column names, and raw search snippet.
    fn on_result_set_start(&mut self, set_index: usize, columns: &[String], search_text: &str);
    /// Called for every row. Return false to request early termination of this result set.
    fn on_row(&mut self, set_index: usize, row: Vec<ResultCell>) -> bool;
    /// Called when a result set finishes (naturally or via limit/early stop).
    fn on_result_set_end(&mut self, set_index: usize, row_count: usize, limited: bool);
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectedResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<ResultCell>>,
    pub row_count: usize,
    pub limited: bool,
    #[serde(skip)]
    pub metadata: ExecutionMetadata,
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectedResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<ResultCell>>,
    pub row_count: usize,
    pub limited: bool,
    pub search: Option<String>,
    #[serde(skip)]
    pub metadata: ExecutionMetadata,
}

/// Options that affect one complete Traqula script execution.
#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    /// Deterministic replacement for every `@NOW` occurrence in the script.
    pub now: Option<Time>,
    /// Cooperative wall-clock deadline measured from the start of execution.
    pub timeout: Option<Duration>,
    /// Token callers may use to request cooperative cancellation.
    pub cancellation: Option<CancellationToken>,
    /// Maximum rows emitted by each search. A lower Traqula `LIMIT` wins.
    pub max_rows_per_search: Option<usize>,
}

/// A clonable cooperative cancellation token for one script execution.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, AtomicOrdering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(AtomicOrdering::Acquire)
    }
}

/// A non-fatal diagnostic produced while executing a script.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExecutionWarning {
    pub code: String,
    pub message: String,
    pub rewrite: Option<String>,
}

/// Values resolved once and shared by every command in a script execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionMetadata {
    pub traqula_version: u16,
    pub resolved_now: Time,
    pub warnings: Vec<ExecutionWarning>,
}

struct ExecutionContext {
    metadata: ExecutionMetadata,
    deadline: Option<Instant>,
    cancellation: CancellationToken,
    max_rows_per_search: Option<usize>,
}

impl ExecutionOptions {
    fn resolve(self, source: &str) -> Result<ExecutionContext, DatabaseError> {
        let deadline = self
            .timeout
            .map(|timeout| {
                Instant::now().checked_add(timeout).ok_or_else(|| {
                    DatabaseError::Execution("execution timeout is too large".into())
                })
            })
            .transpose()?;
        Ok(ExecutionContext {
            metadata: ExecutionMetadata {
                traqula_version: TRAQULA_VERSION,
                resolved_now: self.now.unwrap_or_default(),
                warnings: execution_warnings(source),
            },
            deadline,
            cancellation: self.cancellation.unwrap_or_default(),
            max_rows_per_search: self.max_rows_per_search,
        })
    }
}

fn execution_warnings(source: &str) -> Vec<ExecutionWarning> {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut in_comment = false;
    let mut in_double_quote = false;
    let mut in_single_quote = false;
    let mut escaped = false;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();
        if in_comment {
            if byte == b'*' && next == Some(b'/') {
                in_comment = false;
                cursor += 2;
                continue;
            }
        } else if in_double_quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_double_quote = false;
            }
        } else if in_single_quote {
            if byte == b'\'' {
                in_single_quote = false;
            }
        } else if byte == b'/' && next == Some(b'*') {
            in_comment = true;
            cursor += 2;
            continue;
        } else if byte == b'"' {
            in_double_quote = true;
        } else if byte == b'\'' {
            in_single_quote = true;
        } else if byte == b'='
            && next == Some(b'=')
            && cursor
                .checked_sub(1)
                .and_then(|index| bytes.get(index))
                .copied()
                != Some(b'=')
            && bytes.get(cursor + 2).copied() != Some(b'=')
        {
            return vec![ExecutionWarning {
                code: "legacy-double-equals".to_string(),
                message: "legacy '==' means nominal equality and is deprecated".to_string(),
                rewrite: Some("replace '==' with '='".to_string()),
            }];
        }
        cursor += 1;
    }
    Vec::new()
}

impl ExecutionContext {
    fn check(&self) -> Result<(), DatabaseError> {
        if self.cancellation.is_cancelled() {
            return Err(DatabaseError::Cancelled);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(DatabaseError::Timeout);
        }
        Ok(())
    }

    fn effective_limit(&self, query_limit: Option<usize>) -> Option<usize> {
        match (query_limit, self.max_rows_per_search) {
            (Some(query), Some(maximum)) => Some(query.min(maximum)),
            (Some(query), None) => Some(query),
            (None, maximum) => maximum,
        }
    }
}
impl<'en> Engine<'en> {
    /// Create a new engine borrowing the provided database.
    pub fn new(database: &'en Database) -> Self {
        Self { database }
    }

    fn acquire_owner(
        &self,
        execution: &ExecutionContext,
    ) -> Result<MutexGuard<'_, ()>, DatabaseError> {
        loop {
            match self.database.execution_owner.try_lock() {
                Ok(owner) => return Ok(owner),
                Err(TryLockError::Poisoned(error)) => {
                    return Err(DatabaseError::Lock(error.to_string()));
                }
                Err(TryLockError::WouldBlock) => {
                    execution.check()?;
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Execute a single-search script in streaming fashion using the provided RowSink.
    /// Returns (columns, limited, row_count) or an error. If the script has zero or multiple search commands an error is returned.
    pub fn execute_stream_single<S: RowSink>(
        &self,
        traqula: &str,
        sink: &mut S,
    ) -> Result<(Vec<String>, bool, usize), crate::error::DatabaseError> {
        self.execute_stream_single_with_options(traqula, sink, ExecutionOptions::default())
    }

    /// Stream one search with cooperative timeout, cancellation, and row limits.
    pub fn execute_stream_single_with_options<S: RowSink>(
        &self,
        traqula: &str,
        sink: &mut S,
        options: ExecutionOptions,
    ) -> Result<(Vec<String>, bool, usize), crate::error::DatabaseError> {
        let execution = options.resolve(traqula)?;
        let _owner = self.acquire_owner(&execution)?;
        execution.check()?;
        for warning in &execution.metadata.warnings {
            if let SinkFlow::Stop = sink.on_warning(warning) {
                return Ok((Vec::new(), false, 0));
            }
        }
        let mut variables: Variables = Variables::default();
        let parse_result = TraqulaParser::parse(Rule::traqula, traqula.trim());
        let pairs = match parse_result {
            Ok(p) => p,
            Err(err) => {
                let mut msg = format!("{}", err);
                if let ErrorVariant::ParsingError {
                    positives,
                    negatives: _,
                } = err.variant
                    && !positives.is_empty()
                {
                    let mut expected: Vec<&'static str> =
                        positives.iter().map(|r| friendly_rule_name(*r)).collect();
                    expected.sort();
                    expected.dedup();
                    msg.push_str(&format!("\nExpected one of: {}", expected.join(", ")));
                }
                return Err(crate::error::DatabaseError::Parse {
                    message: msg,
                    line: None,
                    col: None,
                });
            }
        };
        let search_count = pairs
            .clone()
            .filter(|p| p.as_rule() == Rule::search)
            .count();
        if search_count != 1 {
            return Err(crate::error::DatabaseError::Execution(format!(
                "execute_stream_single expects exactly one search, found {}",
                search_count
            )));
        }
        let mut return_columns: Option<Vec<String>> = None; // will be populated when return clause processed
        let mut total_rows = 0usize;
        let mut limited = false;
        for command in pairs {
            execution.check()?;
            match command.as_rule() {
                Rule::add_role => self.add_role(command)?,
                Rule::add_posit => self.add_posit(
                    command,
                    &mut variables,
                    &execution.metadata.resolved_now,
                    &execution,
                )?,
                Rule::search => {
                    let mut search_variables: Variables = Variables::default();
                    // limit extraction
                    let mut limit = None;
                    let cloned = command.clone();
                    for c in cloned.into_inner() {
                        if c.as_rule() == Rule::limit_clause {
                            for p in c.into_inner() {
                                if let Ok(v) = p.as_str().parse::<usize>() {
                                    limit = Some(v);
                                }
                            }
                        }
                    }
                    let mut err = None;
                    struct CountingSink<'a, T: RowSink> {
                        inner: &'a mut T,
                        limit: Option<usize>,
                        count: usize,
                        limited: bool,
                    }
                    impl<'a, T: RowSink> RowSink for CountingSink<'a, T> {
                        fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
                            self.inner.on_meta(columns)
                        }
                        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
                            if let Some(l) = self.limit
                                && self.count >= l
                            {
                                self.limited = true;
                                return SinkFlow::Stop;
                            }
                            match self.inner.push(row) {
                                SinkFlow::Continue => {
                                    self.count += 1;
                                    SinkFlow::Continue
                                }
                                stop => stop,
                            }
                        }
                    }
                    let mut wrapper = CountingSink {
                        inner: sink,
                        limit: execution.effective_limit(limit),
                        count: 0,
                        limited: false,
                    };
                    self.search(
                        command,
                        &mut search_variables,
                        &mut wrapper,
                        &mut return_columns,
                        &mut err,
                        &execution,
                    );
                    if let Some(e) = err {
                        return Err(e);
                    }
                    total_rows = wrapper.count;
                    limited = wrapper.limited;
                }
                Rule::EOI => (),
                _ => (),
            }
        }
        Ok((return_columns.unwrap_or_default(), limited, total_rows))
    }
    /// Handle an `add role` command.
    fn add_role(&self, command: Pair<Rule>) -> Result<(), DatabaseError> {
        let names: Vec<String> = command
            .into_inner()
            .map(|role| role.as_str().trim().to_string())
            .collect();
        let kept = self
            .database
            .create_roles(names.iter().cloned().map(|name| (name, false)).collect())?;
        let mut added = 0usize;
        for (name, (_role, existed)) in names.iter().zip(kept) {
            if !existed {
                added += 1;
                info!(target: "positorium::traqula", event="add_role", role=name, "role added");
            } else {
                info!(target: "positorium::traqula", event="add_role", role=name, existed=true, "role already existed");
            }
        }
        if added > 0 {
            info!(target: "positorium::traqula", event="add_role_batch", added, "roles batch added");
        }
        Ok(())
    }
    /// Handle an `add posit` command producing one or more posits.
    fn add_posit(
        &self,
        command: Pair<Rule>,
        variables: &mut Variables,
        resolved_now: &Time,
        execution: &ExecutionContext,
    ) -> Result<(), DatabaseError> {
        let mut pending = Vec::new();
        let mut result_bindings = Vec::new();
        for structure in command.into_inner() {
            execution.check()?;
            let mut variable: Option<String> = None;
            let mut value: Option<LiteralValue> = None;
            let mut time: Option<Time> = None;
            let mut local_variables = Vec::new();
            let mut roles = Vec::new();
            match structure.as_rule() {
                Rule::posit => {
                    for component in structure.into_inner() {
                        match component.as_rule() {
                            Rule::insert => {
                                variable = Some(
                                    component.into_inner().next().unwrap().as_str().to_string(),
                                );
                                //println!("Insert: {:?}", &variable);
                            }
                            Rule::appearance_set => {
                                for member in component.into_inner() {
                                    for appearance in member.into_inner() {
                                        match appearance.as_rule() {
                                            Rule::insert => {
                                                let local_variable = appearance
                                                    .into_inner()
                                                    .next()
                                                    .unwrap()
                                                    .as_str();
                                                local_variables.push(local_variable);
                                                let thing = self
                                                    .database
                                                    .thing_generator()
                                                    .lock()
                                                    .unwrap()
                                                    .generate();
                                                match variables.entry(local_variable.to_string()) {
                                                    Entry::Vacant(entry) => {
                                                        let mut result_set = ResultSet::new();
                                                        result_set.insert(thing);
                                                        entry.insert(result_set);
                                                    }
                                                    Entry::Occupied(mut entry) => {
                                                        entry.get_mut().insert(thing);
                                                    }
                                                }
                                            }
                                            Rule::recall => {
                                                local_variables.push(
                                                    appearance
                                                        .into_inner()
                                                        .next()
                                                        .unwrap()
                                                        .as_str(),
                                                );
                                            }
                                            Rule::role => {
                                                roles.push(appearance.as_str());
                                            }
                                            _ => println!("Unknown appearance: {:?}", appearance),
                                        }
                                    }
                                }
                            }
                            Rule::appearing_value => {
                                for value_type in component.into_inner() {
                                    value = Some(parse_lossless_literal(
                                        value_type.as_rule(),
                                        value_type.as_str(),
                                        resolved_now,
                                    )?);
                                }
                            }
                            Rule::appearance_time => {
                                for time_type in component.into_inner() {
                                    match time_type.as_rule() {
                                        Rule::constant => {
                                            time = parse_time_constant_with_now(
                                                time_type.as_str(),
                                                resolved_now,
                                            );
                                            if time.is_none() {
                                                eprintln!(
                                                    "Failed to parse time constant: {}",
                                                    time_type.as_str()
                                                );
                                            }
                                        }
                                        Rule::time => {
                                            //println!("Time: {}", value_type.as_str());
                                            time = parse_time_with_now(
                                                time_type.as_str(),
                                                resolved_now,
                                            );
                                            if time.is_none() {
                                                eprintln!(
                                                    "Failed to parse time: {}",
                                                    time_type.as_str()
                                                );
                                            }
                                        }
                                        _ => println!("Unknown time type: {:?}", time_type),
                                    }
                                }
                            }
                            _ => println!("Unknown component: {:?}", component),
                        }
                    }
                    let mut variable_to_things = HashMap::new();
                    for local_variable in &local_variables {
                        variable_to_things.insert(*local_variable, Vec::new());
                    }
                    for local_variable in &local_variables {
                        let things = variable_to_things.get_mut(local_variable).unwrap();
                        let result_set = variables.get(*local_variable).unwrap();
                        match result_set.mode {
                            ResultSetMode::Empty => (),
                            ResultSetMode::Thing => {
                                things.push(result_set.thing.unwrap());
                            }
                            ResultSetMode::Multi => {
                                let multi = result_set.multi.as_ref().unwrap();
                                for thing in multi {
                                    things.push(thing);
                                }
                            }
                        }
                    }
                    let mut things_for_roles = Vec::new();
                    for local_variable in &local_variables {
                        let things_for_role = variable_to_things.get(local_variable).unwrap();
                        things_for_roles.push(things_for_role.as_slice());
                    }

                    // Reorder roles and their candidate lists by ascending cardinality to improve iteration locality
                    let mut order: Vec<usize> = (0..things_for_roles.len()).collect();
                    order.sort_by_key(|&i| things_for_roles[i].len());
                    let roles_ord: Vec<&str> = order.iter().map(|&i| roles[i]).collect();
                    let things_for_roles_ord: Vec<&[Thing]> =
                        order.iter().map(|&i| things_for_roles[i]).collect();

                    // Stream the Cartesian product (indices) to avoid allocating all combinations.
                    let mut appearance_sets = Vec::new();
                    let mut appearance_error = None;
                    for_each_cartesian_indices(things_for_roles_ord.as_slice(), |idxs| {
                        if let Err(error) = execution.check() {
                            appearance_error = Some(error);
                            return;
                        }
                        if appearance_error.is_some() {
                            return;
                        }
                        let mut appearances = Vec::new();
                        for i in 0..idxs.len() {
                            let role_keeper = self.database.role_keeper();
                            let role = match role_keeper
                                .lock()
                                .map_err(|error| DatabaseError::Lock(error.to_string()))
                                .and_then(|keeper| {
                                    keeper.get(roles_ord[i]).ok_or_else(|| {
                                        DatabaseError::UnknownRole(roles_ord[i].to_string())
                                    })
                                }) {
                                Ok(role) => role,
                                Err(error) => {
                                    appearance_error = Some(error);
                                    return;
                                }
                            };
                            let thing = things_for_roles_ord[i][idxs[i]];
                            match self.database.create_appearance(thing, Arc::clone(&role)) {
                                Ok((appearance, _)) => appearances.push(appearance),
                                Err(error) => {
                                    appearance_error = Some(error);
                                    return;
                                }
                            }
                        }
                        match self.database.create_appearance_set(appearances) {
                            Ok((appearance_set, _)) => appearance_sets.push(appearance_set),
                            Err(error) => appearance_error = Some(error),
                        }
                    });
                    if let Some(error) = appearance_error {
                        return Err(error);
                    }

                    // println!("Appearance sets {:?}", appearance_sets);

                    let time_value = time.ok_or_else(|| {
                        DatabaseError::Execution(
                            "add posit requires a valid appearance time".to_string(),
                        )
                    })?;
                    let value = value.ok_or_else(|| {
                        DatabaseError::Execution(
                            "add posit requires a valid appearing value".to_string(),
                        )
                    })?;

                    let start = pending.len();
                    for appearance_set in appearance_sets {
                        execution.check()?;
                        pending.push((appearance_set, value.clone(), time_value.clone()));
                    }
                    let end = pending.len();
                    if end > start {
                        info!(target: "positorium::traqula", event="add_posit_queued", count=end - start, roles=%roles_ord.join(","), value_family=?value.family(), "posits queued for command commit");
                    }
                    result_bindings.push((variable.take(), start..end));
                }
                _ => println!("Unknown structure: {:?}", structure),
            }
        }
        let kept = self.database.create_literal_posits(pending)?;
        for (variable, range) in result_bindings {
            if let Some(variable) = variable {
                match variables.entry(variable) {
                    Entry::Vacant(entry) => {
                        let mut result_set = ResultSet::new();
                        for posit in &kept[range.clone()] {
                            result_set.insert(posit.posit());
                        }
                        entry.insert(result_set);
                    }
                    Entry::Occupied(mut entry) => {
                        let result_set = entry.get_mut();
                        for posit in &kept[range] {
                            result_set.insert(posit.posit());
                        }
                    }
                }
            }
        }
        if !kept.is_empty() {
            info!(target: "positorium::traqula", event="add_posit_commit", count=kept.len(), "posit command committed");
        }
        Ok(())
    }
    fn search(
        &self,
        command: Pair<Rule>,
        variables: &mut Variables,
        sink: &mut dyn RowSink,
        return_columns: &mut Option<Vec<String>>,
        exec_error: &mut Option<crate::error::DatabaseError>,
        execution: &ExecutionContext,
    ) {
        let resolved_now = &execution.metadata.resolved_now;
        fn interrupted(
            execution: &ExecutionContext,
            exec_error: &mut Option<DatabaseError>,
        ) -> bool {
            match execution.check() {
                Ok(()) => false,
                Err(error) => {
                    if exec_error.is_none() {
                        *exec_error = Some(error);
                    }
                    true
                }
            }
        }

        if let Err(error) = execution.check() {
            *exec_error = Some(error);
            return;
        }
        fn cmp_ordering(ordering: std::cmp::Ordering, op: &str) -> bool {
            use std::cmp::Ordering::{Equal, Greater, Less};
            matches!(
                (ordering, op),
                (Less, "<" | "<=") | (Equal, "<=" | ">=" | "=" | "==") | (Greater, ">" | ">=")
            )
        }
        // Track variables referenced in this search command to guide projection
        let mut active_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Track candidate posits per bound time variable name (e.g., t, tw, birth_t)
        // time_var_candidates removed: time variable constraints are enforced during binding/projection without cloning candidate bitmaps per variable.
        // value_var_candidates removed (late pruning only during filtering stage)
        // Parsed where conditions on time variables: var -> (comparator, Time)
        let mut where_time: Vec<(String, String, Time)> = Vec::new();
        // Parsed where conditions between time variables: (var1, comparator, var2)
        let mut where_time_var: Vec<(String, String, String)> = Vec::new();
        // Parsed generic value conditions: (lhs_var, op, lossless literal)
        let mut where_value: Vec<(String, String, LiteralValue)> = Vec::new();
        let mut where_value_var: Vec<(String, String, String)> = Vec::new();
        fn parse_where_literal(raw: &str) -> Result<LiteralValue, DatabaseError> {
            let token = raw.trim();
            let family = if token.starts_with('"') {
                LiteralFamily::String
            } else if token.starts_with('{') {
                LiteralFamily::Json
            } else if token.ends_with('%') {
                LiteralFamily::Certainty
            } else if token.contains('.') {
                LiteralFamily::Decimal
            } else {
                LiteralFamily::Integer
            };
            LiteralValue::new(token, family).map_err(DatabaseError::Execution)
        }
        // Parsed variable-to-variable value comparisons (both non-time for now): (lhs, op, rhs)
        // (variable-to-variable value comparisons omitted in current implementation)
        // Track kinds of variables seen in this search (identity, value, time)
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum VarKind {
            Identity,
            Value,
            Time,
        }
        // A single binding row under construction during pattern expansion.
        // For now this is a scaffold; integration will replace current projection logic.
        #[derive(Debug, Clone)]
        #[allow(dead_code)]
        struct Binding {
            identities: HashMap<String, Thing>,
            posit_vars: HashMap<String, Thing>, // posit identity variables (e.g. p)
            value_slots: HashMap<String, (Thing /* posit id */, VarKind)>, // maps var -> (posit providing it, kind)
        }
        impl Binding {
            #[allow(dead_code)]
            fn new() -> Self {
                Binding {
                    identities: HashMap::new(),
                    posit_vars: HashMap::new(),
                    value_slots: HashMap::new(),
                }
            }
        }
        // All accumulated bindings (multiplicity preserving).
        let mut bindings: Vec<Binding> = Vec::new();
        #[allow(unused_mut)]
        let mut enumeration_started = false; // tracks if bindings vector has been seeded
        let mut variable_kinds: HashMap<String, VarKind> = HashMap::new();
        // Track whether any clause in this search failed (no candidates after constraints)
        let mut any_clause_failed: bool = false;
        // Track identity variables inserted (+var) in earlier patterns of this search (for recall readiness checks)
        let seen_inserted_id_vars: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // (LIMIT handled externally by a wrapping sink)
        for clause in command.into_inner() {
            if let Err(error) = execution.check() {
                *exec_error = Some(error);
                return;
            }
            match clause.as_rule() {
                Rule::search_clause => {
                    let mut pattern_index = 0usize; // diagnostic counter
                    for structure in clause.into_inner() {
                        if let Err(error) = execution.check() {
                            *exec_error = Some(error);
                            return;
                        }
                        let mut variable: Option<String> = None;
                        let mut _posits: Vec<Thing> = Vec::new();
                        let mut _value_as_literal: Option<LiteralValue> = None;
                        let mut _value_as_variable: Option<&str> = None;
                        let mut _value_is_wildcard = false;
                        let mut _time: Option<Time> = None;
                        let mut _time_as_variable: Option<&str> = None;
                        let mut _time_is_wildcard = false;
                        let mut local_variables = Vec::new();
                        // Track unions like (w|h, name) => ["w","h"]. Parallel to local_variables by index; None for non-union
                        let mut local_variable_unions: Vec<Option<Vec<String>>> = Vec::new();
                        let mut roles = Vec::new();
                        match structure.as_rule() {
                            Rule::posit_search => {
                                let diag_pattern_id = pattern_index;
                                pattern_index += 1; // increment early for logging
                                // Track optional per-clause 'as of' time
                                let mut _as_of_time: Option<Time> = None;
                                let mut _as_of_var: Option<String> = None;
                                // Track identity variables inserted in this pattern (to expose recall-ready after the pattern completes)
                                let mut inserted_id_vars_this_pattern: Vec<String> = Vec::new();
                                for component in structure.into_inner() {
                                    match component.as_rule() {
                                        Rule::insert => {
                                            variable = Some(
                                                component
                                                    .into_inner()
                                                    .next()
                                                    .unwrap()
                                                    .as_str()
                                                    .to_string(),
                                            );
                                            if let Some(v) = &variable {
                                                active_vars
                                                    .insert(v.trim_start_matches('+').to_string());
                                                // Treat posit variables as identity-like so they can be returned
                                                variable_kinds.insert(
                                                    v.trim_start_matches('+').to_string(),
                                                    VarKind::Identity,
                                                );
                                            }
                                            //println!("Insert: {}", &variable.as_ref().unwrap());
                                        }
                                        Rule::appearance_set_search => {
                                            // println!("Appearance set search: {}", component.as_str());
                                            for member in component.into_inner() {
                                                for appearance in member.into_inner() {
                                                    match appearance.as_rule() {
                                                        Rule::insert => {
                                                            let local_variable = appearance
                                                                .into_inner()
                                                                .next()
                                                                .unwrap()
                                                                .as_str();
                                                            local_variables.push(local_variable);
                                                            local_variable_unions.push(None);
                                                            inserted_id_vars_this_pattern
                                                                .push(local_variable.to_string());
                                                            if let Entry::Vacant(entry) = variables
                                                                .entry(local_variable.to_string())
                                                            {
                                                                let result_set = ResultSet::new();
                                                                entry.insert(result_set);
                                                            }
                                                            active_vars
                                                                .insert(local_variable.to_string());
                                                            variable_kinds.insert(
                                                                local_variable.to_string(),
                                                                VarKind::Identity,
                                                            );
                                                        }
                                                        Rule::wildcard => {
                                                            local_variables
                                                                .push(appearance.as_str());
                                                            local_variable_unions.push(None);
                                                            //println!("wildcard");
                                                        }
                                                        Rule::recall => {
                                                            local_variables.push(
                                                                appearance
                                                                    .into_inner()
                                                                    .next()
                                                                    .unwrap()
                                                                    .as_str(),
                                                            );
                                                            local_variable_unions.push(None);
                                                            if let Some(v) = local_variables.last()
                                                            {
                                                                active_vars
                                                                    .insert((*v).to_string());
                                                            }
                                                            if let Some(v) = local_variables.last()
                                                            {
                                                                variable_kinds.insert(
                                                                    (*v).to_string(),
                                                                    VarKind::Identity,
                                                                );
                                                            }
                                                        }
                                                        Rule::recall_union => {
                                                            // Collect all recall names separated by '|'
                                                            let mut names: Vec<String> = Vec::new();
                                                            for part in appearance.into_inner() {
                                                                // parts alternate: recall, '|', recall, '|', ... but pest grouped only recalls due to rule
                                                                if part.as_rule() == Rule::recall {
                                                                    names.push(
                                                                        part.into_inner()
                                                                            .next()
                                                                            .unwrap()
                                                                            .as_str()
                                                                            .to_string(),
                                                                    );
                                                                }
                                                            }
                                                            // Store a synthetic token representing the union; we use "w|h" literal for variable token, but keep union list separately
                                                            let token = names.join("|");
                                                            local_variables.push(Box::leak(
                                                                token.into_boxed_str(),
                                                            ));
                                                            local_variable_unions
                                                                .push(Some(names.clone()));
                                                            for n in names {
                                                                variable_kinds.insert(
                                                                    n.clone(),
                                                                    VarKind::Identity,
                                                                );
                                                                active_vars.insert(n);
                                                            }
                                                        }
                                                        Rule::role => {
                                                            roles.push(appearance.as_str());
                                                        }
                                                        _ => println!(
                                                            "Unknown appearance: {:?}",
                                                            appearance
                                                        ),
                                                    }
                                                }
                                            }
                                        }
                                        Rule::appearing_value_search => {
                                            // println!("Appearing value search: {}", component.as_str());
                                            for value_type in component.into_inner() {
                                                match value_type.as_rule() {
                                                    Rule::insert | Rule::recall => {
                                                        let local_variable = value_type
                                                            .into_inner()
                                                            .next()
                                                            .unwrap()
                                                            .as_str();
                                                        _value_as_variable = Some(local_variable);
                                                        active_vars
                                                            .insert(local_variable.to_string());
                                                        variable_kinds.insert(
                                                            local_variable.to_string(),
                                                            VarKind::Value,
                                                        );
                                                    }
                                                    Rule::wildcard => {
                                                        _value_is_wildcard = true;
                                                        //println!("wildcard");
                                                    }
                                                    Rule::constant
                                                    | Rule::json
                                                    | Rule::string
                                                    | Rule::time
                                                    | Rule::certainty
                                                    | Rule::decimal
                                                    | Rule::int => {
                                                        match parse_lossless_literal(
                                                            value_type.as_rule(),
                                                            value_type.as_str(),
                                                            resolved_now,
                                                        ) {
                                                            Ok(value) => {
                                                                _value_as_literal = Some(value)
                                                            }
                                                            Err(error) => {
                                                                *exec_error = Some(error);
                                                                return;
                                                            }
                                                        }
                                                    }
                                                    _ => println!(
                                                        "Unknown value type: {:?}",
                                                        value_type
                                                    ),
                                                }
                                            }
                                        }
                                        Rule::appearance_time_search => {
                                            // println!("Appearing time search: {}", component.as_str());
                                            for time_type in component.into_inner() {
                                                match time_type.as_rule() {
                                                    Rule::insert | Rule::recall => {
                                                        let local_variable = time_type
                                                            .into_inner()
                                                            .next()
                                                            .unwrap()
                                                            .as_str();
                                                        _time_as_variable = Some(local_variable);
                                                        active_vars
                                                            .insert(local_variable.to_string());
                                                        variable_kinds.insert(
                                                            local_variable.to_string(),
                                                            VarKind::Time,
                                                        );
                                                    }
                                                    Rule::wildcard => {
                                                        _time_is_wildcard = true;
                                                        //println!("wildcard");
                                                    }
                                                    Rule::constant => {
                                                        _time = parse_time_constant_with_now(
                                                            time_type.as_str(),
                                                            resolved_now,
                                                        );
                                                    }
                                                    Rule::time => {
                                                        //println!("Time: {}", value_type.as_str());
                                                        _time = parse_time_with_now(
                                                            time_type.as_str(),
                                                            resolved_now,
                                                        );
                                                    }
                                                    _ => println!(
                                                        "Unknown time type: {:?}",
                                                        time_type
                                                    ),
                                                }
                                            }
                                        }
                                        Rule::as_of_clause => {
                                            // Parse: as of <constant|time|recall>
                                            for part in component.into_inner() {
                                                match part.as_rule() {
                                                    Rule::constant => {
                                                        _as_of_time = parse_time_constant_with_now(
                                                            part.as_str(),
                                                            resolved_now,
                                                        );
                                                    }
                                                    Rule::time => {
                                                        _as_of_time = parse_time_with_now(
                                                            part.as_str(),
                                                            resolved_now,
                                                        );
                                                    }
                                                    Rule::recall => {
                                                        // Variable as_of: record the variable name for per-binding snapshot reduction.
                                                        _as_of_var =
                                                            Some(part.as_str().to_string());
                                                        // Preserve the legacy predicate as a safety net when a time var exists in this pattern.
                                                        if let Some(time_var) = _time_as_variable {
                                                            where_time_var.push((
                                                                time_var.to_string(),
                                                                "<=".to_string(),
                                                                part.as_str().to_string(),
                                                            ));
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        _ => println!("Unknown component: {:?}", component),
                                    }
                                }
                                // Minimal evaluation: compute candidates by role intersection and bind variables
                                if !roles.is_empty() {
                                    info!(target:"positorium::stream", event="pattern_start", pattern_index=diag_pattern_id, roles=?roles, local_vars=?local_variables, unions=?local_variable_unions.iter().map(|u| u.as_ref().map(|v| v.join("|"))).collect::<Vec<_>>(), as_of_literal=%_as_of_time.as_ref().map(|t|format!("{}",t)).unwrap_or_default());
                                    // Intersect role bitmaps starting from the smallest posting list
                                    let rk = self.database.role_keeper();
                                    let rk_guard = rk.lock().unwrap();
                                    let mut role_things = Vec::with_capacity(roles.len());
                                    for role_name in &roles {
                                        if interrupted(execution, exec_error) {
                                            return;
                                        }
                                        let Some(role) = rk_guard.get(role_name) else {
                                            *exec_error = Some(DatabaseError::UnknownRole(
                                                role_name.to_string(),
                                            ));
                                            return;
                                        };
                                        role_things.push(role.role());
                                    }
                                    let lk = self.database.role_to_posit_thing_lookup();
                                    let lk_guard = lk.lock().unwrap();
                                    let mut order: Vec<usize> = (0..role_things.len()).collect();
                                    order.sort_unstable_by_key(|i| {
                                        lk_guard
                                            .lookup(&role_things[*i])
                                            .map_or(0, |posting| posting.len())
                                    });
                                    let mut candidates: Option<RoaringTreemap> = None;
                                    for (pos, idx) in order.into_iter().enumerate() {
                                        if interrupted(execution, exec_error) {
                                            return;
                                        }
                                        let Some(bm_ref) = lk_guard.lookup(&role_things[idx])
                                        else {
                                            candidates = Some(RoaringTreemap::new());
                                            break;
                                        };
                                        if pos == 0 {
                                            if bm_ref.is_empty() {
                                                candidates = Some(RoaringTreemap::new());
                                                break;
                                            }
                                            candidates = Some(bm_ref.clone());
                                        } else if let Some(ref mut acc) = candidates {
                                            if acc.is_empty() {
                                                break;
                                            }
                                            *acc &= bm_ref;
                                        }
                                    }
                                    if let Some(cands_initial) = candidates {
                                        info!(target:"positorium::stream", event="candidates_initial", count=cands_initial.len(), roles=?roles, time_literal=%_time.as_ref().map(|t|format!("{}",t)).unwrap_or_default(), as_of_literal=%_as_of_time.as_ref().map(|t|format!("{}",t)).unwrap_or_default());
                                        // Optional time filter for any role when a literal/constant time is provided
                                        let mut cands = cands_initial;
                                        if let Some(ref t) = _time {
                                            let mut filtered = RoaringTreemap::new();
                                            let tk = self.database.posit_time_lookup();
                                            let guard = tk.lock().unwrap();
                                            for id in cands.iter() {
                                                if interrupted(execution, exec_error) {
                                                    return;
                                                }
                                                if let Some(pt) = guard.get(&id)
                                                    && pt == t
                                                {
                                                    filtered.insert(id);
                                                }
                                            }
                                            cands = filtered;
                                            info!(target:"positorium::stream", event="time_filter", remaining=cands.len());
                                            if cands.is_empty() {
                                                any_clause_failed = true;
                                            }
                                        }
                                        // Optional per-clause 'as of' reduction: keep latest time <= as_of for each appearance set
                                        if let Some(ref as_of) = _as_of_time
                                            && !cands.is_empty()
                                        {
                                            let time_lk = self.database.posit_time_lookup();
                                            let time_guard = time_lk.lock().unwrap();
                                            let aset_lk = self
                                                .database
                                                .posit_thing_to_appearance_set_lookup();
                                            let aset_guard = aset_lk.lock().unwrap();
                                            // Keep every maximal applicable posit per appearance set.
                                            use std::collections::HashMap as StdHashMap;
                                            let mut best: StdHashMap<usize, Vec<(Time, Thing)>> =
                                                StdHashMap::new();
                                            for pid in cands.iter() {
                                                if interrupted(execution, exec_error) {
                                                    return;
                                                }
                                                if let Some(pt) = time_guard.get(&pid)
                                                    && pt.definitely_at_or_before(as_of)
                                                    && let Some(aset) = aset_guard.get(&pid)
                                                {
                                                    let key = Arc::as_ptr(aset) as usize;
                                                    let maxima = best.entry(key).or_default();
                                                    if maxima.iter().any(|(existing, _)| {
                                                        existing.definitely_after(pt)
                                                    }) {
                                                        continue;
                                                    }
                                                    maxima.retain(|(existing, _)| {
                                                        !pt.definitely_after(existing)
                                                    });
                                                    maxima.push((pt.clone(), pid));
                                                }
                                            }
                                            let mut reduced = RoaringTreemap::new();
                                            for maxima in best.into_values() {
                                                for (_, id) in maxima {
                                                    reduced.insert(id);
                                                }
                                            }
                                            cands = reduced;
                                            info!(target:"positorium::stream", event="snapshot_reduced", remaining=cands.len(), as_of=%format!("{}", as_of));
                                            if cands.is_empty() {
                                                any_clause_failed = true;
                                            }
                                        }
                                        // Optional value filter for any role when a literal/constant value is provided
                                        if let Some(expected) = _value_as_literal.as_ref() {
                                            let mut filtered = RoaringTreemap::new();
                                            let pk = self.database.posit_keeper();
                                            let mut pk_guard = pk.lock().unwrap();
                                            for id in cands.iter() {
                                                if interrupted(execution, exec_error) {
                                                    return;
                                                }
                                                if let Some(p) = pk_guard.posit::<LiteralValue>(id)
                                                {
                                                    match p.value().nominally_equals(expected) {
                                                        Ok(true) => {
                                                            filtered.insert(id);
                                                        }
                                                        Ok(false) => {}
                                                        Err(error) => {
                                                            *exec_error = Some(
                                                                DatabaseError::Execution(error),
                                                            );
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                            cands = filtered;
                                            info!(target:"positorium::stream", event="value_filter", remaining=cands.len());
                                            if cands.is_empty() {
                                                any_clause_failed = true;
                                            }
                                        }
                                        // (as-of moved to after local identity constraints)
                                        // Pre-check: error on unbound recall identity variables (non-union). Unions are optional filters.
                                        if !local_variables.is_empty() {
                                            for (i, token) in local_variables.iter().enumerate() {
                                                if *token == "*" {
                                                    continue;
                                                }
                                                if local_variable_unions
                                                    .get(i)
                                                    .and_then(|u| u.as_ref())
                                                    .is_some()
                                                {
                                                    continue;
                                                }
                                                let is_insert = token.starts_with('+');
                                                if !is_insert {
                                                    let key = *token; // recall name as-is
                                                    if !(variables.contains_key(key)
                                                        || seen_inserted_id_vars.contains(key))
                                                    {
                                                        *exec_error = Some(
                                                            DatabaseError::Execution(format!(
                                                                "Variable '{}' is used as a recall but is not bound in a prior command",
                                                                key
                                                            )),
                                                        );
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                        // Apply local identity variable constraints to filter candidates (e.g., (w, name) restricts to bound wife)
                                        if !local_variables.is_empty() && !cands.is_empty() {
                                            let lk = self
                                                .database
                                                .posit_thing_to_appearance_set_lookup();
                                            let aset_guard = lk.lock().unwrap();
                                            let mut filtered = RoaringTreemap::new();
                                            'cand: for id in cands.iter() {
                                                if interrupted(execution, exec_error) {
                                                    return;
                                                }
                                                let appset = match aset_guard.get(&id) {
                                                    Some(aset) => aset,
                                                    None => continue,
                                                };
                                                for (i, token) in local_variables.iter().enumerate()
                                                {
                                                    if *token == "*" {
                                                        continue;
                                                    }
                                                    let role_name = roles[i];
                                                    let bound_opt = appset
                                                        .appearances()
                                                        .iter()
                                                        .find(|a| a.role().name() == role_name)
                                                        .map(|a| a.thing());
                                                    if let Some(bound_id) = bound_opt {
                                                        // Determine if this bound_id satisfies existing variable bindings (support unions)
                                                        let union_names = local_variable_unions
                                                            .get(i)
                                                            .and_then(|u| u.as_ref());
                                                        let satisfies = if let Some(names) =
                                                            union_names
                                                        {
                                                            // If any union member is already bound (non-empty) and contains bound_id, accept;
                                                            // if none are effectively bound (all empty or absent), don't restrict.
                                                            let mut any_bound = false;
                                                            let mut any_match = false;
                                                            for name in names.iter() {
                                                                if let Some(rs) =
                                                                    variables.get(name)
                                                                {
                                                                    match rs.mode {
                                                                        ResultSetMode::Thing => {
                                                                            any_bound = true;
                                                                            any_match |=
                                                                                rs.thing.unwrap()
                                                                                    == bound_id;
                                                                        }
                                                                        ResultSetMode::Multi => {
                                                                            any_bound = true;
                                                                            any_match |= rs
                                                                                .multi
                                                                                .as_ref()
                                                                                .unwrap()
                                                                                .contains(bound_id);
                                                                        }
                                                                        ResultSetMode::Empty => {
                                                                            // Treat empty as unbound
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            if any_bound { any_match } else { true }
                                                        } else {
                                                            let key = token
                                                                .strip_prefix('+')
                                                                .unwrap_or(token);
                                                            if let Some(rs) = variables.get(key) {
                                                                match rs.mode {
                                                                    ResultSetMode::Thing => {
                                                                        rs.thing.unwrap()
                                                                            == bound_id
                                                                    }
                                                                    ResultSetMode::Multi => rs
                                                                        .multi
                                                                        .as_ref()
                                                                        .unwrap()
                                                                        .contains(bound_id),
                                                                    ResultSetMode::Empty => true, // treat empty as unbound
                                                                }
                                                            } else {
                                                                // Unbound variable – don't restrict
                                                                true
                                                            }
                                                        };
                                                        if !satisfies {
                                                            continue 'cand;
                                                        }
                                                    } else {
                                                        continue 'cand;
                                                    }
                                                }
                                                filtered.insert(id);
                                            }
                                            cands = filtered;
                                            if cands.is_empty() {
                                                any_clause_failed = true;
                                            }
                                        }
                                        info!(target:"positorium::stream", event="pattern_end", pattern_index=diag_pattern_id, final_candidates=cands.len(), any_clause_failed=any_clause_failed, enumeration_started=enumeration_started);
                                        // Remember candidate posits for projection when returning values/times
                                        // (legacy single-role candidate capture removed)
                                        // If the appearing value used a variable (e.g., +n or n), capture its candidates
                                        if let Some(vname) = _value_as_variable {
                                            active_vars.insert(vname.to_string());
                                        }
                                        // If the time slot used a variable, just mark it active; per-binding time resolution happens later
                                        if let Some(varname) = _time_as_variable {
                                            active_vars.insert(varname.to_string());
                                        }
                                        // Bind outer posit variable (e.g., +p)
                                        if let Some(var) = &variable {
                                            let name = var.strip_prefix('+').unwrap_or(var);
                                            match variables.entry(name.to_string()) {
                                                Entry::Vacant(entry) => {
                                                    let mut rs = ResultSet::new();
                                                    rs.insert_many(&cands);
                                                    entry.insert(rs);
                                                }
                                                Entry::Occupied(mut entry) => {
                                                    let rs = entry.get_mut();
                                                    rs.insert_many(&cands);
                                                }
                                            }
                                        }
                                        // Do not bind identity variables into the global `variables` map here.
                                        // Identity compatibility between patterns is enforced during binding enumeration/merge below.
                                        // No role-specific special-case filters; recall variables act as join filters generically.
                                        // ---------------- Binding enumeration (multiplicity preservation) ----------------
                                        if !cands.is_empty() {
                                            // Collect identity variable assignments for each candidate posit
                                            let aset_lookup = self
                                                .database
                                                .posit_thing_to_appearance_set_lookup();
                                            let aset_guard = aset_lookup.lock().unwrap();
                                            let mut candidate_info: Vec<(
                                                Thing,
                                                HashMap<String, Thing>,
                                            )> = Vec::new();
                                            for pid in cands.iter() {
                                                if interrupted(execution, exec_error) {
                                                    return;
                                                }
                                                if let Some(appset) = aset_guard.get(&pid) {
                                                    // First collect per-variable identity maps; union variables may yield multiple maps (one per union member)
                                                    let mut pending_maps: Vec<
                                                        HashMap<String, Thing>,
                                                    > = vec![HashMap::new()];
                                                    for (i, token) in
                                                        local_variables.iter().enumerate()
                                                    {
                                                        if *token == "*" {
                                                            continue;
                                                        }
                                                        let role_name = roles[i];
                                                        if let Some(thing) = appset
                                                            .appearances()
                                                            .iter()
                                                            .find(|a| a.role().name() == role_name)
                                                            .map(|a| a.thing())
                                                        {
                                                            if let Some(Some(union_names)) =
                                                                local_variable_unions.get(i)
                                                            {
                                                                // For unions, branch for each member; identity compatibility will filter later.
                                                                let mut branched: Vec<
                                                                    HashMap<String, Thing>,
                                                                > = Vec::with_capacity(
                                                                    pending_maps.len()
                                                                        * union_names.len(),
                                                                );
                                                                for uname in union_names.iter() {
                                                                    for existing in
                                                                        pending_maps.iter()
                                                                    {
                                                                        let mut cloned =
                                                                            existing.clone();
                                                                        cloned.insert(
                                                                            uname.clone(),
                                                                            thing,
                                                                        );
                                                                        branched.push(cloned);
                                                                    }
                                                                }
                                                                pending_maps = branched;
                                                            } else {
                                                                if token.contains('|') {
                                                                    continue;
                                                                }
                                                                let vname = token
                                                                    .strip_prefix('+')
                                                                    .unwrap_or(token)
                                                                    .to_string();
                                                                for m in pending_maps.iter_mut() {
                                                                    m.insert(vname.clone(), thing);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    for id_map in pending_maps.into_iter() {
                                                        candidate_info.push((pid, id_map));
                                                    }
                                                    info!(target:"positorium::stream", event="enumeration_candidate_info", entries=candidate_info.len(), enumeration_started=?enumeration_started);
                                                }
                                            }
                                            // Names for value/time/posit variables (strip plus)
                                            let posit_var_name = variable.as_ref().map(|v| {
                                                v.strip_prefix('+').unwrap_or(v).to_string()
                                            });
                                            let value_var_name = _value_as_variable.map(|v| {
                                                v.strip_prefix('+').unwrap_or(v).to_string()
                                            });
                                            let time_var_name = _time_as_variable.map(|v| {
                                                v.strip_prefix('+').unwrap_or(v).to_string()
                                            });
                                            if !enumeration_started {
                                                for (pid, id_map) in candidate_info.iter() {
                                                    if interrupted(execution, exec_error) {
                                                        return;
                                                    }
                                                    let mut b = Binding::new();
                                                    b.identities.extend(
                                                        id_map.iter().map(|(k, v)| (k.clone(), *v)),
                                                    );
                                                    if let Some(ref pn) = posit_var_name {
                                                        b.posit_vars.insert(pn.clone(), *pid);
                                                    }
                                                    if let Some(ref vn) = value_var_name {
                                                        b.value_slots.insert(
                                                            vn.clone(),
                                                            (*pid, VarKind::Value),
                                                        );
                                                    }
                                                    if let Some(ref tn) = time_var_name {
                                                        b.value_slots.insert(
                                                            tn.clone(),
                                                            (*pid, VarKind::Time),
                                                        );
                                                    }
                                                    bindings.push(b);
                                                }
                                                enumeration_started = true;
                                                info!(target:"positorium::stream", event="enumeration_seed", seeded_bindings=bindings.len());
                                            } else {
                                                let mut new_bindings: Vec<Binding> = Vec::new();
                                                // Guards and caches for per-binding snapshot reduction when using variable as-of
                                                // Acquire time lookup guard; re-use existing aset_guard from candidate enumeration to avoid re-lock deadlock
                                                let time_lk = self.database.posit_time_lookup();
                                                let time_guard = time_lk.lock().unwrap();
                                                for existing in bindings.iter() {
                                                    if interrupted(execution, exec_error) {
                                                        return;
                                                    }
                                                    // Optional: resolve as-of time for this binding
                                                    let binding_as_of_time: Option<Time> =
                                                        if let Some(ref asv) = _as_of_var {
                                                            if let Some((pid_t, VarKind::Time)) =
                                                                existing.value_slots.get(asv)
                                                            {
                                                                time_guard.get(pid_t).cloned()
                                                            } else {
                                                                None
                                                            }
                                                        } else {
                                                            None
                                                        };
                                                    // Precompute best-per-appearance-set once per binding when variable as-of is in effect
                                                    let best_cache: Option<
                                                        std::collections::HashMap<
                                                            usize,
                                                            Vec<(Time, Thing)>,
                                                        >,
                                                    > = if let Some(ref as_of_time) =
                                                        binding_as_of_time
                                                    {
                                                        let mut cache: std::collections::HashMap<
                                                            usize,
                                                            Vec<(Time, Thing)>,
                                                        > = std::collections::HashMap::new();
                                                        for cid in cands.iter() {
                                                            if interrupted(execution, exec_error) {
                                                                return;
                                                            }
                                                            if let Some(aset_c) =
                                                                aset_guard.get(&cid)
                                                                && let Some(ct) =
                                                                    time_guard.get(&cid)
                                                                && ct.definitely_at_or_before(
                                                                    as_of_time,
                                                                )
                                                            {
                                                                let key =
                                                                    Arc::as_ptr(aset_c) as usize;
                                                                let maxima =
                                                                    cache.entry(key).or_default();
                                                                if maxima.iter().any(
                                                                    |(existing, _)| {
                                                                        existing
                                                                            .definitely_after(ct)
                                                                    },
                                                                ) {
                                                                    continue;
                                                                }
                                                                maxima.retain(|(existing, _)| {
                                                                    !ct.definitely_after(existing)
                                                                });
                                                                maxima.push((ct.clone(), cid));
                                                            }
                                                        }
                                                        info!(target:"positorium::stream", event="snapshot_reduced_per_binding", aset_count=cache.len());
                                                        Some(cache)
                                                    } else {
                                                        None
                                                    };
                                                    for (pid, id_map) in candidate_info.iter() {
                                                        if interrupted(execution, exec_error) {
                                                            return;
                                                        }
                                                        // Identity compatibility
                                                        let mut ok = true;
                                                        for (k, v) in id_map.iter() {
                                                            if let Some(prev) =
                                                                existing.identities.get(k)
                                                                && prev != v
                                                            {
                                                                ok = false;
                                                                break;
                                                            }
                                                        }
                                                        if !ok {
                                                            continue;
                                                        }
                                                        // If variable as-of is in effect and bound for this row, enforce snapshot: pid must be the latest <= as_of among cands for its appearance set
                                                        if let (Some(_), Some(cache)) =
                                                            (&binding_as_of_time, &best_cache)
                                                            && let Some(aset_of_pid) =
                                                                aset_guard.get(pid)
                                                        {
                                                            let key =
                                                                Arc::as_ptr(aset_of_pid) as usize;
                                                            if let Some(maxima) = cache.get(&key) {
                                                                if !maxima.iter().any(
                                                                    |(_, best_pid)| pid == best_pid,
                                                                ) {
                                                                    continue;
                                                                }
                                                            } else {
                                                                continue;
                                                            }
                                                        }
                                                        // Posit variable compatibility
                                                        if let Some(ref pn) = posit_var_name
                                                            && let Some(prev) =
                                                                existing.posit_vars.get(pn)
                                                            && prev != pid
                                                        {
                                                            continue;
                                                        }
                                                        // Value variable compatibility
                                                        if let Some(ref vn) = value_var_name
                                                            && let Some((prev_pid, _)) =
                                                                existing.value_slots.get(vn)
                                                            && prev_pid != pid
                                                        {
                                                            continue;
                                                        }
                                                        // Time variable compatibility
                                                        if let Some(ref tn) = time_var_name
                                                            && let Some((prev_pid, _)) =
                                                                existing.value_slots.get(tn)
                                                            && prev_pid != pid
                                                        {
                                                            continue;
                                                        }
                                                        // Merge
                                                        let mut merged = existing.clone();
                                                        for (k, v) in id_map.iter() {
                                                            merged
                                                                .identities
                                                                .entry(k.clone())
                                                                .or_insert(*v);
                                                        }
                                                        if let Some(ref pn) = posit_var_name {
                                                            merged
                                                                .posit_vars
                                                                .entry(pn.clone())
                                                                .or_insert(*pid);
                                                        }
                                                        if let Some(ref vn) = value_var_name {
                                                            merged
                                                                .value_slots
                                                                .entry(vn.clone())
                                                                .or_insert((*pid, VarKind::Value));
                                                        }
                                                        if let Some(ref tn) = time_var_name {
                                                            merged
                                                                .value_slots
                                                                .entry(tn.clone())
                                                                .or_insert((*pid, VarKind::Time));
                                                        }
                                                        new_bindings.push(merged);
                                                    }
                                                }
                                                bindings = new_bindings;
                                                if bindings.is_empty() {
                                                    any_clause_failed = true;
                                                }
                                                info!(target:"positorium::stream", event="enumeration_merge", merged_bindings=bindings.len(), any_clause_failed=any_clause_failed);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => println!("Unknown posit structure: {:?}", structure),
                        }
                        // local variable debug output suppressed
                    }
                }
                Rule::where_clause => {
                    // Extended parser: collect time comparisons (existing behavior) and stash generic ones for future evaluation.
                    // Unsupported (non-time) conditions are currently parsed but not evaluated: we log once if encountered.
                    // (Previously logged unsupported conditions; now we capture generics silently.)
                    for part in clause.into_inner() {
                        if part.as_rule() == Rule::condition {
                            let mut lhs_var: Option<String> = None;
                            let mut op: Option<String> = None;
                            let mut rhs_time: Option<Time> = None;
                            let mut rhs_is_time = false;
                            let mut rhs_raw: Option<String> = None; // generic string form
                            let mut rhs_var: Option<String> = None;
                            for c in part.into_inner() {
                                match c.as_rule() {
                                    Rule::recall => {
                                        if lhs_var.is_none() {
                                            lhs_var = Some(
                                                c.into_inner().next().unwrap().as_str().to_string(),
                                            );
                                        } else if rhs_var.is_none() {
                                            rhs_var = Some(
                                                c.into_inner().next().unwrap().as_str().to_string(),
                                            );
                                        }
                                    }
                                    Rule::comparator => op = Some(c.as_str().to_string()),
                                    Rule::constant => {
                                        // Could be time constant
                                        if let Some(t) =
                                            parse_time_constant_with_now(c.as_str(), resolved_now)
                                        {
                                            rhs_time = Some(t);
                                            rhs_is_time = true;
                                        }
                                        rhs_raw = Some(c.as_str().to_string());
                                    }
                                    Rule::time => {
                                        rhs_time = parse_time_with_now(c.as_str(), resolved_now);
                                        rhs_is_time = true;
                                        rhs_raw = Some(c.as_str().to_string());
                                    }
                                    // literals
                                    Rule::certainty
                                    | Rule::decimal
                                    | Rule::int
                                    | Rule::string
                                    | Rule::json => {
                                        rhs_raw = Some(c.as_str().to_string());
                                    }
                                    Rule::rhs_value => {
                                        // unwrap one level
                                        for r in c.into_inner() {
                                            match r.as_rule() {
                                                Rule::constant => {
                                                    if let Some(t) = parse_time_constant_with_now(
                                                        r.as_str(),
                                                        resolved_now,
                                                    ) {
                                                        rhs_time = Some(t);
                                                        rhs_is_time = true;
                                                    }
                                                    rhs_raw = Some(r.as_str().to_string());
                                                }
                                                Rule::time => {
                                                    rhs_time = parse_time_with_now(
                                                        r.as_str(),
                                                        resolved_now,
                                                    );
                                                    rhs_is_time = true;
                                                    rhs_raw = Some(r.as_str().to_string());
                                                }
                                                Rule::certainty
                                                | Rule::decimal
                                                | Rule::int
                                                | Rule::string
                                                | Rule::json => {
                                                    rhs_raw = Some(r.as_str().to_string());
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if rhs_is_time {
                                if let (Some(v), Some(o), Some(t)) =
                                    (lhs_var.clone(), op.clone(), rhs_time)
                                {
                                    where_time.push((v, o, t));
                                }
                            } else if let (Some(lv), Some(o)) = (lhs_var.clone(), op.clone()) {
                                if let Some(rv) = rhs_var.clone() {
                                    // Defer classification: push to both time_var and value_var lists; execution will keep the valid kind.
                                    where_time_var.push((lv.clone(), o.clone(), rv.clone()));
                                    where_value_var.push((lv, o, rv));
                                } else if let Some(raw) = rhs_raw.clone() {
                                    match parse_where_literal(&raw) {
                                        Ok(rhs) => where_value.push((lv, o, rhs)),
                                        Err(error) => {
                                            *exec_error = Some(error);
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Rule::return_clause => {
                    let mut returns: Vec<String> = Vec::new();
                    for structure in clause.into_inner() {
                        if structure.as_rule() == Rule::recall {
                            returns
                                .push(structure.into_inner().next().unwrap().as_str().to_string());
                        }
                    }
                    let first_time = return_columns.is_none();
                    if first_time {
                        *return_columns = Some(returns.clone());
                    }
                    // Emit meta as soon as we know the column set (only once per search)
                    if first_time
                        && let Some(cols) = return_columns.as_ref()
                        && let SinkFlow::Stop = sink.on_meta(cols)
                    {
                        return;
                    }
                    if any_clause_failed {
                        return;
                    }
                    if enumeration_started {
                        info!(target:"positorium::stream", event="projection_start", bindings=bindings.len(), return_cols=?return_columns.as_ref().unwrap_or(&Vec::new()));
                        // (debug logging removed)
                        // Validate variable references in value predicates
                        if exec_error.is_none() {
                            for (lhs, _op, _rhs) in &where_value {
                                if !variable_kinds.contains_key(lhs) {
                                    *exec_error = Some(DatabaseError::Execution(format!(
                                        "Unknown variable in predicate: {}",
                                        lhs
                                    )));
                                    break;
                                }
                            }
                        }
                        if exec_error.is_some() {
                            return;
                        }
                        if !where_time.is_empty() {
                            let tk = self.database.posit_time_lookup();
                            let guard_time = tk.lock().unwrap();
                            bindings.retain(|b| {
                                if interrupted(execution, exec_error) {
                                    return false;
                                }
                                for (v, op, tcmp) in &where_time {
                                    if let Some((pid, VarKind::Time)) = b.value_slots.get(v) {
                                        if let Some(pt) = guard_time.get(pid) {
                                            let ok = match op.as_str() {
                                                "<" => pt.definitely_before(tcmp),
                                                "<=" => pt.definitely_at_or_before(tcmp),
                                                ">" => pt.definitely_after(tcmp),
                                                ">=" => pt.definitely_at_or_after(tcmp),
                                                "===" | "==" | "=" => pt == tcmp,
                                                "?=" => pt.overlaps(tcmp),
                                                _ => false,
                                            };
                                            if !ok {
                                                return false;
                                            }
                                        } else {
                                            return false;
                                        }
                                    } else {
                                        return false;
                                    }
                                }
                                true
                            });
                        }
                        if !where_time_var.is_empty() {
                            let tk = self.database.posit_time_lookup();
                            let guard_time = tk.lock().unwrap();
                            bindings.retain(|b| {
                                if interrupted(execution, exec_error) {
                                    return false;
                                }
                                for (v1, op, v2) in &where_time_var {
                                    if let (
                                        Some((pid1, VarKind::Time)),
                                        Some((pid2, VarKind::Time)),
                                    ) = (b.value_slots.get(v1), b.value_slots.get(v2))
                                    {
                                        if let (Some(pt1), Some(pt2)) =
                                            (guard_time.get(pid1), guard_time.get(pid2))
                                        {
                                            let ok = match op.as_str() {
                                                "<" => pt1.definitely_before(pt2),
                                                "<=" => pt1.definitely_at_or_before(pt2),
                                                ">" => pt1.definitely_after(pt2),
                                                ">=" => pt1.definitely_at_or_after(pt2),
                                                "===" | "==" | "=" => pt1 == pt2,
                                                "?=" => pt1.overlaps(pt2),
                                                _ => false,
                                            };
                                            if !ok {
                                                return false;
                                            }
                                        } else {
                                            return false;
                                        }
                                    } // else skip (handled in value stage if applicable)
                                }
                                true
                            });
                        }
                        if bindings.is_empty() {
                            return;
                        }
                        if !where_value_var.is_empty() {
                            let posit_keeper = self.database.posit_keeper();
                            let mut pk_guard = posit_keeper.lock().unwrap();
                            bindings.retain(|b| {
                                if interrupted(execution, exec_error) {
                                    return false;
                                }
                                for (l, op, r) in &where_value_var {
                                    let (lpid, lkind) = if let Some(t) = b.value_slots.get(l) { *t } else { if exec_error.is_none() { *exec_error = Some(DatabaseError::Execution(format!("Unknown variable in predicate: {}", l))); } return false; };
                                    let (rpid, rkind) = if let Some(t) = b.value_slots.get(r) { *t } else { if exec_error.is_none() { *exec_error = Some(DatabaseError::Execution(format!("Unknown variable in predicate: {}", r))); } return false; };
                                    if lkind == VarKind::Time || rkind == VarKind::Time { continue; } // handled by where_time_var stage
                                    if lkind != VarKind::Value || rkind != VarKind::Value { if exec_error.is_none() { *exec_error = Some(DatabaseError::Execution(format!("Non-value variable used in value predicate: {} or {}", l, r))); } return false; }
                                    let Some(lhs) = pk_guard
                                        .posit::<LiteralValue>(lpid)
                                        .map(|posit| posit.value().clone())
                                    else {
                                        return false;
                                    };
                                    let Some(rhs) = pk_guard
                                        .posit::<LiteralValue>(rpid)
                                        .map(|posit| posit.value().clone())
                                    else {
                                        return false;
                                    };
                                    let comparison = if matches!(op.as_str(), "<" | "<=" | ">" | ">=") {
                                        lhs.semantic_cmp(&rhs)
                                            .map(|ordering| cmp_ordering(ordering, op))
                                            .map_err(|error| {
                                                format!("Ordering comparison not allowed: {error}")
                                            })
                                    } else if op == "===" {
                                        Ok(lhs.exactly_equals(&rhs))
                                    } else if op == "?=" {
                                        lhs.possibly_equals(&rhs)
                                    } else if op == "=" || op == "==" {
                                        lhs.nominally_equals(&rhs)
                                    } else {
                                        Err(format!("unsupported comparison operator '{op}'"))
                                    };
                                    let pass = match comparison {
                                        Ok(pass) => pass,
                                        Err(error) => {
                                            if exec_error.is_none() {
                                                *exec_error = Some(DatabaseError::Execution(error));
                                            }
                                            false
                                        }
                                    };
                                    if !pass { return false; }
                                }
                                true
                            });
                            if exec_error.is_some() {
                                return;
                            }
                            if bindings.is_empty() {
                                return;
                            }
                        }
                        if !where_value.is_empty() {
                            let posit_keeper = self.database.posit_keeper();
                            let mut pk_guard = posit_keeper.lock().unwrap();
                            bindings.retain(|b| {
                                if interrupted(execution, exec_error) {
                                    return false;
                                }
                                for (lhs, op, rhs) in &where_value {
                                    // locate lhs posit/value
                                    let (pid, vkind) = if let Some(tup) = b.value_slots.get(lhs) { *tup } else { if exec_error.is_none() { *exec_error = Some(DatabaseError::Execution(format!("Unknown variable in predicate: {}", lhs))); } return false; };
                                    if vkind != VarKind::Value { if exec_error.is_none() { *exec_error = Some(DatabaseError::Execution(format!("Non-value variable used in value predicate: {}", lhs))); } return false; }
                                    let Some(stored) = pk_guard
                                        .posit::<LiteralValue>(pid)
                                        .map(|posit| posit.value().clone())
                                    else {
                                        return false;
                                    };
                                    let ordering = matches!(op.as_str(), "<" | "<=" | ">" | ">=");
                                    let comparison = if ordering {
                                        stored
                                            .semantic_cmp(rhs)
                                            .map(|ordering| cmp_ordering(ordering, op))
                                            .map_err(|error| {
                                                let guidance = if stored.family()
                                                    == LiteralFamily::Certainty
                                                    || rhs.family() == LiteralFamily::Certainty
                                                {
                                                    "; certainty comparisons require a percent sign (%) on both sides"
                                                } else {
                                                    ""
                                                };
                                                format!("Ordering comparison not allowed: {error}{guidance}")
                                            })
                                    } else if op == "===" {
                                        Ok(stored.exactly_equals(rhs))
                                    } else if op == "?=" {
                                        stored.possibly_equals(rhs)
                                    } else if op == "=" || op == "==" {
                                        stored.nominally_equals(rhs)
                                    } else {
                                        Err(format!("unsupported comparison operator '{op}'"))
                                    };
                                    let pass = match comparison {
                                        Ok(pass) => pass,
                                        Err(error) => {
                                            if exec_error.is_none() {
                                                *exec_error = Some(DatabaseError::Execution(error));
                                            }
                                            false
                                        }
                                    };
                                    if !pass { return false; }
                                }
                                true
                            });
                            if exec_error.is_some() {
                                return;
                            }
                            if bindings.is_empty() {
                                return;
                            }
                        }
                        let posit_keeper = self.database.posit_keeper();
                        let time_lookup = self.database.posit_time_lookup();
                        let mut pk_guard = posit_keeper.lock().unwrap();
                        let time_guard = time_lookup.lock().unwrap();

                        // Column-level inference removed; we now collect a per-row types vector.
                        // Emission handled after full clause scan; see post-clause block.
                        if !enumeration_started {
                            info!(target:"positorium::stream", event="projection_skipped", reason="no_enumeration", any_clause_failed=any_clause_failed);
                            return;
                        }
                        for b in bindings.iter() {
                            if interrupted(execution, exec_error) {
                                return;
                            }
                            info!(target:"positorium::stream", event="row_binding_iter", identities=b.identities.len(), value_slots=b.value_slots.len(), posit_vars=b.posit_vars.len());
                            let mut row: Vec<ResultCell> = Vec::with_capacity(returns.len());
                            let mut row_ok = true;
                            for rv in &returns {
                                match variable_kinds.get(rv) {
                                    Some(VarKind::Identity) => {
                                        if let Some(idt) = b.identities.get(rv) {
                                            row.push(ResultCell::new(
                                                ResultCellKind::Thing,
                                                idt.to_string(),
                                            ));
                                        } else if let Some(pid) = b.posit_vars.get(rv) {
                                            row.push(ResultCell::new(
                                                ResultCellKind::Thing,
                                                pid.to_string(),
                                            ));
                                        } else {
                                            info!(target:"positorium::stream", event="row_skip", reason="missing_identity", var=%rv);
                                            row_ok = false;
                                            break;
                                        }
                                    }
                                    Some(VarKind::Value) | Some(VarKind::Time) => {
                                        if let Some((pid, kind)) = b.value_slots.get(rv) {
                                            let captured = if *kind == VarKind::Time {
                                                time_guard.get(pid).map(|pt| {
                                                    ResultCell::new(
                                                        ResultCellKind::Time,
                                                        pt.to_string(),
                                                    )
                                                })
                                            } else {
                                                pk_guard.posit::<LiteralValue>(*pid).map(|p| {
                                                    ResultCell::new(
                                                        ResultCellKind::Literal,
                                                        p.value().token(),
                                                    )
                                                })
                                            };
                                            if let Some(cell) = captured {
                                                row.push(cell);
                                            } else {
                                                info!(target:"positorium::stream", event="row_skip", reason="no_capture", var=%rv);
                                                row_ok = false;
                                                break;
                                            }
                                        } else {
                                            // b.value_slots.get(rv) None
                                            info!(target:"positorium::stream", event="row_skip", reason="missing_value_slot", var=%rv);
                                            row_ok = false;
                                            break;
                                        }
                                    } // Value | Time
                                    _ => {
                                        info!(target:"positorium::stream", event="row_skip", reason="unknown_kind", var=%rv);
                                        row_ok = false;
                                        break;
                                    }
                                }
                            }
                            if row_ok && let SinkFlow::Stop = sink.push(row) {
                                break;
                            }
                        }
                        info!(target:"positorium::stream", event="projection_complete");
                        return;
                    }
                } // end Rule::return_clause branch
                Rule::limit_clause => { /* ignored here */ }
                _ => println!("Unknown clause: {:?}", clause),
            }
        }
    }
    /// Parse and execute a Traqula script, discarding any search rows.
    ///
    /// All parsing, execution, cancellation, and persistence failures are returned
    /// to the caller. Use the collection or streaming methods when rows are needed.
    pub fn execute(&self, traqula: &str) -> Result<(), DatabaseError> {
        self.execute_collect_multi(traqula).map(|_| ())
    }

    /// Execute a script and collect one structured result cell per projected value.
    pub fn execute_collect(&self, traqula: &str) -> Result<CollectedResult, DatabaseError> {
        self.execute_collect_with_options(traqula, ExecutionOptions::default())
    }

    /// Execute and collect a script with deterministic execution options.
    pub fn execute_collect_with_options(
        &self,
        traqula: &str,
        options: ExecutionOptions,
    ) -> Result<CollectedResult, DatabaseError> {
        let execution = options.resolve(traqula)?;
        let _owner = self.acquire_owner(&execution)?;
        execution.check()?;
        let metadata = execution.metadata.clone();
        let mut variables: Variables = Variables::default();
        struct CollectSink {
            rows: Vec<Vec<ResultCell>>,
            limit: Option<usize>,
            limited: bool,
        }
        impl RowSink for CollectSink {
            fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
                if let Some(l) = self.limit
                    && self.rows.len() >= l
                {
                    self.limited = true;
                    return SinkFlow::Stop;
                }
                self.rows.push(row);
                SinkFlow::Continue
            }
        }
        let mut collector = CollectSink {
            rows: Vec::new(),
            limit: None,
            limited: false,
        };
        let mut return_columns: Option<Vec<String>> = None;
        // grammar now supports optional limit clause; parse directly
        let parse_result = TraqulaParser::parse(Rule::traqula, traqula.trim());
        let traqula = match parse_result {
            Ok(pairs) => pairs,
            Err(err) => {
                let mut msg = format!("{}", err);
                if let ErrorVariant::ParsingError {
                    positives,
                    negatives: _,
                } = err.variant
                    && !positives.is_empty()
                {
                    let mut expected: Vec<&'static str> =
                        positives.iter().map(|r| friendly_rule_name(*r)).collect();
                    expected.sort();
                    expected.dedup();
                    msg.push_str(&format!("\nExpected one of: {}", expected.join(", ")));
                }
                return Err(DatabaseError::Parse {
                    message: msg,
                    line: None,
                    col: None,
                });
            }
        };
        let mut search_count = 0usize;
        for command in traqula {
            execution.check()?;
            match command.as_rule() {
                Rule::add_role => self.add_role(command)?,
                Rule::add_posit => {
                    self.add_posit(command, &mut variables, &metadata.resolved_now, &execution)?;
                }
                Rule::search => {
                    let mut search_variables: Variables = Variables::default();
                    search_count += 1;
                    // Extract per-search limit and install into sink (overwrite any prior; only meaningful when one search in script)
                    let limit = {
                        let mut l = None;
                        let cloned = command.clone();
                        for c in cloned.into_inner() {
                            if c.as_rule() == Rule::limit_clause {
                                for p in c.into_inner() {
                                    if let Ok(v) = p.as_str().parse::<usize>() {
                                        l = Some(v);
                                    }
                                }
                            }
                        }
                        l
                    };
                    collector.limit = execution.effective_limit(limit);
                    let mut err = None;
                    self.search(
                        command,
                        &mut search_variables,
                        &mut collector,
                        &mut return_columns,
                        &mut err,
                        &execution,
                    );
                    if let Some(e) = err {
                        return Err(e);
                    }
                }
                Rule::EOI => (),
                _ => (),
            }
        }
        let cols = return_columns.unwrap_or_default();
        let row_count = collector.rows.len();
        let limited = search_count == 1 && collector.limited;
        Ok(CollectedResult {
            columns: cols,
            rows: collector.rows,
            row_count,
            limited,
            metadata,
        })
    }

    /// Execute a script and collect separate result sets for each search command.
    /// This provides the foundation for a multi-result JSON protocol.
    pub fn execute_collect_multi(
        &self,
        traqula: &str,
    ) -> Result<Vec<CollectedResultSet>, DatabaseError> {
        self.execute_collect_multi_with_options(traqula, ExecutionOptions::default())
    }

    /// Execute and collect a multi-search script with deterministic execution options.
    pub fn execute_collect_multi_with_options(
        &self,
        traqula: &str,
        options: ExecutionOptions,
    ) -> Result<Vec<CollectedResultSet>, DatabaseError> {
        let execution = options.resolve(traqula)?;
        let _owner = self.acquire_owner(&execution)?;
        execution.check()?;
        let metadata = execution.metadata.clone();
        let mut variables: Variables = Variables::default();
        // Parse once
        let parse_result = TraqulaParser::parse(Rule::traqula, traqula.trim());
        let traqula = match parse_result {
            Ok(pairs) => pairs,
            Err(err) => {
                let mut msg = format!("{}", err);
                if let ErrorVariant::ParsingError {
                    positives,
                    negatives: _,
                } = err.variant
                    && !positives.is_empty()
                {
                    let mut expected: Vec<&'static str> =
                        positives.iter().map(|r| friendly_rule_name(*r)).collect();
                    expected.sort();
                    expected.dedup();
                    msg.push_str(&format!("\nExpected one of: {}", expected.join(", ")));
                }
                return Err(DatabaseError::Parse {
                    message: msg,
                    line: None,
                    col: None,
                });
            }
        };
        let mut results: Vec<CollectedResultSet> = Vec::new();
        for command in traqula {
            execution.check()?;
            match command.as_rule() {
                Rule::add_role => self.add_role(command)?,
                Rule::add_posit => {
                    self.add_posit(command, &mut variables, &metadata.resolved_now, &execution)?;
                }
                Rule::search => {
                    let mut search_variables: Variables = Variables::default();
                    struct LocalSink {
                        rows: Vec<Vec<ResultCell>>,
                        limit: Option<usize>,
                        limited: bool,
                    }
                    impl RowSink for LocalSink {
                        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
                            if let Some(l) = self.limit
                                && self.rows.len() >= l
                            {
                                self.limited = true;
                                return SinkFlow::Stop;
                            }
                            self.rows.push(row);
                            SinkFlow::Continue
                        }
                    }
                    let mut sink = LocalSink {
                        rows: Vec::new(),
                        limit: None,
                        limited: false,
                    };
                    // Capture raw search text before moving command into search execution
                    let raw_search_string = command.as_str().trim().to_string();
                    let query_limit = {
                        let mut l = None;
                        let cloned = command.clone();
                        for c in cloned.into_inner() {
                            if c.as_rule() == Rule::limit_clause {
                                for p in c.into_inner() {
                                    if let Ok(v) = p.as_str().parse::<usize>() {
                                        l = Some(v);
                                    }
                                }
                            }
                        }
                        l
                    };
                    sink.limit = execution.effective_limit(query_limit);
                    let mut local_return_columns: Option<Vec<String>> = None;
                    let mut err = None;
                    self.search(
                        command,
                        &mut search_variables,
                        &mut sink,
                        &mut local_return_columns,
                        &mut err,
                        &execution,
                    );
                    if let Some(e) = err {
                        return Err(e);
                    }
                    let cols = local_return_columns.unwrap_or_default();
                    let row_count = sink.rows.len();
                    let limited = sink.limited;
                    results.push(CollectedResultSet {
                        columns: cols,
                        rows: sink.rows,
                        row_count,
                        limited,
                        search: Some(raw_search_string),
                        metadata: metadata.clone(),
                    });
                }
                Rule::EOI => (),
                _ => (),
            }
        }
        Ok(results)
    }

    /// Execute a script containing multiple searches (>=1) and stream each result set with framing callbacks.
    /// Maintains standard variable scoping semantics across searches.
    pub fn execute_stream_multi<C: MultiStreamCallbacks>(
        &self,
        traqula: &str,
        callbacks: &mut C,
    ) -> Result<(), DatabaseError> {
        self.execute_stream_multi_with_options(traqula, callbacks, ExecutionOptions::default())
    }

    /// Stream multiple searches with cooperative timeout, cancellation, and row limits.
    pub fn execute_stream_multi_with_options<C: MultiStreamCallbacks>(
        &self,
        traqula: &str,
        callbacks: &mut C,
        options: ExecutionOptions,
    ) -> Result<(), DatabaseError> {
        let execution = options.resolve(traqula)?;
        let _owner = self.acquire_owner(&execution)?;
        execution.check()?;
        for warning in &execution.metadata.warnings {
            callbacks.on_warning(warning);
        }
        let mut variables: Variables = Variables::default();
        let parse_result = TraqulaParser::parse(Rule::traqula, traqula.trim());
        let pairs = match parse_result {
            Ok(p) => p,
            Err(err) => {
                let mut msg = format!("{}", err);
                if let ErrorVariant::ParsingError {
                    positives,
                    negatives: _,
                } = err.variant
                    && !positives.is_empty()
                {
                    let mut expected: Vec<&'static str> =
                        positives.iter().map(|r| friendly_rule_name(*r)).collect();
                    expected.sort();
                    expected.dedup();
                    msg.push_str(&format!("\nExpected one of: {}", expected.join(", ")));
                }
                return Err(DatabaseError::Parse {
                    message: msg,
                    line: None,
                    col: None,
                });
            }
        };
        let mut set_index = 0usize;
        for command in pairs {
            execution.check()?;
            match command.as_rule() {
                Rule::add_role => self.add_role(command)?,
                Rule::add_posit => {
                    self.add_posit(
                        command,
                        &mut variables,
                        &execution.metadata.resolved_now,
                        &execution,
                    )?;
                }
                Rule::search => {
                    let mut search_variables: Variables = Variables::default();
                    // Extract limit for this search
                    let search_text_full = command.as_str().trim().to_string();
                    let mut limit = None;
                    let cloned = command.clone();
                    for c in cloned.into_inner() {
                        if c.as_rule() == Rule::limit_clause {
                            for p in c.into_inner() {
                                if let Ok(v) = p.as_str().parse::<usize>() {
                                    limit = Some(v);
                                }
                            }
                        }
                    }
                    // Per-set sink bridging to callbacks
                    struct SetSink<'a, C: MultiStreamCallbacks> {
                        cb: &'a mut C,
                        idx: usize,
                        started: bool,
                        search_text: &'a str,
                    }
                    impl<'a, C: MultiStreamCallbacks> RowSink for SetSink<'a, C> {
                        fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
                            self.started = true;
                            info!(target:"positorium::stream", event="set_meta", set_index=self.idx, cols=?columns, search=?self.search_text);
                            self.cb
                                .on_result_set_start(self.idx, columns, self.search_text);
                            SinkFlow::Continue
                        }
                        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
                            if self.cb.on_row(self.idx, row) {
                                SinkFlow::Continue
                            } else {
                                SinkFlow::Stop
                            }
                        }
                    }
                    struct CountingSetSink<'a, C: MultiStreamCallbacks> {
                        inner: SetSink<'a, C>,
                        limit: Option<usize>,
                        count: usize,
                        limited: bool,
                    }
                    impl<'a, C: MultiStreamCallbacks> RowSink for CountingSetSink<'a, C> {
                        fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
                            self.inner.on_meta(columns)
                        }
                        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
                            if let Some(l) = self.limit
                                && self.count >= l
                            {
                                self.limited = true;
                                return SinkFlow::Stop;
                            }
                            match self.inner.push(row) {
                                SinkFlow::Continue => {
                                    self.count += 1;
                                    SinkFlow::Continue
                                }
                                stop => stop,
                            }
                        }
                    }
                    let mut sink = CountingSetSink {
                        inner: SetSink {
                            cb: callbacks,
                            idx: set_index,
                            started: false,
                            search_text: &search_text_full,
                        },
                        limit: execution.effective_limit(limit),
                        count: 0,
                        limited: false,
                    };
                    let mut return_columns: Option<Vec<String>> = None; // ignored here beyond meta
                    let mut err = None;
                    self.search(
                        command,
                        &mut search_variables,
                        &mut sink,
                        &mut return_columns,
                        &mut err,
                        &execution,
                    );
                    if let Some(e) = err {
                        return Err(e);
                    }
                    let finished_count = sink.count;
                    let limited_flag = sink.limited; // drop sink here
                    callbacks.on_result_set_end(set_index, finished_count, limited_flag);
                    set_index += 1;
                }
                Rule::EOI => (),
                _ => (),
            }
        }
        Ok(())
    }
}

/// Map grammar rules to friendly names in error messages.
fn friendly_rule_name(rule: Rule) -> &'static str {
    match rule {
        Rule::traqula => "Traqula script",
        Rule::add_role => "add role",
        Rule::add_posit => "add posit",
        Rule::search => "search",
        Rule::search_clause => "search clause",
        Rule::where_clause => "where clause",
        Rule::return_clause => "return clause",
        Rule::appearance_set | Rule::appearance_set_search => "appearance set [{(...)}]",
        Rule::appearance | Rule::appearance_search => "appearance (..., <role>)",
        Rule::role => "role name",
        Rule::insert => "+variable",
        Rule::recall => "variable",
        Rule::recall_union => "variable union (a|b)",
        Rule::wildcard => "*",
        Rule::appearing_value | Rule::appearing_value_search => "value",
        // Expand time slot expectations to concrete options
        Rule::appearance_time_search => {
            "time literal (e.g., 'YYYY-MM-DD'), time constant (@NOW/@BOT/@EOT), +variable, variable, or *"
        }
        Rule::appearance_time => "time",
        Rule::json => "JSON literal",
        Rule::string => "string literal",
        Rule::int => "integer literal",
        Rule::decimal => "decimal literal",
        Rule::certainty => "certainty (e.g., 100%)",
        Rule::time => "time literal (e.g., 'YYYY-MM-DD')",
        Rule::constant => "time constant (@NOW/@BOT/@EOT)",
        Rule::as_of_clause => "as of <time> or <variable>",
        Rule::comparator => "comparator (<, <=, >, >=, =, ==)",
        _ => "token",
    }
}

/// Streaming Cartesian product (indices): calls `mut f` with the index vector for each tuple, avoiding temporary tuple materialization.
pub fn for_each_cartesian_indices<F: FnMut(&[usize])>(lists: &[&[impl Copy]], mut f: F) {
    // Early return on empty input
    if lists.is_empty() {
        return;
    }
    // Track indices for each list; -1 represents uninitialized state for the first increment
    let n = lists.len();
    let mut idx = vec![0usize; n];
    // Verify no empty inner list; if any are empty, there are no combinations
    if lists.iter().any(|s| s.is_empty()) {
        return;
    }
    // Emit until we overflow the most significant position
    loop {
        // Provide the current indices to the callback
        f(&idx);

        // Increment positions from least significant upwards
        let mut carry_pos = n;
        for pos in (0..n).rev() {
            idx[pos] += 1;
            if idx[pos] < lists[pos].len() {
                carry_pos = pos;
                break;
            } else {
                idx[pos] = 0;
            }
        }
        if carry_pos == n {
            break; // overflowed most significant digit; done
        }
    }
}
