# Positorium Roadmap

This roadmap prioritizes a stable external beta of the core posit engine. The
beta does not need to implement every formalism from "Transitional
Representation: A Formalism for Conflicting and Evolving Information", but its
published language, storage, and API contracts must be sound enough to evolve
without silently changing existing data or queries.

## Beta Definition

The first beta is a single-process, single-writer transitional database with:

- immutable things, roles, appearances, appearance sets, and posits;
- a versioned manifest and interleaved append-only log for catalog metadata,
  posits, and commit frames as the durable source of truth;
- deterministic replay into in-memory keepers and bitmap indexes;
- specified Traqula binding, temporal, and result semantics;
- a trusted/local HTTP interface and an in-memory WASM build; and
- explicit migration rules for every published on-disk beta format.

Beta does not mean an Internet-facing, production-ready service. Authentication,
replication, distributed execution, and container packaging are outside this
milestone unless they become necessary for a concrete deployment.

All beta-gating decisions D001-D031 in `DECISIONS.md` were accepted on
2026-08-26. The unchecked items below track remaining specification,
implementation, documentation, and test work; decision acceptance alone does not
complete them. Decision identifiers are included where they clarify the governing
contract.

## P0: Defects Found In Review (2026-08-20)

Concrete issues observed in the current code while annotating `DECISIONS.md`.
The accepted decisions now define the required fixes.

- [x] **Equality/ordering law violations in core constructs.**
        - `Role::Hash` hashes `name.to_uppercase()` and includes `reserved`, while
            `PartialEq` compares `name` case-sensitively only (src/construct.rs). Equal
            roles that differ in `reserved` hash differently, breaking `Eq`/`Hash`.
        - `Posit` derives `Ord`/`PartialOrd` over all fields including the identity
            `posit: Thing`, which the manual `PartialEq` excludes (src/construct.rs).
        - `TimeType` derives `Ord` but hand-implements `PartialOrd`; the two disagree
            (derived variant order versus cross-precision comparison), so sorting and
            `<` give different answers. `partial_cmp` also returns `Equal` for values
            `PartialEq` considers unequal, e.g. `Year(2024)` versus any date in 2024
            (src/datatype.rs). Relevant to D007/D008.
- [x] **Value parsing/formatting bugs.**
        - `Certainty` `Display` omits zero padding: alpha 5 renders as `0.5` instead
            of `0.05` (and `-0.5` for `-0.05`) (src/datatype.rs).
        - `parse_certainty("1%")` yields 100%: after stripping `%`, values with
            absolute value <= 1 are treated as fractions (src/traqula.rs).
        - `Time` `Display` for year-month lacks zero padding (`2024-5`), so the
            persisted canonical text differs from the accepted `YYYY-MM` input form
            (src/datatype.rs).
- [x] **Silent data loss and partial state on restore.**
        - `restore_posits` silently skips unknown value types and has no arms for
            `NaiveDateTime`/`NaiveDate` although both implement `DataType` (UIDs 3/4),
            so such posits vanish on restart (src/persist.rs). Relevant to D025/D026.
        - `Database::new` only logs warnings when `restore_things`/`restore_roles`/
            `restore_posits`/`verify_integrity` fail and continues with partial state
            (src/construct.rs).
        - `verify_integrity` rewrites `LedgerHead` as a side effect of verification;
            `current_superhash` unwraps connection/query errors (src/persist.rs).
- [x] **Panic paths reachable from scripts.**
        - `RoleKeeper::get` unwraps an unknown role name; `add posit` with an unadded
            role panics (src/construct.rs, src/traqula.rs).
        - `Database::create_appearance_set` unwraps `AppearanceSet::new`, panicking on
            a duplicate role in one appearance set (src/construct.rs).
        - `Lookup::lookup` and `ThingLookup::lookup` unwrap missing keys
            (src/construct.rs).
- [x] **Where-clause numerics go through binary floating point.**
        - `cmp_numeric` compares via `f64` with a `1e-9` epsilon equality
            (src/traqula.rs), contradicting the intended exact typed comparison (D017).
