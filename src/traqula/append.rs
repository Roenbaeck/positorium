//! Unconditional scripts can expand their bindings and construct candidate
//! batches on the caller's thread. Only the ordered phase uses store identities.

use super::*;
use crate::construct::{Appearance, AppearanceSet, Posit, Role};
use std::collections::HashSet;

type Target = (Arc<AppearanceSet>, LiteralValue, Time);

pub(super) trait AppendScope {
    fn fresh_thing(&mut self) -> Result<Thing, DatabaseError>;
    fn role(&mut self, name: &str) -> Result<Arc<Role>, DatabaseError>;

    fn appearance_set<'a>(
        &mut self,
        members: impl IntoIterator<Item = (&'a str, Thing)>,
    ) -> Result<Arc<AppearanceSet>, DatabaseError> {
        let appearances = members
            .into_iter()
            .map(|(name, thing)| Ok(Arc::new(Appearance::new(thing, self.role(name)?))))
            .collect::<Result<_, DatabaseError>>()?;
        AppearanceSet::checked(appearances)
    }

    fn finish(
        &mut self,
        targets: Vec<Target>,
        assertions: Vec<LiteralAssertionSpec>,
    ) -> Result<Option<CreatedLiteralPosits>, DatabaseError>;
}

pub(super) struct Writer<'a> {
    database: &'a Database,
    view: &'a mut ReadView,
}

impl<'a> Writer<'a> {
    pub(super) fn new(database: &'a Database, view: &'a mut ReadView) -> Self {
        Self { database, view }
    }
    fn commit(
        &mut self,
        targets: Vec<Target>,
        assertions: Vec<LiteralAssertionSpec>,
    ) -> Result<CreatedLiteralPosits, DatabaseError> {
        let kept = self
            .database
            .create_literal_posits_with_assertions(self.view, targets, assertions)?;
        info!(target: "positorium::traqula", event="add_posit_commit", targets=kept.targets.len(), assertions=kept.assertions.len(), "posit command committed");
        Ok(kept)
    }
}

impl AppendScope for Writer<'_> {
    fn fresh_thing(&mut self) -> Result<Thing, DatabaseError> {
        Ok(self
            .database
            .thing_generator
            .lock()
            .map_err(|err| DatabaseError::Lock(err.to_string()))?
            .reserve())
    }

    fn role(&mut self, name: &str) -> Result<Arc<Role>, DatabaseError> {
        self.view
            .normalized_role(name)
            .ok_or_else(|| DatabaseError::UnknownRole(name.to_owned()))
    }

    fn finish(
        &mut self,
        targets: Vec<Target>,
        assertions: Vec<LiteralAssertionSpec>,
    ) -> Result<Option<CreatedLiteralPosits>, DatabaseError> {
        self.commit(targets, assertions).map(Some)
    }
}

enum Command {
    Roles(Vec<String>),
    Posits {
        /// Slots allocated by `+thing` before binding this command's results.
        fresh: std::ops::Range<usize>,
        targets: Vec<Target>,
        assertions: Vec<LiteralAssertionSpec>,
        /// Local posit identities referenced by subsequent commands.
        bindings: Vec<Thing>,
    },
}

#[derive(Default)]
struct PreparedAppends {
    commands: Vec<Command>,
    slots: usize,
}

#[derive(Default)]
struct Preparation {
    plan: PreparedAppends,
    roles: HashMap<String, Arc<Role>, OtherHasher>,
    fresh_start: usize,
    // Equal propositions must share one binding when a later command fans out.
    bound_posits: HashSet<Arc<Posit<LiteralValue>>, OtherHasher>,
    track_posits: bool,
}

impl Preparation {
    fn slot(&mut self) -> Thing {
        let id = self.plan.slots as Thing;
        self.plan.slots += 1;
        id
    }

    fn bound_posit(
        &mut self,
        set: Arc<AppearanceSet>,
        value: &LiteralValue,
        time: &Time,
    ) -> Arc<Posit<LiteralValue>> {
        let candidate = Posit::new(self.plan.slots as Thing, set, value.clone(), time.clone());
        if let Some(existing) = self.bound_posits.get(&candidate) {
            return Arc::clone(existing);
        }
        self.slot();
        let candidate = Arc::new(candidate);
        self.bound_posits.insert(Arc::clone(&candidate));
        candidate
    }

