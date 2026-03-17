//! Data type abstractions and concrete value/time representations.
//!
//! The [`DataType`] trait is implemented for all value/time types that can be
//! stored inside a posit. Each implementation declares a stable numeric
//! identifier (`UID`) and a textual name (`DATA_TYPE`) which are persisted in
//! the catalog. A helper [`DataType::convert`] function reconstructs a value
//! from a SQLite `ValueRef` during restoration.
//!
//! # Implementing a Custom Data Type
//! To add a new logical value type implement the trait for your type and make
//! sure it satisfies the required bounds. Example (toy wrapper around `i64`):
//! ```
//! # #[cfg(feature = "persistence")]
//! # {
//! use rusqlite::types::{ValueRef, ToSql, ToSqlOutput};
//! # }
//! use std::fmt;
//! use positorium::datatype::DataType;
//! #[derive(Eq, PartialEq, Hash)]
//! struct MyCount(i64);
//! impl fmt::Display for MyCount { fn fmt(&self, f:&mut fmt::Formatter)->fmt::Result { write!(f, "{}", self.0) } }
//! #[cfg(feature = "persistence")]
//! impl ToSql for MyCount { fn to_sql(&self)->rusqlite::Result<ToSqlOutput<'_>> { Ok(ToSqlOutput::from(self.0)) } }
//! impl DataType for MyCount {
//!     const UID: u8 = 250; // choose a free id
//!     const DATA_TYPE: &'static str = "MyCount";
//!     #[cfg(feature = "persistence")]
//!     fn convert(v:&ValueRef)->Self { MyCount(v.as_i64().unwrap()) }
//! }
//! assert_eq!(MyCount(3).data_type(), "MyCount");
//! ```
//!
//! # Special Provided Types
//! * [`Certainty`] – subjective probability-like measure with saturation.
//! * [`Decimal`] – arbitrary precision numeric (BigDecimal wrapper).
//! * [`JSON`] – JSON payload using `jsondata::Json`.
//! * [`Time`] / [`TimeType`] – hierarchical (abstract + concrete) temporal points.
//!
//! # Equality & Ordering
//! Most wrapper types delegate ordering and equality to their internal
//! representations while ensuring a stable textual form for persistence.
//!
//! # Notes
//! Adding a new [`DataType`] also requires updating restoration logic in
//! `persist.rs` (see MAINTENANCE comments there).
// used for persistence
#[cfg(feature = "persistence")]
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

// used for timestamps in the database
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
// used for decimal numbers
use bigdecimal::BigDecimal;
// used for JSON
use jsondata::Json;

// used when parsing a string to a DateTime<Utc>
use std::str::FromStr;
// used to print out readable forms of a data type
use std::fmt;
// used to indicate that data types need to be hashable
use std::hash::{Hash, Hasher};
// used to overload common operations for datatypes
use std::ops;
// used when implementing PartialOrd for Time
use std::cmp::Ordering;

use crate::traqula::parse_time;

/// Trait implemented by all logical value/time types that may appear in a posit.
///
/// Implementors must provide a stable numeric `UID` and textual name
/// `DATA_TYPE`. These are persisted so changing them breaks backward
/// compatibility.
#[cfg(feature = "persistence")]
pub trait DataType: fmt::Display + Eq + Hash + Send + Sync + ToSql {
    /// Stable numeric identifier for catalog persistence.
    const UID: u8;
    /// Stable human readable name stored in persistence.
    const DATA_TYPE: &'static str;
    /// Convert from a raw SQLite value (during restore).
    fn convert(value: &ValueRef) -> Self;
    /// Returns the textual data type name.
    fn data_type(&self) -> &'static str {
        Self::DATA_TYPE
    }
    /// Returns the numeric identifier.
    fn identifier(&self) -> u8 {
        Self::UID
    }
}

#[cfg(not(feature = "persistence"))]
pub trait DataType: fmt::Display + Eq + Hash + Send + Sync {
    /// Stable numeric identifier for catalog persistence.
    const UID: u8;
    /// Stable human readable name stored in persistence.
    const DATA_TYPE: &'static str;
    /// Returns the textual data type name.
    fn data_type(&self) -> &'static str {
        Self::DATA_TYPE
    }
    /// Returns the numeric identifier.
    fn identifier(&self) -> u8 {
        Self::UID
    }
}