- [x] **`@NOW` is evaluated per occurrence.**
        - `parse_time_constant` constructs `Time::new()` at every parse site, so two
            `@NOW`s in one script differ (src/traqula.rs). Resolve it once per complete
            script, allow an execution-option override, and expose the resolved value
            in execution metadata (D011).
- [x] **Server/interface gaps.**
        - `timeout_ms` is accepted and ignored (`let _timeout`, src/server.rs);
            `QueryOptions::timeout` is only checked for zero before start
            (src/interface.rs).
        - CORS default is `allow_origin(Any)` (src/server.rs); tighten for the
            trusted/local beta posture (D029).
- [x] **Repository hygiene.**
        - Stray `Cargo 2.toml` at the repository root is a stale copy of an older
            manifest; delete it.
        - `.github/copilot-instructions.md` refers to `PositoriumError`, but the
            error type is `DatabaseError` (src/error.rs); update the instructions.

## P0: Semantic Contracts

- [x] **Specify the core model independently of Traqula and persistence.**
        - Define identity, equality, canonicalization, and ordering for every construct.
        - Formalize an appearance set as a finite partial function from Role to Thing,
            and a posit proposition as `(AppearanceSet, LiteralValue, Time)` with a
            separately assigned Thing identity.
        - Define role equality by immutable role identity while enforcing one
            canonical catalog name per role and one role per canonical name.
        - Make role names case-sensitive and NFC-normalize them at parser and catalog
            boundaries. A rename creates a new role; aliases are ordinary posits (D001).
        - Make `Eq`, `Hash`, and `Ord` obey the same logical identity rules. In
            particular, posit ordering must not include fields excluded from posit
            equality, and role hashing must not include mutable/non-identity metadata.
        - State which invariants are enforced on write and which are query policies.
- [x] **Define appearance-set cardinality and value-slot semantics.**
        - State that an appearance set contains at most one Thing for each Role and
            therefore models named positions rather than repeatable labels (D005).
        - Treat each exact appearance set as one value-bearing transition slot over
            time; later values do not mutate earlier posits.
        - Document that simultaneous multi-valued attributes, repeated participants,
            and symmetric collections require reified relation/membership identities.
        - Add canonical modeling examples for aliases, tags, memberships, and n-ary
            relations with repeated participant roles.
- [x] **Implement the WYSIWYG literal-value contract.**
        - Preserve the user-visible literal and its expressed precision, scale,
            spelling, or structure independently of its physical encoding.
        - Preserve the complete UTF-8 value token as identity-bearing, including
            leading zeros, explicit signs, string escapes, and JSON whitespace, key
            order, number spelling, and escape choices. Exclude comments and whitespace
            outside the token, and do not normalize literal Unicode (D002).
        - Require `render(decode(encode(literal))) = literal` for every codec and
            exclude codec choice, integer width, and compression from posit identity.
        - Keep user-facing datatypes and casts out of the core model. Use constraints
            to express contextual conformance requirements.
        - Separate exact literal identity, nominal equality (`=`), and compatible
            possible-value overlap (`?=`).
- [x] **Formalize temporal precision and ordering.**
        - Represent every year, year-month, date, and datetime as a half-open UTC
            interval at its stated precision. Do not support local-time/offset coercion
            or leap seconds in beta; treat `@BOT` and `@EOT` as unbounded sentinels
            (D007).
        - Make stored-time equality require identical value and precision. Define `<`,
            `<=`, `>`, and `>=` as conservative definite relations, with explicit
            `possibly before`, `possibly after`, `overlaps`, `contains`, and `within`
            predicates. Indeterminate definite comparisons return false (D008).
        - Define snapshots as all maximal applicable posits, preserving equal-time
            conflicts and incomparable candidates rather than choosing by identity or
            append order. Applicability at a cutoff uses the definite `<=` relation
            (D009).
        - Resolve `@NOW` once per complete script at full supported UTC datetime
            precision; support a deterministic execution-option override and report the
            resolved value in execution metadata (D011).
