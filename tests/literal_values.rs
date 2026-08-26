use std::cmp::Ordering;

use positorium::literal::{LiteralFamily, LiteralValue};

fn literal(token: &str, family: LiteralFamily) -> LiteralValue {
    LiteralValue::new(token, family).unwrap()
}

#[test]
fn exact_identity_preserves_numeric_presentation() {
    let canonical = literal("10", LiteralFamily::Integer);
    for spelling in ["010", "+10", "10.0", "10.00"] {
        let family = if spelling.contains('.') {
            LiteralFamily::Decimal
        } else {
            LiteralFamily::Integer
        };
        let presented = literal(spelling, family);
        assert_ne!(canonical, presented, "{spelling} is a distinct literal");
        assert!(canonical.nominally_equals(&presented).unwrap());
    }
}

#[test]
fn string_identity_preserves_escape_spelling_without_normalizing_unicode() {
    let plain = literal(r#""Aé""#, LiteralFamily::String);
    let escaped = literal(r#""\u0041\u00e9""#, LiteralFamily::String);
    let decomposed = literal(r#""Ae\u0301""#, LiteralFamily::String);

    assert_ne!(plain, escaped);
    assert!(plain.nominally_equals(&escaped).unwrap());
    assert!(!plain.nominally_equals(&decomposed).unwrap());
}

#[test]
fn json_nominal_equality_is_structural_and_exact() {
    let left = literal(r#"{"a": 1.00, "b": [true, null]}"#, LiteralFamily::Json);
    let reordered = literal(r#"{ "b" : [true,null], "a" : 1 }"#, LiteralFamily::Json);
    let array_changed = literal(r#"{"a": 1, "b": [null, true]}"#, LiteralFamily::Json);

    assert_ne!(left, reordered);
    assert!(left.nominally_equals(&reordered).unwrap());
    assert!(!left.nominally_equals(&array_changed).unwrap());
}

#[test]
fn duplicate_json_keys_are_rejected_after_escape_decoding() {
    let error =
        LiteralValue::new(r#"{"name": 1, "\u006eame": 2}"#, LiteralFamily::Json).unwrap_err();
    assert!(error.contains("duplicate JSON object key"), "{error}");
}

#[test]
fn certainty_semantics_preserve_spelling_and_compare_exactly() {
    let plain = literal("75%", LiteralFamily::Certainty);
    let padded = literal("075%", LiteralFamily::Certainty);
    let lower = literal("60%", LiteralFamily::Certainty);

    assert_ne!(plain, padded);
    assert!(plain.nominally_equals(&padded).unwrap());
    assert_eq!(lower.semantic_cmp(&plain).unwrap(), Ordering::Less);
    assert!(LiteralValue::new("101%", LiteralFamily::Certainty).is_err());
}

#[test]
fn unsupported_cross_family_and_string_ordering_are_typed_failures() {
    let string = literal(r#""10""#, LiteralFamily::String);
    let integer = literal("10", LiteralFamily::Integer);

    assert!(string.nominally_equals(&integer).is_err());
    assert!(string.semantic_cmp(&string).is_err());
}
