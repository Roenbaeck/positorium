use positorium::datatype::Time;
use positorium::{
    CancellationToken, Database, DatabaseError, Engine, PersistenceMode, QueryInterface,
    QueryOptions, TerrainOptions,
};
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn date(value: &str) -> Time {
    Time::new_date_from(value).unwrap()
}

fn report_at(database: &Database, cutoff: Time) -> positorium::TerrainReport {
    database
        .terrain_with_options(TerrainOptions {
            as_of: Some(cutoff),
            ..TerrainOptions::default()
        })
        .unwrap()
}

#[test]
fn current_frontier_keeps_equal_and_incomparable_times_without_identity_ties() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role attribute; \
             add posit [{(+equal, attribute)}, \"left\", '2024-01-01'], \
                       [{(equal, attribute)}, \"right\", '2024-01-01'], \
                       [{(+mixed, attribute)}, \"year\", '2024'], \
                       [{(mixed, attribute)}, \"day\", '2024-06-01'], \
                       [{(+ordered, attribute)}, \"old\", '2023'], \
                       [{(ordered, attribute)}, \"new\", '2024'];",
        )
        .unwrap();
    let report = report_at(&database, date("2025-01-01"));
    assert_eq!(report.frames.history.stats.posits, 6);
    assert_eq!(report.frames.current.stats.posits, 5);
    assert_eq!(report.frames.current.stats.appearance_sets, 3);
    assert_eq!(report.frames.current.stats.incidences, 5);
}

#[test]
fn bot_finite_and_eot_cutoffs_are_resolved_exactly() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role attribute; \
             add posit [{(+bottom, attribute)}, 0, @BOT], \
                       [{(+finite, attribute)}, 1, '2024-01-01'], \
                       [{(+end, attribute)}, 2, @EOT];",
        )
        .unwrap();
    let bottom = report_at(&database, Time::new_beginning_of_time());
    assert_eq!(bottom.resolved_as_of, "@BOT");
    assert_eq!(bottom.frames.current.stats.posits, 1);
    let finite = report_at(&database, date("2024-01-01"));
    assert_eq!(finite.frames.current.stats.posits, 2);
    let end = report_at(&database, Time::new_end_of_time());
    assert_eq!(end.resolved_as_of, "@EOT");
    assert_eq!(end.frames.current.stats.posits, 3);
}

#[test]
fn relationship_only_endpoints_have_mask_zero_allocations() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role name, owner, pet; \
             add posit [{(+named, name)}, \"Named\", '2024-01-01'], \
                       [{(+relation_only, owner), (named, pet)}, \"linked\", '2024-01-01'];",
        )
        .unwrap();
    let report = report_at(&database, date("2025-01-01"));
    let frame = &report.frames.current;
    assert_eq!(
        frame
            .profiles
            .iter()
            .map(|profile| profile.things)
            .sum::<u64>(),
        2
    );
    let zero = frame
        .profiles
        .iter()
        .find(|profile| profile.mask == 0)
        .unwrap();
    assert_eq!(zero.things, 1);
    assert_eq!(zero.isopleth_id, None);
    assert!(frame.isopleths.iter().all(|isopleth| isopleth.mask != 0));
    let allocations = &frame.relationships[0].allocations;
    let zero_allocation = allocations
        .iter()
        .find(|allocation| allocation.profile_mask == 0)
        .unwrap();
    assert_eq!(zero_allocation.isopleth_id, None);
    for total in &frame.relationships[0].role_totals {
        assert_eq!(
            allocations
                .iter()
                .filter(|allocation| allocation.role_id == total.role_id)
                .map(|allocation| allocation.participations)
                .sum::<u64>(),
            total.participations
        );
        assert_eq!(
            allocations
                .iter()
                .filter(|allocation| allocation.role_id == total.role_id)
                .map(|allocation| allocation.distinct_things)
                .sum::<u64>(),
            total.distinct_things
        );
    }
}

