use positorium::{Database, Engine, PersistenceMode, TerrainOptions};

const TERRAIN_FIXTURE: &str = include_str!("../traqula/terrain.traqula");

#[test]
fn terrain_fixture_produces_the_exact_authoritative_report() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let result_sets = Engine::new(&database)
        .execute_collect_multi(TERRAIN_FIXTURE)
        .unwrap();
    assert!(result_sets.is_empty(), "fixture is population-only");

    let options = TerrainOptions {
        as_of: Some(positorium::datatype::Time::new_date_from("2026-01-01").unwrap()),
        ..TerrainOptions::default()
    };
    let report = database.terrain_with_options(options.clone()).unwrap();
    assert_eq!(report, database.terrain_with_options(options).unwrap());
    assert_eq!(report.terrain_version, 1);
    assert_eq!(report.resolved_as_of, "'2026-01-01'");
    assert_eq!(
        (
            report.database.referenced_things,
            report.database.roles,
            report.database.appearances,
            report.database.appearance_sets,
            report.database.posits,
        ),
        (45, 13, 25, 24, 26)
    );

    assert_eq!(
        report
            .projection
            .roles
            .iter()
            .map(|role| (
                role.name.as_str(),
                role.history_support,
                role.current_support,
                role.bit
            ))
            .collect::<Vec<_>>(),
        [
            ("hair color", 6, 6, 0),
            ("name", 6, 6, 1),
            ("height", 3, 3, 2),
            ("social security number", 3, 3, 3),
            ("RFID", 2, 2, 4),
            ("beard color", 1, 1, 5),
        ]
    );
    assert!(report.projection.complete);

    let history = &report.frames.history;
    let current = &report.frames.current;
    assert_eq!(
        (
            history.stats.endpoint_things,
            history.stats.roles,
            history.stats.appearance_sets,
            history.stats.posits,
            history.stats.incidences,
        ),
        (6, 8, 24, 26, 30)
    );
    assert_eq!(
        (
            current.stats.endpoint_things,
            current.stats.roles,
            current.stats.appearance_sets,
            current.stats.posits,
            current.stats.incidences,
        ),
        (6, 8, 24, 24, 27)
    );
    assert_eq!(
        history
            .profiles
            .iter()
            .map(|profile| (profile.mask, profile.things))
            .collect::<Vec<_>>(),
        [(3, 1), (15, 2), (19, 2), (47, 1)]
    );
    assert_eq!(
        history
            .isopleths
            .iter()
            .map(|isopleth| (isopleth.mask, isopleth.support))
            .collect::<Vec<_>>(),
        [(3, 6), (19, 2), (15, 3), (47, 1)]
    );

    let signature = &report.relationship_catalog.signatures[0];
    assert_eq!(
        signature
            .roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>(),
        ["owner", "pet"]
    );
    assert_eq!(
        report.relationship_catalog.default_signature_id.as_deref(),
        Some(signature.id.as_str())
    );
    let history_relationship = &history.relationships[0];
    let current_relationship = &current.relationships[0];
    assert_eq!(
        (
            history_relationship.appearance_sets,
            history_relationship.posits
        ),
        (3, 4)
    );
    assert_eq!(
        (
            current_relationship.appearance_sets,
            current_relationship.posits
        ),
        (3, 3)
    );
    assert_eq!(
        history_relationship
            .role_totals
            .iter()
            .map(|total| (total.distinct_things, total.participations))
            .collect::<Vec<_>>(),
        [(2, 3), (2, 3)]
    );
    assert_eq!(
        history_relationship
            .allocations
            .iter()
            .map(|allocation| (
                allocation.profile_mask,
                allocation.distinct_things,
                allocation.participations,
            ))
            .collect::<Vec<_>>(),
        [(15, 1, 1), (47, 1, 2), (19, 2, 3)]
    );
}
