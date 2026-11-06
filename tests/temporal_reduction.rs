use positorium::datatype::Time;
use positorium::{
    Database, DatabaseError, EffectCut, EffectLimits, Engine, PersistenceMode, TerrainLimits,
    TerrainOptions,
};
use std::collections::HashSet;

fn time(token: &str) -> Time {
    positorium::traqula::parse_time(token).unwrap()
}

#[test]
fn snapshots_terrain_and_effect_agree_with_pairwise_maxima_at_each_cut() {
    let specs = [
        (0, "@BOT"),
        (0, "'2023'"),
        (0, "'2024'"),
        (0, "'2024-06-15'"),
        (0, "'2024-06-15'"),
        (1, "'2024-06'"),
        (1, "'2024-06-15 12:00:00'"),
        (1, "'2024-07'"),
        (2, "@BOT"),
        (2, "@EOT"),
        (2, "@EOT"),
        (3, "'2024'"),
        (3, "'2024-06'"),
        (3, "'2024-06-15'"),
    ];
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let mut script = String::from("add role status;");
    let mut entities = HashSet::new();
    for (id, (entity, token)) in specs.iter().enumerate() {
        let prefix = if entities.insert(entity) { "+" } else { "" };
        script.push_str(&format!(
            "add posit +fact_{id} [{{({prefix}entity_{entity}, status)}}, {id}, {token}];"
        ));
    }
    engine.execute(&script).unwrap();

    let mut cuts = Vec::new();
    for cutoff in ["@BOT", "'2024'", "'2024-06-15'", "'2025'", "@EOT"] {
        let cut = time(cutoff);
        let expected = specs
            .iter()
            .enumerate()
            .filter(|(_, (group, token))| {
                let candidate = time(token);
                candidate.definitely_at_or_before(&cut)
                    && !specs.iter().any(|(other_group, other_token)| {
                        let other = time(other_token);
                        group == other_group
                            && other.definitely_at_or_before(&cut)
                            && candidate.definitely_before(&other)
                    })
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        for marker in ["", "latest "] {
            let query = format!(
                "search {marker}[{{(?entity, status)}}, ?value, *] as of {cutoff} return ?value order by ?value;"
            );
            let actual = engine
                .execute_collect(&query)
                .unwrap()
                .rows
                .into_iter()
                .map(|row| row[0].as_str().parse::<usize>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{query}");
        }
        let report = database
            .terrain_with_options(TerrainOptions {
                as_of: Some(cut.clone()),
                ..TerrainOptions::default()
            })
            .unwrap();
        assert_eq!(
            report.frames.current.stats.posits,
            expected.len() as u64,
            "{cutoff}"
        );
        cuts.push((cut, expected));
    }

    // Search variables are lexical; reuse the mutation bindings by appending
    // evidence to the original script in a fresh database.
    for id in 0..specs.len() {
        let source = if id == 0 { "+source" } else { "source" };
        script.push_str(&format!(
            "add posit [{{(fact_{id}, posit), ({source}, ascertains)}}, 100%, '2026'];"
        ));
    }
    let evidence = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&evidence).execute(&script).unwrap();
    for (cut, expected) in cuts {
        let slice = evidence
            .information_in_effect(EffectCut::new(Time::new_end_of_time(), cut))
            .unwrap();
        let mut actual = slice
            .assertions()
            .iter()
            .map(|row| row.target().value().token().parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }
}

#[test]
fn retraction_witnesses_preserve_overlap_ties_reassertions_and_source_scope() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let script = "add role status; \
        add posit +target [{(+entity, status)}, 1, '2020'], +other [{(entity, status)}, 2, '2020']; \
        add posit [{(target, posit), (+source, ascertains)}, 10%, '2023'], \
                  [{(target, posit), (source, ascertains)}, 20%, '2024'], \
                  [{(target, posit), (source, ascertains)}, 30%, '2024-06-15'], \
                  [{(target, posit), (source, ascertains)}, 40%, '2024-06-20'], \
                  [{(target, posit), (source, ascertains)}, 50%, '2024-06-20'], \
                  [{(target, posit), (source, ascertains)}, 0%, '2024-06'], \
                  [{(target, posit), (source, ascertains)}, 0%, '2024-06-15'], \
                  [{(target, posit), (+independent, ascertains)}, 60%, '2023-12-31'], \
                  [{(other, posit), (source, ascertains)}, 70%, '2023-01-01'];";
    engine.execute(script).unwrap();
    for (cutoff, expected) in [
        ("'2024-06-15'", vec![30, 60, 70]),
        ("'2025'", vec![20, 40, 50, 60, 70]),
    ] {
        let slice = database
            .information_in_effect(EffectCut::new(time(cutoff), Time::new_end_of_time()))
            .unwrap();
        let mut actual = slice
            .assertions()
            .iter()
            .map(|row| row.certainty_percent())
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }
    let retracted = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&retracted)
        .execute(&format!(
            "{script} add posit [{{(target, posit), (source, ascertains)}}, 0%, '2025'];"
        ))
        .unwrap();
    let slice = retracted
        .information_in_effect(EffectCut::new(
            Time::new_end_of_time(),
            Time::new_end_of_time(),
        ))
        .unwrap();
    let mut actual = slice
        .assertions()
        .iter()
        .map(|row| row.certainty_percent())
        .collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, [60, 70]);
}

