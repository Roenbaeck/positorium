use positorium::error::{DatabaseError, Result};
use positorium::maintenance::{backup_store, export_store, import_store, inspect_store};
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run(std::env::args_os().skip(1).collect()) {
        eprintln!("positorium-store: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<OsString>) -> Result<()> {
    let Some(command) = arguments.first().and_then(|argument| argument.to_str()) else {
        print_usage();
        return Err(DatabaseError::Config("missing maintenance command".into()));
    };
    match (command, &arguments[1..]) {
        ("inspect", [store]) => print_json(&inspect_store(PathBuf::from(store))?),
        ("backup", [store, destination]) => print_json(&backup_store(
            PathBuf::from(store),
            PathBuf::from(destination),
        )?),
        ("dump", [store, output]) => {
            print_json(&export_store(PathBuf::from(store), PathBuf::from(output))?)
        }
        ("import", [export, destination, remap]) => print_json(&import_store(
            PathBuf::from(export),
            PathBuf::from(destination),
            PathBuf::from(remap),
        )?),
        ("help" | "--help" | "-h", []) => {
            print_usage();
            Ok(())
        }
        _ => {
            print_usage();
            Err(DatabaseError::Config(format!(
                "invalid arguments for '{command}'"
            )))
        }
    }
}

fn print_json(value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(std::io::stdout().lock(), value)
        .map_err(|error| DatabaseError::Execution(error.to_string()))?;
    println!();
    Ok(())
}

fn print_usage() {
    eprintln!(
        "Usage:\n  positorium-store inspect STORE\n  positorium-store backup STORE DESTINATION\n  positorium-store dump STORE OUTPUT.jsonl\n  positorium-store import INPUT.jsonl DESTINATION REMAP.json"
    );
}
