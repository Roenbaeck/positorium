use super::{JsonParser, LiteralFamily, LiteralValue};
use serde::de::IgnoredAny;

/// An RFC 6901 string pointer. Decode once when preparing a query.
#[derive(Debug, Clone)]
pub(crate) struct JsonPointer(Vec<String>);

impl JsonPointer {
    pub(crate) fn parse(path: &str) -> Result<Self, String> {
        if path.is_empty() {
            return Ok(Self(Vec::new()));
        }
        let path = path
            .strip_prefix('/')
            .ok_or("JSON Pointer must be empty or start with '/'")?;
        let mut tokens = Vec::new();
        for segment in path.split('/') {
            let mut token = String::with_capacity(segment.len());
            let mut chars = segment.chars();
            while let Some(ch) = chars.next() {
                token.push(match ch {
                    '~' => match chars.next() {
                        Some('0') => '~',
                        Some('1') => '/',
                        _ => return Err("invalid JSON Pointer escape, expected ~0 or ~1".into()),
                    },
                    other => other,
                });
            }
            tokens.push(token);
        }
        Ok(Self(tokens))
    }

    fn locate<'a>(&self, document: &'a LiteralValue) -> Result<Option<JsonParser<'a>>, String> {
        if document.family() != LiteralFamily::Json {
            return Ok(None);
        }
        let mut json = JsonParser {
            source: document.token(),
            cursor: 0,
        };
        json.whitespace();
        for token in &self.0 {
            if !json.seek_member(token)? {
                return Ok(None);
            }
        }
        Ok(Some(json))
    }

    pub(crate) fn exists(&self, document: &LiteralValue) -> Result<bool, String> {
        Ok(self.locate(document)?.is_some())
    }

    pub(crate) fn at<'a>(&self, document: &'a LiteralValue) -> Result<Option<&'a str>, String> {
        if self.0.is_empty() {
            return Ok((document.family() == LiteralFamily::Json).then(|| document.token()));
        }
        let Some(mut json) = self.locate(document)? else {
            return Ok(None);
        };
        let start = json.cursor;
        json.skip_value()?;
        Ok(Some(&json.source[start..json.cursor]))
    }
}

impl LiteralValue {
    /// The containing document has already validated every nested key and number.
    pub(crate) fn from_json_fragment(token: &str) -> Self {
        let trimmed = token.trim();
        let family = match trimmed.as_bytes()[0] {
            b'"' => LiteralFamily::String,
            b'-' | b'0'..=b'9' if token.contains(['.', 'e', 'E']) => LiteralFamily::Decimal,
            b'-' | b'0'..=b'9' => LiteralFamily::Integer,
            _ => LiteralFamily::Json,
        };
        Self {
            token: if family == LiteralFamily::Json {
                token
            } else {
                trimmed
            }
            .to_owned(),
            family,
        }
    }
}

impl JsonParser<'_> {
    /// Walk forward in a validated document. Skip unrelated subtrees without
    /// building them or converting their numbers to floating point.
    fn skip_value(&mut self) -> Result<(), String> {
        let mut stream = serde_json::Deserializer::from_str(&self.source[self.cursor..])
            .into_iter::<IgnoredAny>();
        stream
            .next()
            .ok_or("missing JSON value")?
            .map_err(|e| e.to_string())?;
        self.cursor += stream.byte_offset();
        Ok(())
    }

    fn seek_member(&mut self, token: &str) -> Result<bool, String> {
        let index = match self.peek() {
            Some(b'{') => None,
            Some(b'[') => {
                if token.starts_with(['+', '-']) || (token.starts_with('0') && token.len() > 1) {
                    return Ok(false);
                }
                let Ok(index) = token.parse::<usize>() else {
                    return Ok(false);
                };
                Some(index)
            }
            _ => return Ok(false),
        };
        self.cursor += 1;
        for position in 0.. {
            self.whitespace();
            if matches!(self.peek(), Some(b'}' | b']')) {
                break;
            }
            let matches = if let Some(index) = index {
                position == index
            } else {
                let key = self.string()?;
                self.whitespace();
                self.expect(b':')?;
                self.whitespace();
                key == token
            };
            if matches {
                return Ok(true);
            }
            self.skip_value()?;
            self.whitespace();
            if !self.consume(b',') {
                break;
            }
        }
        Ok(false)
    }
}