// ------------- Data Types --------------
impl DataType for Certainty {
    const UID: u8 = 1;
    const DATA_TYPE: &'static str = "Certainty";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> Certainty {
        let raw_i64 = value.as_i64().unwrap_or_else(|e| {
            panic!("[positorium][restore] Expected integer for Certainty, got error: {e:?}")
        });
        let alpha = i8::try_from(raw_i64).unwrap_or_else(|e| {
            panic!("[positorium][restore] Certainty value out of i8 range (raw={raw_i64}) -> {e:?}")
        });
        Certainty { alpha }
    }
}
impl DataType for String {
    const UID: u8 = 2;
    const DATA_TYPE: &'static str = "String";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> String {
        match value.as_str() {
            Ok(s) => s.to_string(),
            Err(e) => panic!("[positorium][restore] Failed to read persisted String value: {e:?}"),
        }
    }
}
impl DataType for NaiveDateTime {
    const UID: u8 = 3;
    const DATA_TYPE: &'static str = "NaiveDateTime";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> NaiveDateTime {
        let raw = value.as_str().unwrap_or_else(|e| {
            panic!("[positorium][restore] NaiveDateTime not stored as text: {e:?}")
        });
        NaiveDateTime::from_str(raw).unwrap_or_else(|e| {
            panic!("[positorium][restore] Failed to parse NaiveDateTime from '{raw}': {e:?}")
        })
    }
}
impl DataType for NaiveDate {
    const UID: u8 = 4;
    const DATA_TYPE: &'static str = "NaiveDate";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> NaiveDate {
        let raw = value
            .as_str()
            .unwrap_or_else(|e| panic!("[positorium][restore] NaiveDate not stored as text: {e:?}"));
        NaiveDate::from_str(raw).unwrap_or_else(|e| {
            panic!("[positorium][restore] Failed to parse NaiveDate from '{raw}': {e:?}")
        })
    }
}
impl DataType for i64 {
    const UID: u8 = 5;
    const DATA_TYPE: &'static str = "i64";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> i64 {
        value
            .as_i64()
            .unwrap_or_else(|e| panic!("[positorium][restore] Failed to read i64 value: {e:?}"))
    }
}
impl DataType for Decimal {
    const UID: u8 = 6;
    const DATA_TYPE: &'static str = "Decimal";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> Decimal {
        let raw = value
            .as_str()
            .unwrap_or_else(|e| panic!("[positorium][restore] Decimal not stored as text: {e:?}"));
        match BigDecimal::from_str(raw) {
            Ok(d) => Decimal(d),
            Err(e) => panic!("[positorium][restore] Failed to parse Decimal from '{raw}': {e:?}"),
        }
    }
}
impl DataType for JSON {
    const UID: u8 = 7;
    const DATA_TYPE: &'static str = "JSON";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> JSON {
        let raw = value
            .as_str()
            .unwrap_or_else(|e| panic!("[positorium][restore] JSON not stored as text: {e:?}"));
        match Json::from_str(raw) {
            Ok(j) => JSON(j),
            Err(e) => panic!("[positorium][restore] Failed to parse JSON from '{raw}': {e:?}"),
        }
    }
}
impl DataType for Time {
    const UID: u8 = 8;
    const DATA_TYPE: &'static str = "Time";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> Time {
        let raw = value
            .as_str()
            .unwrap_or_else(|e| panic!("[positorium][restore] Time not stored as text: {e:?}"));
        // Try persisted canonical formats first, then fall back to script literals.
        match Time::parse_persisted(raw) {
            Some(t) => t,
            None => parse_time(raw).unwrap_or_else(|| {
                panic!("[positorium][restore] Failed to parse Time literal '{raw}' from persistence (no recognized format)")
            }),
        }
    }
}

// Special types below
/// JSON value wrapper implementing [`DataType`]. Display prints compact JSON.
#[derive(Eq, PartialEq, PartialOrd, Ord, Clone)]
pub struct JSON(Json);

