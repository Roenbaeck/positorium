//! Lossless user-visible literal values and their semantic interpretations.

use bigdecimal::BigDecimal;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LiteralFamily {
    String = 1,
    Integer = 2,
    Decimal = 3,
    Certainty = 4,
    Json = 5,
    Time = 6,
}

impl LiteralFamily {
    pub fn from_identifier(identifier: u8) -> Option<Self> {
        match identifier {
            1 => Some(Self::String),
            2 => Some(Self::Integer),
            3 => Some(Self::Decimal),
            4 => Some(Self::Certainty),
            5 => Some(Self::Json),
            6 => Some(Self::Time),
            _ => None,
        }
    }

    pub fn identifier(self) -> u8 {
        self as u8
    }
}

/// A complete, presentation-preserving UTF-8 value token.
#[derive(Debug, Clone)]
pub struct LiteralValue {
    token: String,
    family: LiteralFamily,
}

impl LiteralValue {
    pub fn new(token: impl Into<String>, family: LiteralFamily) -> Result<Self, String> {
        let token = token.into();
        validate(&token, family)?;
        Ok(Self { token, family })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn family(&self) -> LiteralFamily {
        self.family
    }

    /// Returns the exact signed percentage carried by a certainty literal.
    ///
    /// Non-certainty literals return `None`. Construction has already validated
    /// certainty tokens, so an error here indicates an internal invariant
    /// violation rather than user input that was accepted unchecked.
    pub fn certainty_percent(&self) -> Result<Option<i8>, String> {
        if self.family == LiteralFamily::Certainty {
            parse_certainty(&self.token).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Exact literal identity is byte-for-byte token equality.
    pub fn exactly_equals(&self, other: &Self) -> bool {
        self.token == other.token
    }

    /// Nominal semantic equality for beta literal families.
    pub fn nominally_equals(&self, other: &Self) -> Result<bool, String> {
        match (self.family, other.family) {
            (
                LiteralFamily::Integer | LiteralFamily::Decimal,
                LiteralFamily::Integer | LiteralFamily::Decimal,
            ) => Ok(parse_number(&self.token)? == parse_number(&other.token)?),
            (LiteralFamily::Certainty, LiteralFamily::Certainty) => {
                Ok(parse_certainty(&self.token)? == parse_certainty(&other.token)?)
            }
            (LiteralFamily::String, LiteralFamily::String) => {
                Ok(parse_json_string(&self.token)? == parse_json_string(&other.token)?)
            }
            (LiteralFamily::Json, LiteralFamily::Json) => {
                Ok(JsonParser::parse(&self.token)? == JsonParser::parse(&other.token)?)
            }
            (LiteralFamily::Time, LiteralFamily::Time) => Ok(self.token == other.token),
            _ => Err(format!(
                "unsupported nominal comparison between {:?} and {:?}",
                self.family, other.family
            )),
        }
    }

    /// Ordering is available only for exact numerics and certainty values.
    pub fn semantic_cmp(&self, other: &Self) -> Result<Ordering, String> {
        match (self.family, other.family) {
            (
                LiteralFamily::Integer | LiteralFamily::Decimal,
                LiteralFamily::Integer | LiteralFamily::Decimal,
            ) => Ok(parse_number(&self.token)?.cmp(&parse_number(&other.token)?)),
            (LiteralFamily::Certainty, LiteralFamily::Certainty) => {
                Ok(parse_certainty(&self.token)?.cmp(&parse_certainty(&other.token)?))
            }
            _ => Err(format!(
                "ordering is unsupported between {:?} and {:?}",
                self.family, other.family
            )),
        }
    }

    /// Beta literal families denote exact values, so compatibility is nominal
    /// equality. Imprecise temporal compatibility is handled by `Time`.
    pub fn possibly_equals(&self, other: &Self) -> Result<bool, String> {
        self.nominally_equals(other)
    }
}

impl PartialEq for LiteralValue {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token
    }
}

impl Eq for LiteralValue {}

impl PartialOrd for LiteralValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LiteralValue {
    fn cmp(&self, other: &Self) -> Ordering {
        self.token.cmp(&other.token)
    }
}

impl Hash for LiteralValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.token.hash(state);
    }
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.token)
    }
}

fn validate(token: &str, family: LiteralFamily) -> Result<(), String> {
    match family {
        LiteralFamily::String => parse_json_string(token).map(|_| ()),
        LiteralFamily::Integer => validate_integer(token),
        LiteralFamily::Decimal => validate_decimal(token),
        LiteralFamily::Certainty => parse_certainty(token).map(|_| ()),
        LiteralFamily::Json => JsonParser::parse(token).map(|_| ()),
        LiteralFamily::Time => {
            if token.is_empty() {
                Err("time literal token cannot be empty".into())
            } else {
                Ok(())
            }
        }
    }
}

fn validate_integer(token: &str) -> Result<(), String> {
    let digits = token.strip_prefix(['+', '-']).unwrap_or(token);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid integer literal '{token}'"));
    }
    parse_number(token).map(|_| ())
}

