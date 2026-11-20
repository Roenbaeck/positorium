use positorium::{Database, ExecutionOptions, Result};

fn main() -> Result<()> {
    let db = Database::memory()?;
    db.execute("add role name, score;")?;

    let result = db.execute_one_with_options(
        r#"add posit [{(+person, name)}, $name, @NOW], [{(person, score)}, $score, @NOW];
           search [{(?person, name)}, $name, *], [{(?person, score)}, ?score, *]
           return ?person, ?score;"#,
        ExecutionOptions::default()
            .bind("name", "Ada")
            .bind("score", 42_i64),
    )?;
    for row in result.rows {
        println!("Thing {} has score {}", row[0], row[1]);
    }
    Ok(())
}