#[test]
fn profiles_and_isopleths_use_exact_distinct_support() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role a, b, unused, owner, pet; \
             add posit [{(+x, a)}, 1, '2023-01-01'], \
                       [{(x, a)}, 2, '2024-01-01'], \
                       [{(x, b)}, 1, '2024-01-01'], \
                       [{(+y, a)}, 1, '2024-01-01'], \
                       [{(+relation_only, owner), (x, pet)}, 1, '2024-01-01'];",
        )
        .unwrap();
    let report = report_at(&database, date("2025-01-01"));
    assert_eq!(report.database.roles, 10, "unused and built-in Roles count");
    assert_eq!(report.frames.current.stats.roles, 4);
    assert_eq!(
        report
            .projection
            .roles
            .iter()
            .map(|role| (role.name.as_str(), role.current_support))
            .collect::<Vec<_>>(),
        [("a", 2), ("b", 1)]
    );
    assert_eq!(
        report
            .frames
            .history
            .role_supports
            .iter()
            .map(|support| support.distinct_things)
            .collect::<Vec<_>>(),
        [2, 1],
        "repeated history for x/a must not multiply support"
    );

    for frame in [&report.frames.history, &report.frames.current] {
        assert_eq!(
            frame
                .profiles
                .iter()
                .map(|profile| profile.things)
                .sum::<u64>(),
            frame.stats.endpoint_things
        );
        assert_eq!(
            frame
                .profiles
                .iter()
                .map(|profile| (profile.mask, profile.things))
                .collect::<Vec<_>>(),
            [(0, 1), (1, 1), (3, 1)]
        );
        assert_eq!(
            frame
                .isopleths
                .iter()
                .map(|isopleth| (isopleth.mask, isopleth.support))
                .collect::<Vec<_>>(),
            [(1, 2), (3, 1)]
        );
        assert!(frame.isopleths[1].support <= frame.isopleths[0].support);
        assert!(frame.profiles[0].isopleth_id.is_none());
    }
}

#[test]
fn binary_and_nary_signatures_share_a_deterministic_catalog() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role left, right, first, second, third; \
             add posit [{(+l, left), (+r, right)}, 1, '2024-01-01'], \
                       [{(l, left), (r, right)}, 2, '2025-01-01'], \
                       [{(+one, first), (+two, second), (+three, third)}, 1, '2030-01-01'];",
        )
        .unwrap();
    let report = report_at(&database, date("2026-01-01"));
    assert_eq!(report.relationship_catalog.total_signatures, 2);
    assert!(report.relationship_catalog.complete);
    assert_eq!(
        report.relationship_catalog.default_signature_id,
        Some(report.relationship_catalog.signatures[0].id.clone())
    );
    assert_eq!(
        report.relationship_catalog.signatures[0]
            .roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>(),
        ["left", "right"]
    );
    assert_eq!(report.relationship_catalog.signatures[1].roles.len(), 3);

    let history = &report.frames.history.relationships;
    let current = &report.frames.current.relationships;
    assert_eq!(history.len(), 2);
    assert_eq!(current.len(), 2);
    assert_eq!((history[0].appearance_sets, history[0].posits), (1, 2));
    assert_eq!((current[0].appearance_sets, current[0].posits), (1, 1));
    assert_eq!(history[0].role_totals[0].participations, 1);
    assert_eq!((history[1].appearance_sets, history[1].posits), (1, 1));
    assert_eq!((current[1].appearance_sets, current[1].posits), (0, 0));
    assert!(current[1].allocations.is_empty());
    assert!(
        current[1]
            .role_totals
            .iter()
            .all(|total| total.distinct_things == 0 && total.participations == 0)
    );
}

#[test]
fn normalized_and_quoted_role_names_keep_backend_identity() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role `alpha, beta`, `spaced role`, e\u{301}; \
             add posit [{(+x, `alpha, beta`)}, 1, '2024-01-01'], \
                       [{(x, `spaced role`)}, 1, '2024-01-01'], \
                       [{(x, é)}, 1, '2024-01-01'];",
        )
        .unwrap();
    let report = report_at(&database, date("2025-01-01"));
    let names_and_ids = report
        .projection
        .roles
        .iter()
        .map(|role| (role.name.as_str(), role.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        names_and_ids
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["alpha, beta", "spaced role", "é"]
    );
    assert_eq!(
        names_and_ids
            .iter()
            .map(|(_, id)| *id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
}

#[test]
fn projection_and_catalog_truncation_are_deterministic_and_shared() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role a, b, c, d, e, f, g, h, i, owner, pet, source, target; \
             add posit [{(+x, a)}, 1, '2024-01-01'], [{(x, b)}, 1, '2024-01-01'], \
                       [{(x, c)}, 1, '2024-01-01'], [{(x, d)}, 1, '2024-01-01'], \
                       [{(x, e)}, 1, '2024-01-01'], [{(x, f)}, 1, '2024-01-01'], \
                       [{(x, g)}, 1, '2024-01-01'], [{(x, h)}, 1, '2024-01-01'], \
                       [{(x, i)}, 1, '2024-01-01'], \
                       [{(+o, owner), (+p, pet)}, 1, '2024-01-01'], \
                       [{(+s, source), (+t, target)}, 1, '2030-01-01'];",
        )
        .unwrap();
    let report = database
        .terrain_with_options(TerrainOptions {
            as_of: Some(date("2025-01-01")),
            max_relationship_signatures: 1,
            ..TerrainOptions::default()
        })
        .unwrap();
    assert!(!report.projection.complete);
    assert_eq!(report.projection.total_attribute_roles, 9);
    assert_eq!(report.projection.roles.len(), 8);
    assert!(!report.relationship_catalog.complete);
    assert_eq!(report.relationship_catalog.total_signatures, 2);
    assert_eq!(report.relationship_catalog.signatures.len(), 1);
    assert_eq!(report.frames.history.relationships.len(), 1);
    assert_eq!(report.frames.current.relationships.len(), 1);
    assert_eq!(
        report.frames.history.relationships[0].signature_id,
        report.frames.current.relationships[0].signature_id
    );
    assert_eq!(
        report
            .frames
            .history
            .profiles
            .iter()
            .map(|profile| profile.things)
            .sum::<u64>(),
        report.frames.history.stats.endpoint_things
    );
}