    fn prepare(
        mut self,
        commands: Pairs<'_, Rule>,
        execution: &ExecutionContext,
    ) -> Result<PreparedAppends, DatabaseError> {
        let mut variables = Variables::default();
        let commands = commands
            .enumerate()
            .filter(|(_, command)| command.as_rule() != Rule::EOI);
        // A later +posit binding can rediscover a previously unnamed target or
        // envelope. Give it the slot for its first occurrence so subsequent
        // fan-out visits identities in the same order as the ordered writer.
        let last_binding = commands.clone().rev().skip(1).find_map(|(index, command)| {
            (command.as_rule() == Rule::add_posit
                && command.into_inner().any(|item| {
                    item.into_inner().next().is_some_and(|first| {
                        matches!(first.as_rule(), Rule::insert | Rule::assertion_binding)
                    })
                }))
            .then_some(index)
        });
        for (index, command) in commands {
            execution.check()?;
            self.track_posits = last_binding.is_some_and(|last| index <= last);
            match command.as_rule() {
                Rule::add_role => {
                    let names: Vec<_> = command
                        .into_inner()
                        .map(|role| parse_role_name(role.as_str()))
                        .collect();
                    self.plan.commands.push(Command::Roles(names));
                }
                Rule::add_posit => {
                    Engine::construct_posits(&mut self, command, &mut variables, execution)?
                }
                _ => unreachable!("only unconditional appends enter preparation"),
            }
        }
        Ok(self.plan)
    }
}

impl AppendScope for Preparation {
    fn fresh_thing(&mut self) -> Result<Thing, DatabaseError> {
        Ok(self.slot())
    }

    fn role(&mut self, name: &str) -> Result<Arc<Role>, DatabaseError> {
        if let Some(role) = self.roles.get(name) {
            return Ok(Arc::clone(role));
        }
        // Existence and canonical store identity are checked in command order at commit.
        let role = Arc::new(Role::new(self.roles.len() as Thing, name.into(), false));
        self.roles.insert(name.to_owned(), Arc::clone(&role));
        Ok(role)
    }

    fn finish(
        &mut self,
        targets: Vec<Target>,
        assertions: Vec<LiteralAssertionSpec>,
    ) -> Result<Option<CreatedLiteralPosits>, DatabaseError> {
        let fresh = self.fresh_start..self.plan.slots;
        let mut bindings = Vec::new();
        let bound = if self.track_posits {
            let target_posits: Vec<_> = targets
                .iter()
                .map(|(set, value, time)| self.bound_posit(Arc::clone(set), value, time))
                .collect();
            let mut envelopes = Vec::with_capacity(assertions.len());
            for spec in &assertions {
                let target = target_posits[spec.target_index].posit();
                let set = self.appearance_set([("posit", target), ("ascertains", spec.positor)])?;
                envelopes.push(self.bound_posit(set, &spec.certainty, &spec.time));
            }
            bindings.extend(
                target_posits
                    .iter()
                    .chain(&envelopes)
                    .map(|posit| posit.posit()),
            );
            Some(CreatedLiteralPosits {
                targets: target_posits,
                assertions: envelopes,
            })
        } else {
            None
        };
        self.fresh_start = self.plan.slots;
        self.plan.commands.push(Command::Posits {
            fresh,
            targets,
            assertions,
            bindings,
        });
        Ok(bound)
    }
}

impl PreparedAppends {
    fn commit(
        self,
        engine: &Engine<'_>,
        execution: &ExecutionContext,
    ) -> Result<(), DatabaseError> {
        let mut identities = vec![0; self.slots];
        let owner = engine.acquire_owner(execution)?;
        let mut current = ScriptView::new(engine.database, Some(owner));
        execution.check()?;
        let view = current.write()?;
        for command in self.commands {
            execution.check()?;
            match command {
                Command::Roles(names) => engine.add_role_names(view, names)?,
                Command::Posits {
                    fresh,
                    targets,
                    mut assertions,
                    bindings,
                } => {
                    let mut writer = Writer::new(engine.database, view);
                    {
                        let mut generator = writer
                            .database
                            .thing_generator
                            .lock()
                            .map_err(|err| DatabaseError::Lock(err.to_string()))?;
                        for slot in fresh {
                            identities[slot] = generator.reserve();
                        }
                    }
                    let resolve = |slot: Thing| {
                        identities
                            .get(slot as usize)
                            .copied()
                            .filter(|&id| id != 0)
                            .ok_or_else(|| {
                                DatabaseError::Invariant(format!(
                                    "unresolved append identity {slot}"
                                ))
                            })
                    };
                    let targets = targets
                        .into_iter()
                        .map(|(set, value, time)| {
                            execution.check()?;
                            let members = set
                                .appearances()
                                .iter()
                                .map(|member| {
                                    let thing = resolve(member.thing())?;
                                    Ok(Arc::new(Appearance::new(
                                        thing,
                                        writer.role(member.role_name())?,
                                    )))
                                })
                                .collect::<Result<_, DatabaseError>>()?;
                            Ok((AppearanceSet::checked(members)?, value, time))
                        })
                        .collect::<Result<_, DatabaseError>>()?;
                    for assertion in &mut assertions {
                        assertion.positor = resolve(assertion.positor)?;
                    }
                    execution.check()?;
                    let kept = writer.commit(targets, assertions)?;
                    debug_assert!(
                        bindings.is_empty()
                            || bindings.len() == kept.targets.len() + kept.assertions.len()
                    );
                    for (slot, posit) in bindings
                        .into_iter()
                        .zip(kept.targets.iter().chain(&kept.assertions))
                    {
                        identities[slot as usize] = posit.posit();
                    }
                }
            }
        }
        Ok(())
    }
}

