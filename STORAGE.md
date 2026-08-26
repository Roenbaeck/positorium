# Positorium Store Format v1

Status: normative design for the first beta format. The implementation may not
claim format v1 support until every required validation and recovery rule in
this document is covered by tests.

This specification implements decisions D003 and D020-D027. Terms such as
MUST, MUST NOT, SHOULD, and MAY are normative.

## 1. Store directory and ownership

A store is one directory containing:

- `manifest.pmf`, the current manifest;
- `store.lock`, the operating-system lock target; and
- one or more log segments named `log-<16 lowercase hex digits>.ptl`.

One exclusive operating-system lock on `store.lock` covers the manifest and all
active or sealed log segments. A writable open MUST acquire it before reading
the manifest and retain it until shutdown. A second owner MUST fail to open the
store. Read-only inspection and backup use a shared lock when the platform can
provide one; otherwise they coordinate through the exclusive owner.

The store UUID is a randomly generated 128-bit UUID recorded in the manifest
and every log header. Thing identifiers are unsigned 64-bit integers scoped to
that UUID. A stable external identity is `(store UUID, Thing)`.

## 2. Primitive encodings

- Fixed-width integers are unsigned little-endian unless explicitly signed.
- Signed calendar years are little-endian two's-complement `i32` values.
- Variable lengths and counts use canonical unsigned LEB128. An encoding with
  redundant high zero groups is invalid.
- Booleans are one byte: `0` or `1`; every other value is invalid.
- Text is a ULEB128 byte length followed by that many well-formed UTF-8 bytes.
- Reserved bytes and flag bits MUST be zero. Readers reject unknown mandatory
  flags and ignore only flags explicitly designated optional by a supported
  format version.
- Decoders MUST use checked arithmetic and validate lengths before allocating.

The maximum complete record frame, including framing and checksum, is
16,777,216 bytes (16 MiB). No manifest, header, length, or nested count may
cause an allocation above that limit.

## 3. Manifest

`manifest.pmf` is a deterministic binary document:

| Field | Encoding |
| --- | --- |
| Magic | 8 bytes: `POSITPMF` |
| Manifest major | `u16`, initially `1` |
| Manifest minor | `u16`, initially `0` |
| Header length | `u32` |
| Store UUID | 16 UUID bytes in network order |
| Required feature flags | `u64` |
| Optional feature flags | `u64` |
| Segment count | ULEB128 |
| Segments | repeated segment entries below |
| CRC32C | `u32` over every prior manifest byte |

A segment entry contains its UTF-8 file name, `u64` ordinal, one-byte sealed
flag, and `u64` committed length. Names MUST be simple file names matching the
log naming rule; path separators and traversal components are invalid. Ordinals
are unique and strictly increasing in manifest order. Exactly one final segment
may be unsealed.

The UUID is immutable. Feature flags and active files change only through an
atomic manifest replacement: write `manifest.next`, flush the file, rename it
over `manifest.pmf`, then flush the store directory before acknowledging the
operation. A reopen MUST reject a bad checksum, UUID change, unsupported
required flag, missing segment, duplicate ordinal, or committed length beyond
the corresponding file length.

The initial beta defines no required or optional feature bits. Bit allocation is
part of the format registry. Bit 0 in the optional field is reserved for a
future BLAKE3 chain over committed framed bytes; it has no v1 semantics.

## 4. Log header

Every `.ptl` segment begins with this fixed 64-byte header:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic `POSITLOG` |
| 8 | 2 | format major `1` |
| 10 | 2 | format minor `0` |
| 12 | 4 | header length `64` |
| 16 | 16 | store UUID |
| 32 | 8 | segment ordinal |
| 40 | 8 | required feature flags |
| 48 | 8 | optional feature flags |
| 56 | 4 | reserved zero bytes |
| 60 | 4 | CRC32C over bytes 0 through 59 |

The UUID and supported flags MUST agree with the manifest. Record sequence
numbers continue across segments and never reset. Segment rotation is permitted
by the format but is not required by the first beta implementation.

## 5. Record frame

Records immediately follow the log header:

| Field | Encoding |
| --- | --- |
| Sync word | 4 bytes: `PTR1` |
| Frame version | `u8`, initially `1` |
| Record type | `u8` |
| Required record flags | `u16` |
| Sequence | `u64` |
| Payload length | canonical ULEB128 |
| Payload | exactly `payload length` bytes |
| CRC32C | `u32` over the complete frame before this field |

Sequences begin at 1, are unique, and increase by exactly 1. A record with an
unknown type, unsupported version, or unknown required flag is invalid. CRC32C
detects accidental corruption; v1 has no rolling cryptographic chain and makes
no tamper-proof claim.

Record types are:

| Type | Name | Batch member |
| ---: | --- | --- |
| `1` | Role | yes |
| `2` | Codec | yes |
| `3` | Posit | yes |
| `255` | Commit | terminates the batch |

Types `4..=127` are reserved for future mandatory records. Types `128..=254`
are reserved for future explicitly optional records and may be skipped only
when a future supported version defines that behavior.

## 6. Role record

A Role payload is:

1. Role Thing identity as `u64`.
2. Reserved status as a boolean byte.
3. Canonical role name as UTF-8 text.

The name MUST already be Unicode NFC, is case-sensitive, and is non-empty. A
store has one immutable role per identity and one identity per canonical name.
Repeating an identical Role record is permitted only when an importer is
explicitly operating in idempotent mode; the normal writer emits it once.
Conflicting repeats are corruption.

Role identities are Thing identities. Format v1 fixes `posit` to Thing 1 and
`ascertains` to Thing 2, both reserved. Any other metadata for those names or
identities is incompatible with this format.

## 7. Codec record and closed registry

A Codec payload is:

1. Codec identifier as `u16`.
2. Codec version as `u16`.
3. Literal-family identifier as `u8`.
4. Required capability flags as `u16`.
5. Stable ASCII codec name as UTF-8 text.

The `(codec identifier, codec version)` pair has immutable decoding semantics.
Codec metadata MUST precede every dependent Posit in the same log. Unknown
required pairs fail replay.

Literal-family identifiers are private storage metadata and are never returned
as user-facing types:

Codec metadata uses family identifier `0` only for a codec, such as the
mandatory raw fallback, whose immutable definition applies to every family.
Posit records always carry one of the concrete family identifiers below.

| Family | Meaning |
| ---: | --- |
| `1` | string token |
| `2` | integer token |
| `3` | decimal token |
| `4` | certainty token |
| `5` | JSON token |
| `6` | time-valued token |

The mandatory v1 codec registry is:

| Codec/version | Name | Families | Payload |
| --- | --- | --- | --- |
| `0/1` | `raw-utf8-token` | all | exact UTF-8 token bytes |
| `1/1` | `canonical-i64` | integer | zig-zag LEB128 |
| `2/1` | `canonical-certainty` | certainty | signed `i8` percentage |

Every literal family MUST support `raw-utf8-token`. A compact codec is legal
only when decoding and rendering reproduces the exact input token byte for byte.
For example, integer `10` may use `canonical-i64`, while `010` must use raw UTF-8;
certainty `75%` may use its compact codec, while `075%` must use raw UTF-8.
Strings and JSON use raw UTF-8 in v1 so escape spelling, whitespace, key order,
and number spelling remain identity-bearing. New compact codecs require new
immutable registry pairs, not reinterpretation of an existing pair.

## 8. Time encoding

Appearance time is encoded independently from a value codec:

| Tag | Meaning | Following fields |
| ---: | --- | --- |
| `0` | beginning of time | none |
| `1` | end of time | none |
| `2` | year | `i32 year` |
| `3` | year-month | `i32 year`, `u8 month` |
| `4` | date | `i32 year`, `u8 month`, `u8 day` |
| `5` | UTC datetime | date fields, `u8 hour`, `u8 minute`, `u8 second`, `u32 nanosecond` |

Calendar fields MUST form a valid proleptic-Gregorian value. Month is `1..=12`,
second is `0..=59`, and nanosecond is `0..=999,999,999`; leap seconds and UTC
offsets are invalid. The tag preserves precision. Semantic time relations use
the half-open intervals defined by D007/D008; the byte encoding does not replace
stored-time identity.