- [ ] **Keep `as of` as derived Traqula syntax only.**
        - Define it as latest ordinary posit(s) per appearance set at or before a
            cutoff, with identical behavior for literal and variable cutoffs.
        - For ordinary `[pattern] as of T`, select appearance sets by structure, reduce
            each to all maximal applicable posits, and only then apply value and other
            posit-field predicates (D010).
        - Implement `latest [pattern] as of T` for filter-first, latest-matching-history
            semantics. Desugar both forms through grouping and partial-order maximal
            selection in the beta algebra.
        - Add equivalence tests between the shorthand and its expanded query.
        - Do not make `as of` implicitly inspect assertions, positors, or certainty.
- [ ] **Stabilize binding and result semantics.**
        - Review the existing `Binding` evaluator, variable-to-variable comparisons,
            cross-search binding lifetime, multiplicity, projection, and limits.
        - Separate ordered script commands from declarative search semantics; pattern
            evaluation order must not change the meaning of a search.
        - Return typed errors for unknown variables, incompatible variable domains,
            invalid recalls, and unsupported comparisons.
- [x] **Freeze the reserved role vocabulary.**
        - Reserve only `posit` and `ascertains` for beta, with fixed identities and
            names persisted as compatibility data (D006).
        - Treat `thing`, `class`, `classification`, `named`, `subclass`, and
            `superclass` as ordinary roles until a class model is specified.
        - Dedicated keyword syntax is not required for beta.
- [x] **Document external identification.**
        - Explain that identification is a modeling/query concern built from posits,
            not a second mutable key system inside the engine.
        - Forbid destructive identity merging in the beta format; represent proposed
            equivalence using posits and resolve it through explicit query policy.
        - Document equivalence as a reified identification Thing with one membership
            posit per identified Thing and separate evidence/certainty posits. Do not
            add storage-layer aliases or new built-in roles (D004).
        - Define Things as store-local identities and require import/export to carry a
            store UUID and perform explicit collision-free identity remapping.
- [x] **Validate core certainty behavior.**
        - Add boundary and contradiction tests for the signed `[-1, 1]` scale and
            `Certainty::consistent`.

## P0: Traqula Query Algebra

Keep the posit-shaped notation, but define Traqula over a small semantic algebra before
freezing its surface syntax. Every shorthand must have an equivalent expansion in
this algebra.

- [ ] **Define structural variable domains and declarative unification.**
        - Distinguish Thing, Role, Posit, AppearanceSet, LiteralValue, and Time
            variables and reject reuse across incompatible domains.
        - Use one unification rule for query variables: the first occurrence binds and
            repeated occurrences constrain, independent of pattern order.
        - Use `+x` only for allocation in `add`, `?x` for non-allocating query
            variables, and `?p = [...]` for explicit posit-identity binding (D012).
- [x] **Make variable scope explicit.**
        - Scope ordinary query variables lexically to one `search`; migrate tests that
            currently retain search bindings across commands (D013).
        - Keep allocation binders from `add` script-visible to later mutation commands,
            but do not turn them into query variables. Defer `let`, named results, and
            `using` until a concrete post-beta workflow requires them.
        - Keep command order meaningful for scripts without making join order part of
            declarative search semantics.
- [ ] **Distinguish exact and open appearance-set matching.**
        - Make `{(?thing, role)}` exact and a trailing ellipsis, as in
            `{(?thing, role), ...}`, open for subset matching (D014).
        - Bind a complete stored set with `?appearances = { ... }`, allow Role variables
            such as `(?thing, ?role)`, make `*` consume one anonymous field, and make
            `...` permit zero or more additional members.
        - Define grouping for snapshot reduction by the complete stored appearance
            set, even when the query pattern is open.
- [ ] **Implement the beta query-algebra nucleus.**
        - Provide scans, natural joins, selection, projection, union, safe
            anti-join/`NOT EXISTS`, grouping/maximum, distinctness, ordering, and limit.
        - Define `NOT EXISTS` as absence of recorded evidence under an open-world
            model; it must never mean that the matched proposition is false.
        - Defer OPTIONAL/left join and general aggregation until after beta; consider
            `count` first if a concrete need appears (D015).
        - Implement safe `not exists { <patterns> }`: correlated variables must be
            bound outside, inner variables are local, and no inner binding escapes.
            It means absence of recorded evidence, never a false proposition (D016).
