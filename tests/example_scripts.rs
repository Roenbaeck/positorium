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
        "cross-search.traqula",
        "heist.traqula",
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