## 9. Posit record

A Posit payload is:

1. Posit Thing identity as `u64`.
2. Appearance count as ULEB128.
3. That many appearances, each `u64 role identity` then `u64 thing identity`.
4. Literal-family identifier as `u8`.
5. Codec identifier and version as two `u16` values.
6. Encoded literal payload as ULEB128 length plus bytes.
7. Appearance time using section 8.

Appearances MUST be sorted by `(role identity, thing identity)` and role
identities MUST be unique. The set therefore encodes a finite partial function
from Role to Thing. Every referenced role and codec MUST have an earlier record.
Thing identities used only as appearing things become durable through this
committed Posit; there is no standalone Thing record in v1.

The literal codec MUST reconstruct the exact identity-bearing UTF-8 value token.
Proposition identity is `(AppearanceSet, exact LiteralValue token, Time)`.
Posit Thing identity and physical codec choice are excluded. Re-adding an
identical proposition returns the existing canonical Posit and emits no record.
Append sequence is physical order only and MUST NOT break semantic ties.

## 10. Commit record and command atomicity

Every mutating semicolon-delimited command is one batch. Its Commit payload is:

1. Batch identifier as monotonic `u64`.
2. First batch-member sequence as `u64`.
3. Last batch-member sequence as `u64` (zero for an empty member range).
4. Batch-member count as ULEB128.

Member records are contiguous and immediately precede their Commit. The range,
count, and physical records MUST agree. A multi-posit command includes every new
metadata and Posit record it needs in one batch. Either the Commit and every
member are visible, or none are. Earlier committed commands survive a later
failure. Read-only commands produce no Commit.

For a persistent store, success may be acknowledged only after all member
records and the Commit have been written and the active log flushed through the
Commit. Any manifest, newly created segment, or directory entry required to
reopen that Commit must also be flushed first. In-memory execution makes no disk
durability claim. In-memory indexes are updated only after durable append
succeeds, unless the implementation demonstrates complete rollback.

## 11. Replay and recovery

Replay starts with empty keepers and indexes, verifies the manifest and every
header, then scans frames in sequence order. Records are staged per batch and
become visible only after a valid Commit. Replay MUST validate:

- canonical frame lengths, checksums, sequence continuity, types, versions, and
  flags;
- immutable and unique role/codec definitions;
- sorted appearance sets with unique roles;
- all role and codec references;
- literal decoding and exact render round-trips;
- valid time fields;
- proposition deduplication and canonical Posit identity; and
- Commit ranges and counts.

Writable recovery may truncate only bytes after the last valid Commit. An
incomplete frame or bad checksum after that Commit is a torn, uncommitted tail.
A read-only open may ignore the same tail without changing files.

Any malformed record, checksum failure, sequence gap, dangling reference,
unknown mandatory version/codec, conflicting definition, or identity mismatch
inside committed history fails startup. The error MUST include the segment,
byte offset, and record sequence when available. Normal startup never skips a
record or serves a prefix of committed history. A future offline salvage tool
may expose a valid prefix read-only and MUST NOT overwrite its source.

Replaying the same committed bytes MUST rebuild identical Things, Roles,
AppearanceSets, Posits, duplicate maps, identity-generator lower bound, and
query indexes regardless of host platform.

## 12. Backup, import, and evolution

A physical backup holds the store backup/read lock and copies the manifest plus
every listed segment through its manifest-recorded committed length. Tail bytes
are excluded. The manifest and logs are one backup unit.

Logical export represents every Thing as `(store UUID, local u64)`. Import into
another store allocates a collision-free local identity for every foreign
identity and rewrites every internal reference through one complete remap table.
It never retains a foreign local number merely because it is currently unused.

Each beta engine reads its current format and at least the immediately preceding
beta format. A breaking migration writes a new store beside the source, replays
and validates it completely, and activates it only after success; it never
mutates the old store. Published stepwise migrators remain available. There is no
pre-beta SQLite compatibility boundary (D031); this format starts the published
migration window.

Snapshots, derived indexes, compaction, and immutable segment rotation may be
added later. They are rebuildable optimizations and cannot change the logical
record stream or the semantics above.
