use positorium::construct::{Appearance, AppearanceSet, Posit, Role, ThingGenerator};
use positorium::construct::{Database, PersistenceMode};
use positorium::datatype::{Certainty, Time};
use positorium::traqula::Engine;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

fn hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn role_identity_controls_equality_hashing_and_ordering() {
    let first = Role::new(42, "first name".to_string(), false);
    let renamed_metadata = Role::new(42, "other name".to_string(), true);
    let other_identity = Role::new(43, "first name".to_string(), false);

    assert_eq!(first, renamed_metadata);
    assert_eq!(first.cmp(&renamed_metadata), std::cmp::Ordering::Equal);
    assert_eq!(hash(&first), hash(&renamed_metadata));
    assert_ne!(first, other_identity);
}

#[test]
fn posit_identity_is_not_part_of_proposition_equality_or_ordering() {
    let role = Arc::new(Role::new(7, "name".to_string(), false));
    let appearance = Arc::new(Appearance::new(11, role));
    let appearance_set = Arc::new(AppearanceSet::new(vec![appearance]).unwrap());
    let time = Time::new_date_from("2024-05-06").unwrap();
    let first = Posit::new(
        100,
        Arc::clone(&appearance_set),
        "Alice".to_string(),
        time.clone(),
    );
    let duplicate = Posit::new(101, appearance_set, "Alice".to_string(), time);

    assert_eq!(first, duplicate);
    assert_eq!(first.cmp(&duplicate), std::cmp::Ordering::Equal);
    assert_eq!(hash(&first), hash(&duplicate));
}

#[test]
fn mixed_precision_time_ordering_obeys_eq_ord_laws() {
    let year = Time::new_year_from("2024").unwrap();
    let date = Time::new_date_from("2024-05-06").unwrap();

    assert_ne!(year, date);
    assert_ne!(year.cmp(&date), std::cmp::Ordering::Equal);
    assert_eq!(year.partial_cmp(&date), Some(year.cmp(&date)));
}

#[test]
fn temporal_relations_use_half_open_precision_intervals() {
    let year = Time::new_year_from("2024").unwrap();
    let date = Time::new_date_from("2024-05-06").unwrap();
    let next_year = Time::new_year_from("2025").unwrap();
    let beginning = Time::new_beginning_of_time();
    let end = Time::new_end_of_time();

    assert!(year.overlaps(&date));
    assert!(year.contains(&date));
    assert!(date.within(&year));
    assert!(!year.definitely_before(&date));
    assert!(!date.definitely_after(&year));
    assert!(year.definitely_before(&next_year));
    assert!(year.definitely_at_or_before(&year));
    assert!(beginning.definitely_before(&year));
    assert!(end.definitely_after(&year));
    assert!(!beginning.definitely_before(&beginning));
    assert!(!end.definitely_after(&end));
}

#[test]
fn snapshots_preserve_incomparable_and_equal_time_conflicts() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let result = engine
        .execute_collect(
            "add role state; add posit [{(+subject, state)}, \"broad\", '2024']; add posit [{(subject, state)}, \"specific-a\", '2024-05-06']; add posit [{(subject, state)}, \"specific-b\", '2024-05-06']; search [{(*, state)}, +value, *] as of @EOT return value;",
        )
        .expect("snapshot query");

    let mut values: Vec<&str> = result.rows.iter().map(|row| row[0].as_str()).collect();
    values.sort_unstable();
    assert_eq!(
        values,
        vec!["\"broad\"", "\"specific-a\"", "\"specific-b\""]
    );
}

#[test]
fn role_catalog_normalizes_nfc_and_keeps_names_case_sensitive() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&db)
        .execute("add role cafe\u{301}; add role caf\u{e9}; add role CAF\u{c9};")
        .unwrap();
    assert_eq!(db.role_count().unwrap(), 4);
    assert!(db.contains_role("cafe\u{301}").unwrap());
    assert!(db.contains_role("caf\u{e9}").unwrap());
    assert!(db.contains_role("CAF\u{c9}").unwrap());
}

#[test]
fn thing_identities_are_never_recycled() {
    let mut generator = ThingGenerator::new();
    let abandoned = generator.generate();
    generator.release(abandoned);
    assert!(generator.generate() > abandoned);
}

#[test]
fn accepted_literal_display_forms_are_canonical() {
    assert_eq!(Certainty::new(0.05).to_string(), "0.05");
    assert_eq!(Certainty::new(-0.05).to_string(), "-0.05");
    assert_eq!(
        Time::new_year_month_from("2024-05").unwrap().to_string(),
        "2024-05"
    );
}

#[test]
fn invalid_public_time_inputs_return_errors() {
    assert!(Time::new_year_from("not-a-year").is_err());
    assert!(Time::new_year_month_from("2024-00").is_err());
    assert!(Time::new_year_month_from("2024-13").is_err());
    assert!(Time::new_date_from("2024-02-30").is_err());
    assert!(Time::new_datetime_from("2024-02-30T25:00:00").is_err());
}

#[test]
fn certainty_boundaries_and_consistency_follow_the_signed_scale() {
    assert_eq!(Certainty::from_percent(-128).percent(), -100);
    assert_eq!(Certainty::from_percent(127).percent(), 100);
    assert_eq!(Certainty::new(-2.0).percent(), -100);
    assert_eq!(Certainty::new(2.0).percent(), 100);

    assert!(Certainty::consistent(&[]));
    assert!(Certainty::consistent(&[
        Certainty::from_percent(60),
        Certainty::from_percent(40),
    ]));
    assert!(!Certainty::consistent(&[
        Certainty::from_percent(60),
        Certainty::from_percent(41),
    ]));
    assert!(Certainty::consistent(&[
        Certainty::from_percent(25),
        Certainty::from_percent(-75),
    ]));
    assert!(Certainty::consistent(&vec![
        Certainty::from_percent(-95);
        20
    ]));
    assert!(!Certainty::consistent(&vec![
        Certainty::from_percent(-95);
        21
    ]));
}

#[test]
fn only_assertion_roles_are_reserved_by_default() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    assert_eq!(db.role_count().unwrap(), 2);
    assert!(db.contains_role("posit").unwrap());
    assert!(db.contains_role("ascertains").unwrap());
    assert!(!db.contains_role("thing").unwrap());
    Engine::new(&db).execute("add role thing;").unwrap();
    assert_eq!(db.role_count().unwrap(), 3);
}

#[test]
fn invalid_appearance_sets_and_unknown_roles_return_errors() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let duplicate_role = engine
        .execute_collect(
            "add role member; add posit [{(+first, member), (+second, member)}, \"value\", @NOW];",
        )
        .unwrap_err();
    assert!(duplicate_role.to_string().contains("at most one Thing"));
    let error = engine
        .execute_collect("add posit [{(+x, missing)}, \"value\", @NOW];")
        .unwrap_err();
    assert!(error.to_string().contains("Unknown role: missing"));
}