impl JSON {
    pub fn from_str(s: &str) -> Option<JSON> {
        match Json::from_str(s) {
            Ok(json) => Some(JSON(json)),
            _ => None,
        }
    }
}
#[cfg(feature = "persistence")]
impl ToSql for JSON {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.to_string()))
    }
}
#[cfg(feature = "persistence")]
impl FromSql for JSON {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        rusqlite::Result::Ok(JSON(Json::from_str(value.as_str().unwrap()).unwrap()))
    }
}
impl Hash for JSON {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_string().hash(state);
    }
}
impl fmt::Display for JSON {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl ops::Deref for JSON {
    type Target = Json;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
/*
// all our values are immutable
impl ops::DerefMut for JSON {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
*/

/*
Certainty is a subjective measure that can be held against a posit.
This ranges from being certain of a posit to certain of its opposite,
exemplified by the following statements:

The master will certainly win.
The master will probably win.
The master may win.
The master is unlikely to win.
The master has a small chance of winning.
I have no idea whether the master could win or lose (not win).
The master has a small risk of losing.
The master is unlikely to lose.
The master may lose.
The master will probably lose.
The master will certainly lose.

*/

/// Subjective certainty value in [-1,1] saturated & stored internally as an i8 percent.
///
/// Negative numbers indicate certainty of the *opposite* proposition.
///
/// # Example
/// ```
/// use positorium::datatype::Certainty;
/// let a = Certainty::new(0.30); // +30%
/// let b = Certainty::new(-0.20); // -20%
/// // By the non-contradictory inequality (#negatives + sum(alpha) <= 1),
/// // this pair is inconsistent: 1 + 0.10 = 1.10 > 1
/// assert!(!Certainty::consistent(&[a.clone(), b.clone()]));
/// // Display includes the leading zero for decimals in (-1,1)
/// assert_eq!(format!("{}", a), "0.30");
/// let s = f64::from(a.clone()) + f64::from(b.clone());
/// assert!((s - 0.10).abs() < 1e-6);
/// ```
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Debug)]
pub struct Certainty {
    alpha: i8,
}

impl Certainty {
    pub fn new<T: Into<f64>>(a: T) -> Self {
        let a = a.into();
        let a = if a < -1. {
            -1.
        } else if a > 1. {
            1.
        } else {
            a
        };
        Self {
            alpha: (100f64 * a) as i8,
        }
    }
    /// Returns true if a set of reliabilities is non-contradictory per
    /// the inequality from Transitional Modeling:
    ///
    /// (1/2) * Σ [1 - α_i/|α_i|] + Σ α_i ≤ 1
    ///
    /// where α_i ∈ [-1,1] are individual reliabilities. The first term counts
    /// the number of negative assertions (each contributes 1), and the second
    /// term is the signed sum of reliabilities.
    ///
    /// # Examples
    ///
    /// Donna's example: +0.25 and -0.75 yields 0.5, which is ≤ 1.
    /// ```
    /// use positorium::datatype::Certainty;
    /// let s1 = [Certainty::new(0.25), Certainty::new(-0.75)];
    /// assert!(Certainty::consistent(&s1));
    /// ```
    ///
    /// Twenty 95% negative assertions remain non-contradictory, but the 21st does not.
    /// ```
    /// use positorium::datatype::Certainty;
    /// let twenty = vec![Certainty::new(-0.95); 20];
    /// assert!(Certainty::consistent(&twenty));
    /// let twenty_one = vec![Certainty::new(-0.95); 21];
    /// assert!(!Certainty::consistent(&twenty_one));
    /// ```
    pub fn consistent(rs: &[Certainty]) -> bool {
        let r_total = rs
            .iter()
            .map(|r: &Certainty| r.alpha as i32)
            .filter(|i| *i != 0)
            .fold(0, |sum, i| sum + 100 * (1 - i.signum()))
            / 2
            + rs.iter()
                .map(|r: &Certainty| r.alpha as i32)
                .filter(|i| *i != 0)
                .sum::<i32>();

        r_total <= 100
    }
}
impl ops::Add for Certainty {
    type Output = f64;
    fn add(self, other: Certainty) -> f64 {
        (self.alpha as f64 + other.alpha as f64) / 100f64
    }
}
impl ops::Mul for Certainty {
    type Output = f64;
    fn mul(self, other: Certainty) -> f64 {
        (self.alpha as f64 / 100f64) * (other.alpha as f64 / 100f64)
    }
}
impl fmt::Display for Certainty {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.alpha {
            -100 => write!(f, "-1"),
            -99..=-1 => write!(f, "-0.{}", -self.alpha),
            0 => write!(f, "0"),
            1..=99 => write!(f, "0.{}", self.alpha),
            100 => write!(f, "1"),
            _ => write!(f, "?"),
        }
    }
}
impl From<Certainty> for f64 {
    fn from(r: Certainty) -> f64 {
        r.alpha as f64 / 100f64
    }
}
impl<'a> From<&'a Certainty> for f64 {
    fn from(r: &Certainty) -> f64 {
        r.alpha as f64 / 100f64
    }
}
#[cfg(feature = "persistence")]
impl ToSql for Certainty {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.alpha))
    }
}
#[cfg(feature = "persistence")]
impl FromSql for Certainty {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        rusqlite::Result::Ok(Certainty {
            alpha: i8::try_from(value.as_i64().unwrap()).ok().unwrap(),
        })
    }
}

