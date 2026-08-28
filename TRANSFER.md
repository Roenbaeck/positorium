# Positorium Logical Transfer And Backup

This document specifies logical export version 1, identity-remap version 1, and
the physical backup procedure for append-only beta stores. These versions are
independent of storage format version 1.0.

## Logical export version 1

A logical export is UTF-8 JSON Lines. Every line is one complete JSON object,
has a maximum encoded size of 16 MiB, and ends with LF. Blank lines are invalid.
The first record is the only header, every Role record follows it, and every
Posit record follows all Roles. Writers order Roles and Posits by local identity
for reproducible output. Readers reject unknown fields, record kinds, versions,
literal families, time precisions, and cross-store identity references.

The header is:

```json
{"record":"header","format":"positorium.logical-export","version":1,"source_store_uuid":"123e4567-e89b-42d3-a456-426614174000"}
```

A Role carries its external identity and complete catalog metadata:

```json
{"record":"role","identity":{"store_uuid":"123e4567-e89b-42d3-a456-426614174000","local":3},"name":"value","reserved":false}
```

A Posit carries its external identity, complete sorted appearance set, exact
literal family and token, and structured time:

```json
{"record":"posit","identity":{"store_uuid":"123e4567-e89b-42d3-a456-426614174000","local":5},"appearances":[{"role":{"store_uuid":"123e4567-e89b-42d3-a456-426614174000","local":3},"thing":{"store_uuid":"123e4567-e89b-42d3-a456-426614174000","local":4}}],"literal":{"family":"decimal","token":"+001.00"},"time":{"precision":"date","year":2026,"month":8,"day":26}}
```

Literal family names are `string`, `integer`, `decimal`, `certainty`, `json`,
and `time`. The token is the complete identity-bearing UTF-8 token. Time
precision values are `beginning_of_time`, `end_of_time`, `year`, `year_month`,
`date`, and `date_time_utc`; concrete forms carry their numeric components and
never depend on locale or formatted display text.

Every identity in an export is the pair `(source_store_uuid, local)`. Import
creates a new store UUID and constructs one complete collision-free remap table
before writing records. Every non-built-in foreign identity receives a different
local number, even when its old number happens to be unused. The five fixed
built-in Roles are the exception: D006 fixes `posit` = 1, `ascertains` = 2,
`thing` = 3, `class` = 4, and `subclass` = 5 in every store. Import maps those
catalog entries to the same destination identities. All references are rewritten
through the same table, which includes those five explicit mappings.

Import accepts only a destination path that does not exist. It writes and
durably flushes a new store, replays and compares every transformed logical
record, reopens it through read-only inspection, verifies counts, and only then
writes the identity-remap artifact. A failure never modifies the source export;
an incomplete destination remains visibly beside it and is never activated as a
replacement store.

## Identity-remap version 1

The remap artifact is one UTF-8 JSON document:

```json
{
  "format": "positorium.identity-remap",
  "version": 1,
  "source_store_uuid": "123e4567-e89b-42d3-a456-426614174000",
  "destination_store_uuid": "987e6543-e21b-42d3-a456-426614174000",
  "mappings": [
    {
      "source": {"store_uuid": "123e4567-e89b-42d3-a456-426614174000", "local": 3},
      "destination": {"store_uuid": "987e6543-e21b-42d3-a456-426614174000", "local": 4}
    }
  ]
}
```

Mappings are ordered by source local identity and cover every Role, Posit, and
appearing Thing referenced by the export.

## Physical backup

A physical backup is an offline, immutable snapshot of one store. The backup
tool acquires the store's shared read/backup lock, validates the manifest and
every committed frame, copies the active log only through the manifest-recorded
committed length, and writes the manifest last. It excludes an uncommitted tail
without truncating or otherwise modifying the source. The destination must not
exist and cannot be nested inside the source store. The tool reopens and validates
the completed backup before reporting success.

## Command-line tool

```text
positorium-store inspect STORE
positorium-store backup STORE DESTINATION
positorium-store dump STORE OUTPUT.jsonl
positorium-store import INPUT.jsonl DESTINATION REMAP.json
```

All successful commands print a structured JSON report. The writer/server must
be stopped before inspection, dump, or backup so the shared lock can be acquired.