- [ ] **Specify literal comparison and lossless result cells.**
        - Return the entered literal representation consistently across Rust, HTTP,
            SSE, and WASM; internal codec IDs are not user-facing result metadata.
        - Define `=` as nominal equality under supported semantic interpretation and
            `?=` as intersection of declared possible-value sets, never a hidden
            epsilon or implementation-defined tolerance.
        - Use `===` for exact literal identity. The legacy `==` spelling means nominal
            `=` only in the explicitly selected compatibility grammar and emits a
            deprecation warning (D017).
        - Compare integers and decimals with exact arbitrary-precision arithmetic;
            compare certainty only with certainty by exact percentage; support string
            equality without normalization but no string ordering in beta.
        - Make JSON nominal equality structural: ignore object key order and
            insignificant whitespace, preserve array order, compare numbers exactly,
            and reject duplicate object keys. Keep exact identity
            presentation-sensitive.
        - Keep constraints conformance-only in beta. Fail unsupported operand pairs,
            including heterogeneous-role pairs, with a typed comparison error.
- [ ] **Specify result cardinality and ordering.**
        - Give joins bag semantics and require explicit `DISTINCT` for duplicate
            elimination (D018).
        - Leave row order unspecified without `ORDER BY`; never promise index or append
            order.
        - Apply filtering, then projection/`DISTINCT`, then `ORDER BY`, then `LIMIT`.
            Set the versioned SSE end event's `limited: true` exactly when more rows
            existed.
- [ ] **Define stable role and literal syntax.**
        - Make role names case-sensitive and NFC-normalized at parser and catalog
            boundaries (D001).
        - Keep identifier-like bare role names and use backticks for literal role names
            containing whitespace, punctuation, or reserved words. Reserve unquoted
            `::`; content inside backticks, including `::`, is always literal (D019).
        - Use `$name` for typed literal/time parameters supplied in a separate API
            object. Parameters never substitute source text, roles, variables, or
            syntax.
        - Provide the D012/D014/D017 legacy Traqula version for one beta minor release,
            with deprecation warnings and mechanical rewrite hints, and update the Pest
            and both VS Code grammars together.

## P0: Append-Only Persistence

The unpublished SQLite prototype is not a compatibility boundary (D031). The
beta uses a format designed for compact sequential writes and deterministic
posit replay.

- [x] **Separate the engine from the storage implementation.**
        - Define a narrow append/replay/flush storage interface that returns all
            durability and corruption errors to the caller.
        - Replace the public `DataType` persistence contract with a storage-neutral
            `LiteralValue`, semantic interpreter capabilities, and private codecs.
        - Remove `rusqlite` types, storage UIDs, and conversion methods from the
            logical value and query interfaces.
- [x] **Write the storage format specification before implementing it.**
        - Define a small manifest containing the immutable store UUID, format version,
            feature flags, and active log-file list. Give every log file a fixed header
            with magic, version, byte order, feature flags, and the same store UUID
            (D003, D023, D024).
        - Use one interleaved append-only log containing all ordered metadata, posit,
            and commit records; allow future rotation into immutable segments.
        - Give every record a type, version, sequence number, unsigned-LEB128 length,
            and CRC32C checksum. Limit a complete framed record to 16 MiB and validate
            its size with checked arithmetic before allocation.
        - Define stable binary encodings for identities, appearance sets, hidden codec
            identifiers, lossless literal payloads, and all time precisions.
        - Use hand-specified endian-independent payloads with little-endian fixed
            integers; never persist Rust memory layouts or `Display` output.
- [x] **Persist catalog metadata in the interleaved log.**
        - Persist role identity, canonical name, reserved status, and record version as
            metadata records in the same log.
        - Preserve the fact that role identities are also Thing identities.
        - Order role and codec metadata before dependent posit records within the same
            command batch. Commit the command atomically, so an unused durable role is
            acceptable but a dangling role reference is not (D021, D023).
        - Specify a closed registry of built-in `(codec identifier, codec version)`
            pairs with immutable decoding semantics. Require a raw UTF-8 token fallback
            for every beta literal family and defer custom codecs (D026).
