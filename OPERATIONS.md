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

The physical backup unit is the manifest, lock target, and every log byte through
the manifest-recorded committed length. Copying only a log file or copying past
that length is not a valid backup procedure.

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

## Durability, failures, and shutdown

Each semicolon-delimited `add role` or `add posit` command is one atomic batch.
An `and assert` suffix persists its target and assertion envelopes in that same
batch. A `search ... or add posit ...` command holds the script execution owner
across search and fallback selection; when the fallback runs, its posits and
optional assertions form one atomic batch.
For a persistent store, success is returned only after the records and Commit
frame are flushed, the replacement manifest is flushed and renamed, and the
required directory entries are synchronized. A multi-posit command is all or
nothing. Earlier successful commands remain committed if a later command fails.

If a log or manifest write has an uncertain outcome, the active writer becomes
fail-closed and rejects further writes. Restart the process: writable recovery
replays the manifest's committed prefix and truncates only bytes after it.
Corruption or truncation inside committed history is fatal and is never repaired
in place. Restore a verified backup or import a logical export instead.

Graceful server shutdown stops accepting work and flushes the database before
returning. Treat an unsuccessful shutdown as an unclean stop and inspect the
store before relying on it operationally.

## Trusted interface and resource limits

The HTTP beta is unauthenticated and intended for trusted local clients. It
binds to `127.0.0.1` by default. Exact loopback CORS origins may be configured;
wildcard origins are rejected. A non-loopback bind does not make the service
safe for Internet exposure.

Defaults and hard boundaries are:

| Resource | Default | Hard maximum |
| --- | ---: | ---: |
| Request body | 1 MiB | 16 MiB |
| Script commands | 1,000 | 1,000 |
| Execution time | 5 seconds | 30 seconds |
| Rows per search | 100,000 | 100,000 |
| Complete storage frame | 16 MiB | 16 MiB |

`timeout_ms` may lower the configured deadline but cannot raise it. Buffered and
SSE responses expose actual row truncation through `limited`. Timeouts and
cancellation are cooperative and never publish a command that failed before its
durable commit.

## Release artifacts

Tags matching `vMAJOR.MINOR.PATCH` (including prerelease suffixes) build stable
Rust binaries for Linux x86-64, macOS Apple Silicon, and Windows x86-64. GitHub
Releases contain generated change notes, an archive per platform, and a SHA-256
file for each archive. Verify the checksum before unpacking.

The modeling and maintenance recipes in `COOKBOOK.md` complement these
procedures; normative transfer details remain in `TRANSFER.md`.
