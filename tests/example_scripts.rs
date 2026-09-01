use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;
use std::fs;
use std::path::Path;

fn source(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("traqula")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

#[test]
fn standalone_example_scripts_execute() {
    for name in [
        "welcome.traqula",
        "blackthorn.traqula",
        "cross-search.traqula",
        "multi-match.traqula",
        "timetest.traqula",
    ] {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute_collect_multi(&source(name))
            .unwrap_or_else(|error| panic!("execute {name}: {error}"));
    }
}

#[test]
fn welcome_example_preserves_conflict_correction_and_history() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let script = source("welcome.traqula");
    let results = engine
        .execute_collect_multi(&script)
        .expect("execute welcome.traqula");

    assert_eq!(results.len(), 3);
    assert_eq!(
        results
            .iter()
            .map(|result| result.row_count)
            .collect::<Vec<_>>(),
        [2, 2, 4]
    );
    assert_eq!(results[0].rows[0][1].as_str(), "\"high risk\"");
    assert_eq!(results[0].rows[1][1].as_str(), "\"needs review\"");
    assert_eq!(results[1].rows[1][1].as_str(), "\"low risk\"");
    assert_eq!(results[2].rows[2][2].as_str(), "0%");

    engine
        .execute_collect_multi(&script)
        .expect("rerun idempotent welcome.traqula");
    let rerun = engine
        .execute_collect_multi(&script)
        .expect("collect welcome.traqula after rerun");
    assert_eq!(
        rerun
            .iter()
            .map(|result| result.row_count)
            .collect::<Vec<_>>(),
        [2, 2, 4]
    );
}

#[test]
fn blackthorn_detective_story_reaches_supported_conclusion() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let script = source("blackthorn.traqula");
    let results = engine
        .execute_collect_multi(&script)
        .expect("execute blackthorn.traqula");

    assert_eq!(results.len(), 15, "one result set per narrated search");
    assert_eq!(
        results
            .iter()
            .map(|result| result.row_count)
            .collect::<Vec<_>>(),
        [5, 10, 4, 5, 7, 5, 2, 1, 1, 1, 2, 1, 2, 1, 3]
    );
    assert_eq!(
        results[6]
            .rows
            .iter()
            .map(|row| row[0].as_str())
            .collect::<Vec<_>>(),
        ["\"Beatrice Shaw\"", "\"Lydia Marr\""]
    );
    assert_eq!(results[7].rows[0][0].as_str(), "\"Jonah Pike\"");
    assert_eq!(results[7].rows[0][1].as_str(), "\"Master Card JP-1\"");
    assert_eq!(results[8].rows[0][0].as_str(), "\"Jonah Pike\"");
    assert_eq!(results[8].rows[0][1].as_str(), "44");
    assert_eq!(results[9].rows[0][0].as_str(), "\"Jonah Pike\"");
    assert_eq!(results[9].rows[0][1].as_str(), "\"Blackthorn red wool\"");
    assert_eq!(results[11].rows[0][1].as_str(), "\"Jonah Pike\"");
    assert_eq!(results[11].rows[0][2].as_str(), "44");
    assert_eq!(results[13].rows[0][0].as_str(), "\"Jonah Pike\"");
    assert_eq!(results[13].rows[0][2].as_str(), "96%");
    assert_eq!(
        results[14].rows[2][0].as_str(),
        "\"closed — Jonah Pike charged\""
    );

    let report = database
        .terrain_with_options(positorium::TerrainOptions {
            as_of: Some(
                positorium::datatype::Time::new_date_from("2026-01-01").expect("fixed cutoff"),
            ),
            ..positorium::TerrainOptions::default()
        })
        .expect("blackthorn terrain");
    assert!(report.projection.complete);
    assert_eq!(report.projection.total_attribute_roles, 8);
    assert_eq!(
        report
            .projection
            .roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>(),
        [
            "dossier_id",
            "name",
            "occupation",
            "class",
            "description",
            "source type",
            "shoe size",
            "status",
        ]
    );
    let signatures = report
        .relationship_catalog
        .signatures
        .iter()
        .map(|signature| {
            signature
                .roles
                .iter()
                .map(|role| role.name.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for expected in [
        vec!["thing", "class"],
        vec!["suspect", "case"],
        vec!["credential", "holder"],
        vec!["credential", "access point"],
        vec!["trace", "location"],
    ] {
        assert!(signatures.contains(&expected), "missing {expected:?}");
    }

    engine
        .execute_collect_multi(&script)
        .expect("rerun idempotent blackthorn.traqula");
    let rerun_report = database
        .terrain_with_options(positorium::TerrainOptions {
            as_of: Some(
                positorium::datatype::Time::new_date_from("2026-01-01").expect("fixed cutoff"),
            ),
            ..positorium::TerrainOptions::default()
        })
        .expect("blackthorn terrain after rerun");
    assert_eq!(
        rerun_report, report,
        "rerunning must not duplicate the case"
    );
}

#[test]
fn add_and_search_examples_execute_together() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute_collect_multi(&source("adds.traqula"))
        .expect("execute adds.traqula");
    engine
        .execute_collect_multi(&source("searches.traqula"))
        .expect("execute searches.traqula");
}

#[test]
fn cookbook_traqula_blocks_execute_independently() {
    let cookbook =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guides/COOKBOOK.md"))
            .expect("read docs/guides/COOKBOOK.md");
    let blocks = cookbook
        .split("```traqula")
        .skip(1)
        .map(|remainder| remainder.split("```").next().unwrap().trim())
        .collect::<Vec<_>>();
    assert_eq!(blocks.len(), 8, "unexpected cookbook Traqula block count");

    for (index, block) in blocks.into_iter().enumerate() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute_collect_multi(block)
            .unwrap_or_else(|error| panic!("execute cookbook block {index}: {error}"));
    }
}
