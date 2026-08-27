use positorium::{Database, Engine, PersistenceMode, ResultCellKind};
use std::collections::HashSet;

const TERRAIN_FIXTURE: &str = include_str!("../traqula/terrain.traqula");

#[test]
fn terrain_fixture_returns_history_and_snapshot_incidence_rows() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let results = Engine::new(&database)
        .execute_collect_multi(TERRAIN_FIXTURE)
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].columns,
        [
            "posit_id",
            "appearance_set",
            "thing",
            "role_name",
            "value",
            "time"
        ]
    );
    assert_eq!(results[0].row_count, 30);
    assert_eq!(results[1].row_count, 27);
    assert_eq!(results[0].rows[0][0].kind, ResultCellKind::Posit);
    assert_eq!(results[0].rows[0][1].kind, ResultCellKind::AppearanceSet);
    assert_eq!(results[0].rows[0][2].kind, ResultCellKind::Thing);
    assert_eq!(results[0].rows[0][3].kind, ResultCellKind::Role);
    assert_eq!(results[0].rows[0][4].kind, ResultCellKind::Literal);
    assert_eq!(results[0].rows[0][5].kind, ResultCellKind::Time);

    let cardinalities = results
        .iter()
        .map(|result| {
            [0, 1, 2, 3].map(|column| {
                result
                    .rows
                    .iter()
                    .map(|row| row[column].as_str())
                    .collect::<HashSet<_>>()
                    .len()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(cardinalities[0], [26, 24, 6, 8]);
    assert_eq!(cardinalities[1], [24, 24, 6, 8]);

    let relationship_counts = results
        .iter()
        .map(|result| {
            let rows = result
                .rows
                .iter()
                .filter(|row| matches!(row[3].as_str(), "owner" | "pet"))
                .collect::<Vec<_>>();
            let sets = rows
                .iter()
                .map(|row| row[1].as_str())
                .collect::<HashSet<_>>();
            let posits = rows
                .iter()
                .map(|row| row[0].as_str())
                .collect::<HashSet<_>>();
            (sets.len(), posits.len())
        })
        .collect::<Vec<_>>();
    assert_eq!(relationship_counts, [(3, 4), (3, 3)]);
}