#[test]
fn terrain_enforces_timeout_and_cancellation() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let error = database
        .terrain_with_options(TerrainOptions {
            timeout: Some(Duration::ZERO),
            ..TerrainOptions::default()
        })
        .unwrap_err();
    assert!(matches!(error, DatabaseError::Timeout));

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = database
        .terrain_with_options(TerrainOptions {
            cancellation: Some(cancellation),
            ..TerrainOptions::default()
        })
        .unwrap_err();
    assert!(matches!(error, DatabaseError::Cancelled));
}

#[test]
fn separate_query_interfaces_yield_only_old_or_new_atomic_captures() {
    for _ in 0..24 {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        let writer = QueryInterface::new(Arc::clone(&database));
        let reader = QueryInterface::new(Arc::clone(&database));
        let barrier = Arc::new(Barrier::new(3));
        let writer_barrier = Arc::clone(&barrier);
        let writer = std::thread::spawn(move || {
            writer_barrier.wait();
            writer
                .start_query(
                    "add role a; \
                     add posit [{(+x, a)}, 1, '2024-01-01'], \
                               [{(+y, a)}, 1, '2024-01-01'];"
                        .into(),
                    QueryOptions {
                        stream_results: false,
                        timeout: Some(Duration::from_secs(1)),
                    },
                )
                .unwrap()
                .join()
                .unwrap();
        });
        let reader_barrier = Arc::clone(&barrier);
        let reader = std::thread::spawn(move || {
            reader_barrier.wait();
            reader
                .terrain_with_options(TerrainOptions {
                    as_of: Some(date("2025-01-01")),
                    timeout: Some(Duration::from_secs(1)),
                    ..TerrainOptions::default()
                })
                .unwrap()
        });
        barrier.wait();
        writer.join().unwrap();
        let report = reader.join().unwrap();
        assert!(
            matches!(
                (report.database.roles, report.database.posits),
                (5, 0) | (6, 2)
            ),
            "Terrain observed a mixed writer state: {:?}",
            (report.database.roles, report.database.posits)
        );
        assert!(matches!(report.frames.history.stats.posits, 0 | 2));
    }
}

#[cfg(feature = "persistence")]
#[test]
fn in_memory_and_replayed_reports_are_byte_equivalent() {
    let script = "add role name, owner, pet; \
                  add posit [{(+a, name)}, \"Ada\", '2024-01-01'], \
                            [{(+p, name)}, \"Pixel\", '2024-01-01'], \
                            [{(a, owner), (p, pet)}, \"adopted\", '2024-02-01'];";
    let memory = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&memory).execute(script).unwrap();

    let directory = std::env::temp_dir().join(format!(
        "positorium-terrain-replay-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let stored = Database::new(PersistenceMode::File(
        directory.to_string_lossy().into_owned(),
    ))
    .unwrap();
    Engine::new(&stored).execute(script).unwrap();
    stored.flush().unwrap();
    drop(stored);
    let replayed = Database::new(PersistenceMode::File(
        directory.to_string_lossy().into_owned(),
    ))
    .unwrap();

    let cutoff = date("2025-01-01");
    let memory_report = report_at(&memory, cutoff.clone());
    let replayed_report = report_at(&replayed, cutoff);
    assert_eq!(
        serde_json::to_vec(&memory_report).unwrap(),
        serde_json::to_vec(&replayed_report).unwrap()
    );
    drop(replayed);
    std::fs::remove_dir_all(directory).unwrap();
}