/// Arbitrary precision decimal wrapper implementing [`DataType`].
#[derive(Eq, PartialEq, Hash, PartialOrd, Ord, Clone)]
pub struct Decimal(BigDecimal);

impl Decimal {
    pub fn from_str(s: &str) -> Option<Decimal> {
        match BigDecimal::from_str(s) {
            Ok(decimal) => Some(Decimal(decimal)),
            _ => None,
        }
    }
}
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
#[cfg(feature = "persistence")]
impl FromSql for Decimal {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        rusqlite::Result::Ok(Decimal(
            BigDecimal::from_str(value.as_str().unwrap()).unwrap(),
        ))
    }
}
#[cfg(feature = "persistence")]
impl ToSql for Decimal {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.to_string()))
    }
}
impl ops::Deref for Decimal {
    type Target = BigDecimal;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl ops::DerefMut for Decimal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// TODO: We will use a specialized time type instead of the
// trait constrained generic
/// Hierarchical temporal points (abstract sentinels + concrete resolutions).
#[derive(Eq, PartialEq, Ord, Debug, Hash, Clone)]
pub enum TimeType {
    // abstract time points
    BeginningOfTime,
    EndOfTime,
    // concrete time points
    Year(i32),
    YearMonth(i32, u8),
    Date(NaiveDate),
    DateTime(NaiveDateTime),
}

impl PartialOrd for TimeType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            // abstact type combinations
            (TimeType::BeginningOfTime, TimeType::BeginningOfTime)
            | (TimeType::EndOfTime, TimeType::EndOfTime) => Some(Ordering::Equal),
            (TimeType::BeginningOfTime, _) | (_, TimeType::EndOfTime) => Some(Ordering::Less),
            (_, TimeType::BeginningOfTime) | (TimeType::EndOfTime, _) => Some(Ordering::Greater),
            // concrete type combinations
            (TimeType::Year(y_self), type_other) => match type_other {
                TimeType::Year(y) => y_self.partial_cmp(y),
                TimeType::YearMonth(y, _) => y_self.partial_cmp(y),
                TimeType::Date(d) => y_self.partial_cmp(&d.year()),
                TimeType::DateTime(d) => y_self.partial_cmp(&d.year()),
                _ => None,
            },
            (TimeType::YearMonth(y_self, m_self), type_other) => match type_other {
                TimeType::Year(y) => y_self.partial_cmp(y),
                TimeType::YearMonth(y, m) => match y_self.partial_cmp(y) {
                    Some(Ordering::Equal) => m_self.partial_cmp(m),
                    _ => y_self.partial_cmp(y),
                },
                TimeType::Date(d) => match y_self.partial_cmp(&d.year()) {
                    Some(Ordering::Equal) => m_self.partial_cmp(&(d.month() as u8)),
                    _ => y_self.partial_cmp(&d.year()),
                },
                TimeType::DateTime(d) => match y_self.partial_cmp(&d.year()) {
                    Some(Ordering::Equal) => m_self.partial_cmp(&(d.month() as u8)),
                    _ => y_self.partial_cmp(&d.year()),
                },
                _ => None,
            },
            (TimeType::Date(d_self), type_other) => match type_other {
                TimeType::Year(y) => d_self.year().partial_cmp(y),
                TimeType::YearMonth(y, m) => match d_self.year().partial_cmp(y) {
                    Some(Ordering::Equal) => (d_self.month() as u8).partial_cmp(m),
                    _ => d_self.year().partial_cmp(y),
                },
                TimeType::Date(d) => d_self.partial_cmp(d),
                TimeType::DateTime(d) => d_self.partial_cmp(&d.date()),
                _ => None,
            },
            (TimeType::DateTime(d_self), type_other) => match type_other {
                TimeType::Year(y) => d_self.year().partial_cmp(y),
                TimeType::YearMonth(y, m) => match d_self.year().partial_cmp(y) {
                    Some(Ordering::Equal) => (d_self.month() as u8).partial_cmp(m),
                    _ => d_self.year().partial_cmp(y),
                },
                TimeType::Date(d) => d_self.date().partial_cmp(d),
                TimeType::DateTime(d) => d_self.partial_cmp(d),
                _ => None,
            },
        }
    }
}

