# Embed Positorium in a Rust application

Add Positorium to an existing Cargo project using stable Rust.

```sh
cargo add positorium@=0.3.3-beta.8 --no-default-features --features persistence
```

For an in-memory application, omit `--features persistence`. To develop against
a local checkout, replace `positorium@=0.3.3-beta.8` with
`--path ../positorium`. The default features also build the CLI and HTTP service.

## Open a database and execute a script

```rust
use positorium::{Database, ExecutionOptions, Result, ResultCellKind};

fn main() -> Result<()> {
    let db = Database::open("orders.store")?;
    db.execute("add role order_number, amount;")?;
    let orders = db.execute_one_with_options(
        "add posit [{(+order, order_number)}, $number, @NOW], \
                   [{(order, amount)}, $amount, @NOW]; \
         search [{(?order, order_number)}, $number, *], \
                [{(?order, amount)}, ?amount, *] return ?order, ?amount;",
        ExecutionOptions::default().bind("number", "A-17").bind("amount", 42_i64),
    )?;
    if let Some(amount) = orders.cell(0, "amount") {
        assert_eq!(amount.kind, ResultCellKind::Literal);
        println!("Recorded amount {}", amount.as_str());
    }
    Ok(())
}
```

`Database::open` accepts a filesystem path and creates the store directory on
first use. Drop the last database handle to release the store lock. Open the
same directory again to recover committed commands. Use `Database::memory()?`
for a database that lives for the lifetime of its handle.

## Parameters and results

- Bind Rust strings and `i64` values to `$name` placeholders with
  `ExecutionOptions::bind`. A string containing quotes or Traqula syntax is
  encoded as a single string value. For an exact literal such as `+0010.00`,
  pass `ExecutionParameter::Literal { text: "+0010.00".into() }`.
  `ExecutionParameter::Time` accepts a time token such as `'2026-11'`.
- `db.execute` returns one result set per projection, in script order. A
  write-only script yields an empty vector. Use `db.execute_one` for exactly
  one result set, including an empty search result. It checks the number of
  projections before applying writes. Both methods accept execution options
  through their `_with_options` counterparts.
- A result set has column names and rows of `ResultCell`. Each cell preserves
  its kind and exact entered text, including quotes, signs and decimal scale.
  `cell(row, name)` returns `None` for a missing row or column.
- Operations return `positorium::Result<T>`. Use `?` to propagate errors or
  match `DatabaseError` to distinguish parse errors, timeouts, resource limits
  and storage errors. Each successful mutation command commits independently.

To reuse a saved constraint across scripts, call `saved.reference(0, "revision")?`
on the result of `add constraint`. Pass that parameter to
`db.execute_one_with_options("check constraint $rev;", ExecutionOptions::default().bind("rev", revision))?`.
The reference carries the originating store UUID and identifies that exact
revision after a newer definition is saved.

For concurrent callers, share an `Arc<Database>` across threads. Read-only
scripts use one snapshot. For larger results, borrow the same database with
`Engine::new(&db)` and stream rows through `execute_stream_multi_with_options`.
The engine also exposes query plans through `explain`. [Rust API](https://docs.rs/positorium/0.3.3-beta.8/positorium/) · [Traqula reference](../reference/TRAQULA.md)

Run the standalone example from the Positorium checkout with

```sh
cargo run --no-default-features --example embedded
```
