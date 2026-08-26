# Positorium Beta Operations

## Configuration

Copy `positorium.json` beside the native `positorium` binary. The default binds
the trusted HTTP service to loopback, opens `positorium.store`, and executes the
configured Traqula startup file. Persistence and destructive recreation are
separate booleans; recreation defaults to false. Invalid values, unreadable
startup scripts, failed replay, unsupported formats, and bind failures stop
startup. A non-loopback bind remains unauthenticated and is not safe Internet
exposure.

## Backup and inspection

Stop the writer before offline maintenance. Create a validated physical backup:

```text
positorium-store backup positorium.store backup.store
```

The destination must not exist. The tool copies only the committed prefix,
writes the manifest last, and validates the result. It never truncates or edits
the source. Inspect or create a stable logical dump with:

```text
positorium-store inspect positorium.store
positorium-store dump positorium.store export.jsonl
```

Keep physical backups and logical exports outside the live store directory.

## Restore and transfer

Restore a physical backup by configuring the server to open the retained backup
directory while no other process owns it. Import a logical export into a new
store and retain the generated collision-free identity map:

```text
positorium-store import export.jsonl restored.store identity-remap.json
```

Import never writes into an existing destination and never preserves foreign
non-built-in local identifiers. The exact formats and built-in identity exception
are specified in `TRANSFER.md`.

## Migration and compatibility

Store format 1.0 is the first published persistence format. No SQLite importer or
prototype migration exists because no release used SQLite (D031). Future breaking
store migrations must write and validate a new store beside the source, retain the
old store for rollback, and provide the direct or stepwise logical-data path in
the release notes. Independent API/language/wire versions and the 0.x policy are
listed in `CONTRACTS.md`.

## Release artifacts

Tags matching `vMAJOR.MINOR.PATCH` (including prerelease suffixes) build stable
Rust binaries for Linux x86-64, macOS Apple Silicon, and Windows x86-64. GitHub
Releases contain generated change notes, an archive per platform, and a SHA-256
file for each archive. Verify the checksum before unpacking.