/// Public temporal wrapper used in posits. Provides helper constructors.
#[derive(Eq, PartialEq, PartialOrd, Ord, Debug, Hash, Clone)]
pub struct Time {
    moment: TimeType,
}
impl Time {
    /// Now (UTC) time point.
    pub fn new() -> Time {
        Time {
            moment: TimeType::DateTime(Utc::now().naive_utc()),
        }
    }
    /// Abstract lower bound.
    pub fn new_beginning_of_time() -> Time {
        Time {
            moment: TimeType::BeginningOfTime,
        }
    }
    /// Abstract upper bound.
    pub fn new_end_of_time() -> Time {
        Time {
            moment: TimeType::EndOfTime,
        }
    }
    // TODO: may panic
    /// Creates a `Time` from a year string (e.g. "2024"). Panics on invalid input.
    pub fn new_year_from(d: &str) -> Time {
        Time {
            moment: TimeType::Year(d.parse::<i32>().unwrap()),
        }
    }
    /// Creates a `Time` from a year-month string ("YYYY-MM"). Panics on invalid input.
    pub fn new_year_month_from(d: &str) -> Time {
        let mut year = String::new();
        let mut month = String::new();
        for c in d.chars() {
            if c != '-' {
                if year.len() < 4 {
                    year.push(c);
                } else {
                    month.push(c);
                }
            }
        }
        Time {
            moment: TimeType::YearMonth(year.parse::<i32>().unwrap(), month.parse::<u8>().unwrap()),
        }
    }
    /// Creates a `Time` from a date string ("YYYY-MM-DD"). Panics on invalid input.
    pub fn new_date_from(d: &str) -> Time {
        Time {
            moment: TimeType::Date(NaiveDate::from_str(d).unwrap()),
        }
    }
    /// Creates a `Time` from a datetime string acceptable by `NaiveDateTime::from_str`.
    pub fn new_datetime_from(d: &str) -> Time {
        match NaiveDateTime::from_str(d) {
            Ok(dt) => Time {
                moment: TimeType::DateTime(dt),
            },
            Err(e) => panic!("[positorium][time] Failed to parse datetime '{d}': {e}"),
        }
    }
    /// Creates a `Time` from an existing `NaiveDateTime` (internal helper to avoid double parsing)
    pub fn from_naive_datetime(dt: NaiveDateTime) -> Time {
        Time {
            moment: TimeType::DateTime(dt),
        }
    }
    /// Parse a persisted canonical textual form of Time (no quotes, produced by Display).
    /// Accepted forms:
    /// BOT | EOT | YYYY | YYYY-MM | YYYY-MM-DD | YYYY-MM-DD HH:MM:SS[.fraction] | YYYY-MM-DDTHH:MM:SS[.fraction]
    fn parse_persisted(raw: &str) -> Option<Time> {
        let s = raw.trim();
        match s {
            "BOT" => return Some(Time::new_beginning_of_time()),
            "EOT" => return Some(Time::new_end_of_time()),
            _ => {}
        }
        if (s.contains(':')) && (s.contains(' ') || s.contains('T')) && s.matches('-').count() == 2
        {
            if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
                return Some(Time::from_naive_datetime(dt));
            }
            if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
                return Some(Time::from_naive_datetime(dt));
            }
        }
        if s.matches('-').count() == 2 && !s.contains(':') {
            if NaiveDate::from_str(s).is_ok() {
                return Some(Time::new_date_from(s));
            }
        }
        if s.matches('-').count() == 1 && !s.contains(':') {
            if let Some((y, m)) = s.split_once('-') {
                if y.chars().all(|c| c == '-' || c.is_ascii_digit())
                    && m.chars().all(|c| c.is_ascii_digit())
                {
                    if let Ok(mm) = m.parse::<u8>() {
                        if (1..=12).contains(&mm) {
                            return Some(Time::new_year_month_from(s));
                        }
                    }
                }
            }
        }
        if s.chars().all(|c| c == '-' || c.is_ascii_digit()) && (4..=8).contains(&s.len()) {
            return Some(Time::new_year_from(s));
        }
        None
    }
}
impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.moment {
            TimeType::BeginningOfTime => {
                write!(f, "BOT")
            }
            TimeType::EndOfTime => {
                write!(f, "EOT")
            }
            TimeType::Year(y) => {
                write!(f, "{}", y)
            }
            TimeType::YearMonth(y, m) => {
                write!(f, "{}-{}", y, m)
            }
            TimeType::Date(d) => {
                write!(f, "{}", d)
            }
            TimeType::DateTime(d) => {
                write!(f, "{}", d)
            }
        }
    }
}
#[cfg(feature = "persistence")]
impl ToSql for Time {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}