- [x] **Define the posit log record model.**
        - Persist posit identity, sorted appearances as `(thing, role)` identities,
            hidden value codec identifier, lossless literal payload, and appearance
            time.
        - Permit different physical codecs for small and large values without making
            codec selection observable or proposition-significant.
        - Treat append sequence as physical storage order only, never as appearance
            time, assertion time, truth, preference, or a conflict tie-breaker.
        - Log canonical constructs rather than API calls. Re-adding an existing posit
            is idempotent and does not become new evidence; provenance is modeled with
            assertion posits.
        - Exclude standalone durable Thing allocation from the public beta API. Make an
            identity durable only when a committed role or posit first references it;
            permit gaps but never recycle released identities (D020).
        - Preserve duplicate detection and canonical reconstruction during replay.
- [x] **Define commit and acknowledgment semantics.**
        - Enforce one writer/owner for the complete store with one operating-system
            lock covering the manifest and every active or sealed log (D023).
        - Make each semicolon-delimited command one atomic batch delimited by a commit
            frame. Every role, referenced Thing, and posit created by a multi-posit
            command commits together or not at all; earlier successful commands remain
            committed if a later command fails (D021).
        - Provide one persistent durability level for beta. Return success only after
            the command, required metadata, and commit frame are durably flushed,
            including manifest, new-file, and directory-entry changes needed to reopen
            the commit. In-memory execution makes no disk-durability claim (D022).
        - Update in-memory state only after the durable append succeeds, or provide a
            complete rollback path.
- [x] **Implement deterministic recovery.**
        - On writable recovery, truncate only bytes after the last valid commit frame.
            A read-only open may ignore that uncommitted tail (D025).
        - Fail startup with byte offset and record sequence on checksum failure,
            malformed data, dangling references, or other corruption in committed
            history.
        - Reject unknown mandatory record versions and required codec pairs rather than
            silently skipping data.
        - Rebuild the identity generator, keepers, and all indexes identically on
            every replay.
- [x] **Implement framed-record integrity for the new log.**
        - Verify CRC32C over each complete framed record except its checksum field
            (D024).
        - Do not require a rolling cryptographic chain in beta. Reserve a manifest
            feature flag for a future optional BLAKE3 chain over committed framed bytes
            in sequence order, without calling it tamper-proof absent a trusted anchor.
- [x] **Provide native transfer and inspection tools.**
        - Provide a stable logical export/dump format using `(store UUID, local u64)`
            for external identities. Import must remap every identity and internal
            reference; foreign local identifiers are never retained verbatim except
            for the fixed D006 built-in Role mappings (D003).
        - Remove the unpublished SQLite prototype path and `rusqlite` dependency;
            the first append-only beta format starts the compatibility window (D031).
        - Hold the store's read/backup lock while copying the manifest and logs through
            a recorded committed length; exclude uncommitted tail bytes (D023).
- [x] **Plan format evolution without premature compaction work.**
        - Reserve a path for snapshots, indexes, and compaction without requiring
            them for the first beta.
        - Make every engine read its current format and at least the immediately
            preceding beta format. Keep published standalone migrators available for a
            stepwise path from older beta stores (D027).
        - Make a breaking migration write and validate a new store beside the source,
            never mutate the old files, and activate the new store only after
            validation.

## P0: Reliability And Ownership

- [x] **Propagate boundary failures.**
        - Return persistence, restore, lock, parse, and network errors instead of
            logging and continuing with partial state.
        - Remove panic/`unwrap` paths reachable through scripts, persisted bytes,
            configuration, locks, or network input. Internal proven invariants may use
            assertions rather than blanket replacement.
- [x] **Enforce a single database owner.**
        - Serialize scripts through one worker/command queue so queries cannot observe
            half-updated keepers or indexes.
        - Enforce command-level atomicity: each semicolon-delimited command commits all
            of its changes or none, while earlier commands remain committed after a
            later failure. Defer explicit script transactions until after beta (D021).
        - Add cooperative cancellation points inside query evaluation.