fn validate_decimal(token: &str) -> Result<(), String> {
    let unsigned = token.strip_prefix(['+', '-']).unwrap_or(token);
    let Some((whole, fraction)) = unsigned.split_once('.') else {
        return Err(format!("invalid decimal literal '{token}'"));
    };
    if whole.is_empty()
        || fraction.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("invalid decimal literal '{token}'"));
    }
    parse_number(token).map(|_| ())
}

fn parse_number(token: &str) -> Result<BigDecimal, String> {
    BigDecimal::from_str(token).map_err(|error| format!("invalid number '{token}': {error}"))
}

fn parse_certainty(token: &str) -> Result<i8, String> {
    let percent = token
        .strip_suffix('%')
        .ok_or_else(|| format!("certainty literal lacks percent sign: '{token}'"))?;
    let value = percent
        .parse::<i16>()
        .map_err(|error| format!("invalid certainty '{token}': {error}"))?;
    if !(-100..=100).contains(&value) {
        return Err(format!("certainty is outside -100%..=100%: '{token}'"));
    }
    Ok(value as i8)
}

fn parse_json_string(token: &str) -> Result<String, String> {
    serde_json::from_str::<String>(token)
        .map_err(|error| format!("invalid string literal '{token}': {error}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SemanticJson {
    Null,
    Bool(bool),
    Number(BigDecimal),
    String(String),
    Array(Vec<SemanticJson>),
    Object(BTreeMap<String, SemanticJson>),
}

struct JsonParser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> JsonParser<'a> {
    fn parse(source: &'a str) -> Result<SemanticJson, String> {
        let mut parser = Self { source, cursor: 0 };
        let value = parser.value()?;
        parser.whitespace();
        if parser.cursor != source.len() {
            return Err(format!("trailing JSON data at byte {}", parser.cursor));
        }
        Ok(value)
    }

    fn value(&mut self) -> Result<SemanticJson, String> {
        self.whitespace();
        match self.peek() {
            Some(b'n') => {
                self.keyword("null")?;
                Ok(SemanticJson::Null)
            }
            Some(b't') => {
                self.keyword("true")?;
                Ok(SemanticJson::Bool(true))
            }
            Some(b'f') => {
                self.keyword("false")?;
                Ok(SemanticJson::Bool(false))
            }
            Some(b'"') => self.string().map(SemanticJson::String),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.number().map(SemanticJson::Number),
            Some(other) => Err(format!(
                "invalid JSON byte '{}' at {}",
                char::from(other),
                self.cursor
            )),
            None => Err("empty or truncated JSON".into()),
        }
    }

    fn array(&mut self) -> Result<SemanticJson, String> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.consume(b']') {
            return Ok(SemanticJson::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.whitespace();
            if self.consume(b']') {
                return Ok(SemanticJson::Array(values));
            }
            self.expect(b',')?;
        }
    }

    fn object(&mut self) -> Result<SemanticJson, String> {
        self.expect(b'{')?;
        let mut members = BTreeMap::new();
        self.whitespace();
        if self.consume(b'}') {
            return Ok(SemanticJson::Object(members));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            self.whitespace();
            self.expect(b':')?;
            let value = self.value()?;
            if members.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate JSON object key '{key}'"));
            }
            self.whitespace();
            if self.consume(b'}') {
                return Ok(SemanticJson::Object(members));
            }
            self.expect(b',')?;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let start = self.cursor;
        self.expect(b'"')?;
        let mut escaped = false;
        while let Some(byte) = self.peek() {
            self.cursor += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return serde_json::from_str::<String>(&self.source[start..self.cursor])
                    .map_err(|error| format!("invalid JSON string at byte {start}: {error}"));
            } else if byte < 0x20 {
                return Err(format!("unescaped JSON control byte at {start}"));
            }
        }
        Err(format!("unterminated JSON string at byte {start}"))
    }

    fn number(&mut self) -> Result<BigDecimal, String> {
        let start = self.cursor;
        self.consume(b'-');
        match self.peek() {
            Some(b'0') => {
                self.cursor += 1;
                if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    return Err(format!("leading zero in JSON number at byte {start}"));
                }
            }
            Some(b'1'..=b'9') => {
                self.cursor += 1;
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.cursor += 1;
                }
            }
            _ => return Err(format!("invalid JSON number at byte {start}")),
        }
        if self.consume(b'.') {
            let fraction_start = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            if self.cursor == fraction_start {
                return Err(format!("missing JSON fraction at byte {start}"));
            }
        }
        if self.consume(b'e') || self.consume(b'E') {
            let _ = self.consume(b'+') || self.consume(b'-');
            let exponent_start = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            if self.cursor == exponent_start {
                return Err(format!("missing JSON exponent at byte {start}"));
            }
        }
        let token = &self.source[start..self.cursor];
        BigDecimal::from_str(token)
            .map_err(|error| format!("invalid JSON number '{token}': {error}"))
    }

    fn keyword(&mut self, keyword: &str) -> Result<(), String> {
        if self.source[self.cursor..].starts_with(keyword) {
            self.cursor += keyword.len();
            Ok(())
        } else {
            Err(format!("invalid JSON token at byte {}", self.cursor))
        }
    }

    fn whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.cursor += 1;
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(format!(
                "expected '{}' at JSON byte {}",
                char::from(expected),
                self.cursor
            ))
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.cursor).copied()
    }
}