#[test]
fn long_histories_fit_linear_comparison_budgets_and_enforce_exact_limits() {
    const ROWS: usize = 512;
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from(
        "add role status; add posit +target [{(+entity, status)}, 1, '2020']; add posit ",
    );
    for index in 0..ROWS {
        if index != 0 {
            script.push(',');
        }
        let source = if index == 0 { "+source" } else { "source" };
        script.push_str(&format!(
            "[{{(target, posit), ({source}, ascertains)}}, 100%, '{}']",
            2000 + index
        ));
    }
    script.push(';');
    Engine::new(&database).execute(&script).unwrap();
    let cut = || EffectCut::new(Time::new_end_of_time(), Time::new_end_of_time());
    let slice = database
        .information_in_effect_with_limits(
            cut(),
            EffectLimits {
                temporal_comparisons: (4 * ROWS) as u64,
                ..EffectLimits::default()
            },
        )
        .unwrap();
    assert_eq!(slice.assertions().len(), 1);
    let comparisons = slice.counters().temporal_comparisons;
    assert!(comparisons > 0 && comparisons <= (4 * ROWS) as u64);
    assert!(matches!(
        database.information_in_effect_with_limits(
            cut(),
            EffectLimits {
                temporal_comparisons: comparisons - 1,
                ..EffectLimits::default()
            }
        ),
        Err(DatabaseError::ResourceLimit {
            resource: "effective temporal comparisons",
            ..
        })
    ));
    assert!(
        database
            .information_in_effect_with_limits(
                cut(),
                EffectLimits {
                    temporal_comparisons: comparisons,
                    ..EffectLimits::default()
                }
            )
            .is_ok()
    );

    let tied = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from("add role status; add posit ");
    for index in 0..ROWS {
        if index != 0 {
            script.push(',');
        }
        let entity = if index == 0 { "+entity" } else { "entity" };
        script.push_str(&format!("[{{({entity}, status)}}, {index}, '2024']"));
    }
    script.push(';');
    Engine::new(&tied).execute(&script).unwrap();
    let report = tied
        .terrain_with_options(TerrainOptions {
            as_of: Some(Time::new_end_of_time()),
            limits: TerrainLimits {
                temporal_comparisons: (3 * ROWS) as u64,
                ..TerrainLimits::default()
            },
            ..TerrainOptions::default()
        })
        .unwrap();
    assert_eq!(report.frames.current.stats.posits, ROWS as u64);
}