- [x] **Make startup and shutdown explicit.**
        - Fail startup on committed corruption, unsupported formats, or failed replay.
        - Flush according to the single persistent durability contract during graceful
            shutdown (D022).
        - Do not run destructive database recreation by default.

## P0: Compatibility Boundaries

- [x] **Version every external contract.**
        - Version the manifest/log format, Traqula grammar, HTTP endpoint and streaming
            events, WASM interface, and logical export format independently.
        - Use 0.x SemVer: minor releases may contain documented beta breaks and patch
            releases remain compatible. Embed independent storage, Traqula, HTTP/SSE,
            and WASM versions in the applicable files and interfaces (D030).
        - Preserve roles, referenced Things, posits, assertions, exact literal tokens,
            and times through a supported direct or stepwise migration path. Where
            practical, warn for one beta minor release with mechanical migration
            guidance before removing published syntax or APIs.
        - Permit immediate, release-noted changes for security fixes, corruption fixes,
            and behavior that was never part of a published contract (D030).
- [x] **Narrow the Rust public API.**
        - Stabilize only high-level `Database` open/construction and command/query entry
            points; opaque logical/external identity handles; execution options;
            lossless result and stream-event types; storage configuration; and
            `DatabaseError` (D028).
        - Keep keepers, indexes/lookups, `ThingGenerator`, parser/AST/planner internals,
            physical storage owners/records/codecs, and public fields exposing those
            details explicitly unstable.
        - Remove the unreleased `create_apperance` compatibility shim while narrowing
            the API; no release exposed it, so no deprecation window is needed (D031).
- [x] **Unify structured results.**
        - Use one lossless literal result model for Rust, HTTP, streaming, and WASM
            instead of mixing normalized values, datatype names, and tab-separated text.
- [x] **Stabilize physical codec identifiers.**
        - Record the closed hidden registry of immutable `(codec identifier, codec
            version)` pairs and add checks that prevent pair reuse (D026).
        - Require every codec to reconstruct the same logical literal regardless of
            storage optimization, migration, or compaction.
        - Require raw UTF-8 fallback support for every beta literal family and hard-fail
            replay on an unknown required codec pair.

## P1: Beta Validation

- [ ] **Storage contract tests.**
        - Round-trip every literal family, presentation edge case, and temporal
            precision exactly across restart and across every applicable codec.
        - Verify that physically different encodings of one literal have identical
            proposition identity and query behavior.
        - Test empty files, duplicate records, unsupported versions, malformed
            lengths, checksum failures, and unknown codecs.
        - Simulate truncation at every byte boundary near the log tail and verify that
            only uncommitted tail data can be discarded.
        - Verify metadata-before-dependent-posit ordering in the interleaved log,
            command-level atomicity, strict acknowledgment durability, backup committed
            lengths and identity remapping.
- [ ] **Language contract tests.**
        - Cover mixed time precision, equal-time ties, literal and variable `as of`,
            shorthand expansion equivalence, binding multiplicity, and deterministic
            result ordering where promised.
        - Cover exact versus open appearance-set matching, complete set/Role binding,
            variable-domain errors, lexical scope, pattern reordering, safe negation,
            literal identity, nominal `=`, compatible `?=`, DISTINCT, and ordered
            LIMIT behavior.
        - Cover script-scoped and overridden `@NOW`, typed unsupported-comparison
            errors, exact numeric and structural JSON comparison, and the one-minor
            legacy grammar with its rewrite warnings.
- [ ] **Core equality/property tests.**
        - Verify the `Eq`/`Hash` and `Eq`/`Ord` laws for Role, Appearance,
            AppearanceSet, literal values, Time, and Posit.
        - Verify that replay and import preserve proposition identity independently of
            record order and that no store-local identity collisions survive remapping.
- [ ] **Concurrency and failure tests.**
        - Verify single-writer locking, concurrent client serialization, cancellation,
            disk-full/write failure behavior, restart after failure, and clean shutdown.
- [ ] **Fuzz storage and language parsers.**
        - Ensure malformed or arbitrary bytes/scripts produce bounded errors, not
            panics or unbounded allocation.