impl Engine<'_> {
    /// A failed speculative preparation has touched no store state. Executing it
    /// through the regular writer preserves the first command error, and the
    /// durable prefix preceding it, in their original order.
    pub(super) fn execute_appends(
        &self,
        commands: &Pairs<'_, Rule>,
        execution: &ExecutionContext,
    ) -> Result<bool, DatabaseError> {
        if !commands
            .clone()
            .next()
            .is_some_and(|command| matches!(command.as_rule(), Rule::add_role | Rule::add_posit))
            || !commands.clone().all(|command| {
                matches!(
                    command.as_rule(),
                    Rule::add_role | Rule::add_posit | Rule::EOI
                )
            })
        {
            return Ok(false);
        }
        let plan = Preparation::default().prepare(commands.clone(), execution);
        execution.check()?;
        let Ok(plan) = plan else {
            return Ok(false);
        };
        plan.commit(self, execution)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::PersistenceMode;

    #[test]
    fn rebinding_older_propositions_preserves_identity_order_in_later_fanout() {
        let appends = r#"
            add role tag, link, evidence;
            add posit [{(+case, tag)}, 1, '2024'] and assert by +source with 90% at '2025';
            add posit +p [{(case, tag)}, 2, '2024'] and assert +e by source with 90% at '2025';
            add posit +p [{(case, tag)}, 1, '2024'] and assert +e by source with 90% at '2025';
            add posit [{(p, link), (e, evidence)}, "link", '2026'];
        "#;
        let search =
            "search ?id = [{(?p, link), (?e, evidence)}, *, *] return ?id, ?p, ?e order by ?id;";
        let run = |suffix: &str| {
            let db = Database::new(PersistenceMode::InMemory).unwrap();
            let engine = Engine::new(&db);
            engine.execute(&format!("{appends}{suffix}")).unwrap();
            engine.execute_collect(search).unwrap().rows
        };
        let prepared = run("");
        assert_eq!(prepared.len(), 4);
        assert_eq!(prepared, run("explain search [{(*, tag)}, *, *];"));
    }

    #[test]
    fn independently_prepared_batches_resolve_roles_against_the_commit_view() {
        let db = Database::new(PersistenceMode::InMemory).unwrap();
        let engine = Engine::new(&db);
        let owner = db.execution_owner.lock().unwrap();
        let plans = std::thread::scope(|scope| {
            let workers = (0..2)
                .map(|value| {
                    scope.spawn(move || {
                        let script = format!("add posit [{{(+case, status)}}, {value}, '2024'];");
                        Preparation::default()
                            .prepare(
                                parse_script(&script).unwrap(),
                                &ExecutionOptions::default().resolve(&script).unwrap(),
                            )
                            .unwrap()
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(db.read_view().posit_count(), 0);
        assert!(!db.contains_role("status").unwrap());
        assert_eq!(db.thing_generator.lock().unwrap().len(), 5);
        drop(owner);
        engine
            .execute("add role status; add posit [{(+prior, status)}, 2, '2024'];")
            .unwrap();
        for plan in plans {
            plan.commit(&engine, &ExecutionOptions::default().resolve("").unwrap())
                .unwrap();
        }
        let result = engine
            .execute_collect("search [{(?case, status)}, ?v, *] return ?case, ?v order by ?v;")
            .unwrap();
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row[1].as_str())
                .collect::<Vec<_>>(),
            ["0", "1", "2"]
        );
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row[0].as_str())
                .collect::<HashSet<_>>()
                .len(),
            3
        );
    }
}
