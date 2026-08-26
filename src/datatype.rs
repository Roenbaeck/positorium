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
//! #[derive(Eq, PartialEq, PartialOrd, Ord, Hash)]
//! struct MyCount(i64);
//! impl fmt::Display for MyCount { fn fmt(&self, f:&mut fmt::Formatter)->fmt::Result { write!(f, "{}", self.0) } }
//! #[cfg(feature = "persistence")]
//! impl ToSql for MyCount { fn to_sql(&self)->rusqlite::Result<ToSqlOutput<'_>> { Ok(ToSqlOutput::from(self.0)) } }
//! impl DataType for MyCount {
//!     const UID: u8 = 250; // choose a free id
//!     const DATA_TYPE: &'static str = "MyCount";
//!     #[cfg(feature = "persistence")]
//!     fn convert(v:&ValueRef)->Result<Self, String> {
//!         v.as_i64().map(MyCount).map_err(|error| error.to_string())
//!     }
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
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike, Utc};
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

#[cfg(feature = "persistence")]
use crate::traqula::parse_time;

/// Trait implemented by all logical value/time types that may appear in a posit.
///
/// Implementors must provide a stable numeric `UID` and textual name
/// `DATA_TYPE`. These are persisted so changing them breaks backward
/// compatibility.
#[cfg(feature = "persistence")]
pub trait DataType: fmt::Display + Eq + Ord + Hash + Send + Sync + ToSql {
    /// Stable numeric identifier for catalog persistence.
    const UID: u8;
    /// Stable human readable name stored in persistence.
    const DATA_TYPE: &'static str;
    /// Convert from a raw SQLite value (during restore).
    fn convert(value: &ValueRef) -> std::result::Result<Self, String>
    where
        Self: Sized;
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
pub trait DataType: fmt::Display + Eq + Ord + Hash + Send + Sync {
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
    fn convert(value: &ValueRef) -> std::result::Result<Certainty, String> {
        let raw_i64 = value.as_i64().map_err(|error| error.to_string())?;
        let alpha = i8::try_from(raw_i64)
            .map_err(|error| format!("certainty {raw_i64} is out of range: {error}"))?;
        Ok(Certainty { alpha })
    }
}
impl DataType for String {
    const UID: u8 = 2;
    const DATA_TYPE: &'static str = "String";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<String, String> {
        value
            .as_str()
            .map(str::to_string)
            .map_err(|error| error.to_string())
    }
}
impl DataType for NaiveDateTime {
    const UID: u8 = 3;
    const DATA_TYPE: &'static str = "NaiveDateTime";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<NaiveDateTime, String> {
        let raw = value.as_str().map_err(|error| error.to_string())?;
        NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f")
            .map_err(|error| error.to_string())
    }
}
impl DataType for NaiveDate {
    const UID: u8 = 4;
    const DATA_TYPE: &'static str = "NaiveDate";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<NaiveDate, String> {
        let raw = value.as_str().map_err(|error| error.to_string())?;
        NaiveDate::from_str(raw).map_err(|error| error.to_string())
    }
}
impl DataType for i64 {
    const UID: u8 = 5;
    const DATA_TYPE: &'static str = "i64";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<i64, String> {
        value.as_i64().map_err(|error| error.to_string())
    }
}
impl DataType for Decimal {
    const UID: u8 = 6;
    const DATA_TYPE: &'static str = "Decimal";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<Decimal, String> {
        let raw = value.as_str().map_err(|error| error.to_string())?;
        BigDecimal::from_str(raw)
            .map(Decimal)
            .map_err(|error| error.to_string())
    }
}
impl DataType for JSON {
    const UID: u8 = 7;
    const DATA_TYPE: &'static str = "JSON";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<JSON, String> {
        let raw = value.as_str().map_err(|error| error.to_string())?;
        Json::from_str(raw)
            .map(JSON)
            .map_err(|error| error.to_string())
    }
}
impl DataType for Time {
    const UID: u8 = 8;
    const DATA_TYPE: &'static str = "Time";
    #[cfg(feature = "persistence")]
    fn convert(value: &ValueRef) -> std::result::Result<Time, String> {
        let raw = value.as_str().map_err(|error| error.to_string())?;
        // Try persisted canonical formats first, then fall back to script literals.
        Time::parse_persisted(raw)
            .or_else(|| parse_time(raw))
            .ok_or_else(|| format!("unrecognized persisted time literal '{raw}'"))
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

    /// Creates a certainty from its exact integer percentage representation.
    pub fn from_percent(alpha: i8) -> Self {
        Self { alpha }
    }

    /// Returns this certainty's exact integer percentage representation.
    pub fn percent(&self) -> i8 {
        self.alpha
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
            -99..=-1 => write!(f, "-0.{:02}", -self.alpha),
            0 => write!(f, "0"),
            1..=99 => write!(f, "0.{:02}", self.alpha),
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
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Public temporal wrapper used in posits. Provides helper constructors.
#[derive(Eq, PartialEq, PartialOrd, Ord, Debug, Hash, Clone)]
pub struct Time {
    moment: TimeType,
}

#[derive(Eq, PartialEq, PartialOrd, Ord, Debug, Clone, Copy)]
struct TemporalInstant {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    nanosecond: u32,
}

#[derive(Eq, PartialEq, PartialOrd, Ord, Debug, Clone, Copy)]
enum TemporalBound {
    NegativeInfinity,
    Finite(TemporalInstant),
    PositiveInfinity,
}

#[derive(Eq, PartialEq, Debug, Clone, Copy)]
struct TimeInterval {
    start: TemporalBound,
    end: TemporalBound,
}

impl TemporalInstant {
    fn start_of_year(year: i32) -> Self {
        Self {
            year,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            nanosecond: 0,
        }
    }

    fn start_of_month(year: i32, month: u8) -> Self {
        Self {
            month,
            ..Self::start_of_year(year)
        }
    }

    fn from_date(date: NaiveDate) -> Self {
        Self {
            year: date.year(),
            month: date.month() as u8,
            day: date.day() as u8,
            hour: 0,
            minute: 0,
            second: 0,
            nanosecond: 0,
        }
    }

    fn from_datetime(datetime: NaiveDateTime) -> Self {
        Self {
            year: datetime.year(),
            month: datetime.month() as u8,
            day: datetime.day() as u8,
            hour: datetime.hour() as u8,
            minute: datetime.minute() as u8,
            second: datetime.second() as u8,
            nanosecond: datetime.nanosecond(),
        }
    }
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

    fn interval(&self) -> TimeInterval {
        let finite = TemporalBound::Finite;
        match self.moment {
            TimeType::BeginningOfTime => TimeInterval {
                start: TemporalBound::NegativeInfinity,
                end: TemporalBound::NegativeInfinity,
            },
            TimeType::EndOfTime => TimeInterval {
                start: TemporalBound::PositiveInfinity,
                end: TemporalBound::PositiveInfinity,
            },
            TimeType::Year(year) => TimeInterval {
                start: finite(TemporalInstant::start_of_year(year)),
                end: year
                    .checked_add(1)
                    .map_or(TemporalBound::PositiveInfinity, |next| {
                        finite(TemporalInstant::start_of_year(next))
                    }),
            },
            TimeType::YearMonth(year, month) => {
                let (next_year, next_month) = if month == 12 {
                    (year.checked_add(1), 1)
                } else {
                    (Some(year), month + 1)
                };
                TimeInterval {
                    start: finite(TemporalInstant::start_of_month(year, month)),
                    end: next_year.map_or(TemporalBound::PositiveInfinity, |next_year| {
                        finite(TemporalInstant::start_of_month(next_year, next_month))
                    }),
                }
            }
            TimeType::Date(date) => TimeInterval {
                start: finite(TemporalInstant::from_date(date)),
                end: date
                    .succ_opt()
                    .map_or(TemporalBound::PositiveInfinity, |next| {
                        finite(TemporalInstant::from_date(next))
                    }),
            },
            TimeType::DateTime(datetime) => TimeInterval {
                start: finite(TemporalInstant::from_datetime(datetime)),
                end: datetime
                    .checked_add_signed(chrono::Duration::nanoseconds(1))
                    .map_or(TemporalBound::PositiveInfinity, |next| {
                        finite(TemporalInstant::from_datetime(next))
                    }),
            },
        }
    }

    /// Returns true when every instant represented by `self` is before every
    /// instant represented by `other`.
    pub fn definitely_before(&self, other: &Self) -> bool {
        self != other && self.interval().end <= other.interval().start
    }

    /// Returns true when every instant represented by `self` is after every
    /// instant represented by `other`.
    pub fn definitely_after(&self, other: &Self) -> bool {
        self != other && self.interval().start >= other.interval().end
    }

    /// Conservative `<=`: identical stored times compare equal; otherwise the
    /// complete interval of `self` must precede `other`.
    pub fn definitely_at_or_before(&self, other: &Self) -> bool {
        self == other || self.definitely_before(other)
    }

    /// Conservative `>=`: identical stored times compare equal; otherwise the
    /// complete interval of `self` must follow `other`.
    pub fn definitely_at_or_after(&self, other: &Self) -> bool {
        self == other || self.definitely_after(other)
    }

    /// Returns true when at least one represented instant may precede an
    /// instant represented by `other`.
    pub fn possibly_before(&self, other: &Self) -> bool {
        self.interval().start < other.interval().end
    }

    /// Returns true when at least one represented instant may follow an
    /// instant represented by `other`.
    pub fn possibly_after(&self, other: &Self) -> bool {
        self.interval().end > other.interval().start
    }

    /// Returns true when the two half-open intervals intersect.
    pub fn overlaps(&self, other: &Self) -> bool {
        let lhs = self.interval();
        let rhs = other.interval();
        lhs.start < rhs.end && rhs.start < lhs.end
    }

    /// Returns true when `self` contains the complete interval of `other`.
    pub fn contains(&self, other: &Self) -> bool {
        let lhs = self.interval();
        let rhs = other.interval();
        lhs.start <= rhs.start && lhs.end >= rhs.end
    }

    /// Returns true when `self` is contained within `other`.
    pub fn within(&self, other: &Self) -> bool {
        other.contains(self)
    }
    /// Parse a persisted canonical textual form of Time (no quotes, produced by Display).
    /// Accepted forms:
    /// BOT | EOT | YYYY | YYYY-MM | YYYY-MM-DD | YYYY-MM-DD HH:MM:SS[.fraction] | YYYY-MM-DDTHH:MM:SS[.fraction]
    #[cfg(feature = "persistence")]
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
                write!(f, "{}-{:02}", y, m)
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