- [ ] **Benchmark the intended architecture.**
        - Measure append throughput, bytes per posit, replay/startup time, query time,
            and memory usage at representative result-set sizes.

## P1: Server And Distribution

- [x] **Harden the trusted/local HTTP beta.**
        - Make HTTP status codes match response bodies and streaming errors.
        - Implement or remove the advertised timeout and cancellation options.
        - Enforce a 1 MiB default request body with a 16 MiB hard maximum and at most
            1,000 commands per script. Use a 5-second default runtime; let `timeout_ms`
            lower it and configuration raise it only to a 30-second hard maximum
            (D029).
        - Return at most 100,000 rows per search across buffered and streaming
            responses.
        - Bind to `127.0.0.1` by default. Use same-origin CORS, allow only explicitly
            configured exact loopback origins, and forbid wildcard origins.
        - Document that even an explicitly configured non-loopback bind remains a
            trusted interface with no authentication or safe-Internet-exposure claim.
            Return structured errors or a `limited` completion when limits are exceeded.
- [x] **Add multi-platform CI.**
        - Build and test the default, no-persistence/in-memory, and WASM feature sets
            on Linux, macOS, and Windows.
        - Run formatting, linting, unit, integration, doctest, and storage recovery
            checks appropriate to each target.
- [ ] **Automate native releases.**
        - Publish versioned binaries and checksums for supported platforms on tags.
        - Document configuration, backup, restore, migration, and compatibility.
- [ ] **Finish the existing WASM path.**
        - Validate the current feature-gated engine in browsers and package its
            structured API for a zero-install testbed.
        - Host the web console and WASM engine on GitHub Pages after browser smoke
            tests cover representative Traqula scripts.
- [ ] **Keep editor support synchronized.**
        - Test that both copies of `traqula.tmLanguage.json` remain aligned with
            `src/traqula.pest`.
- [ ] **Write beta documentation.**
        - Update `TRAQULA.md` to distinguish history, ordinary snapshots, assertions,
            and assertion-resolution policies.
        - Document exact/open set matching, structural query-variable domains, posit
            identity binding, script versus search scope, open-world negation, and
            result cardinality/order.
        - Explain the WYSIWYG value model, hidden physical codecs, exact retrieval,
            nominal `=`, compatible `?=`, and constraints as the conformance mechanism.
        - Correct examples that use a four-slot posit pattern, describe identity
            unions as role unions, or confuse a leading posit binder with an appearing
            Thing binder.
        - Add a cookbook for correction, disagreement, external identification,
            multi-valued attributes, repeated relation roles, backup, restore, and
            native logical transfer.
        - Document the exact reserved-role vocabulary, command atomicity and durability,
            trust/resource limits, beta compatibility window, and the manifest plus
            interleaved-log backup unit.

## Post-Beta Formalism And Ecosystem

- [ ] **Information in Effect and assertion resolution.**
        - Specify this as an explicit query/library policy with separate appearance
            and assertion cutoffs, selected positors, signed certainty semantics, and
            deterministic conflict/tie handling.
        - Keep manual assertion joins possible and do not redefine ordinary `as of`.
- [ ] **Class layer.**
        - Specify a class model before reserving any vocabulary, then consider
            `named`/`thing`/`class` roles and optional subclass transitive closure.
            Those names remain ordinary roles in beta (D006).
- [ ] **Constraint layer.**
        - Implement subjective cardinality policies and "Decisive Fulfillment" after
            core query and assertion semantics are stable.
- [ ] **Richer Traqula operations.**
        - Add convenience OR syntax, BETWEEN, IN, richer aggregates, subqueries, and
            structured returns as orthogonal features with explicit desugarings where
            possible. Keep the beta algebra stable underneath them.
- [ ] **Storage scaling.**
        - Add snapshots, compaction, incremental indexes, and backup tooling when
            replay measurements demonstrate the need.
- [ ] **Tooling and visualization.**
        - Add role/class completion, an LSP, and assertion-aware temporal
            visualization after those semantics are stable.
- [ ] **Optional container image.**
        - Add an OCI/Docker image only when users need that deployment path. It is a
            packaging convenience, not a beta-readiness requirement.
