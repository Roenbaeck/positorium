# Positorium Roadmap

This roadmap prioritizes a stable external beta of the core posit engine. The
beta does not need to implement every formalism from "Transitional
Representation: A Formalism for Conflicting and Evolving Information", but its
published language, storage, and API contracts must be sound enough to evolve
without silently changing existing data or queries.

## Beta Definition

The first beta is a single-process, single-writer transitional database with:

- immutable things, roles, appearances, appearance sets, and posits;
- a versioned role catalog and append-only posit log as the durable source of truth;
- deterministic replay into in-memory keepers and bitmap indexes;
- specified Traqula binding, temporal, and result semantics;
- a trusted/local HTTP interface and an in-memory WASM build; and
- explicit migration rules for every published on-disk beta format.

Beta does not mean an Internet-facing, production-ready service. Authentication,
replication, distributed execution, and container packaging are outside this
milestone unless they become necessary for a concrete deployment.

## P0: Defects Found In Review (2026-08-20)

Concrete issues observed in the current code while annotating DECISIONS.md.
Several make existing roadmap items concrete; fix them regardless of which
decision options are accepted.

- [ ] **Equality/ordering law violations in core constructs.**
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
- [ ] **Value parsing/formatting bugs.**
        - `Certainty` `Display` omits zero padding: alpha 5 renders as `0.5` instead
            of `0.05` (and `-0.5` for `-0.05`) (src/datatype.rs).
        - `parse_certainty("1%")` yields 100%: after stripping `%`, values with
            absolute value <= 1 are treated as fractions (src/traqula.rs).
        - `Time` `Display` for year-month lacks zero padding (`2024-5`), so the
            persisted canonical text differs from the accepted `YYYY-MM` input form
            (src/datatype.rs).
- [ ] **Silent data loss and partial state on restore.**
        - `restore_posits` silently skips unknown value types and has no arms for
            `NaiveDateTime`/`NaiveDate` although both implement `DataType` (UIDs 3/4),
            so such posits vanish on restart (src/persist.rs). Relevant to D025/D026.
        - `Database::new` only logs warnings when `restore_things`/`restore_roles`/
            `restore_posits`/`verify_integrity` fail and continues with partial state
            (src/construct.rs).
        - `verify_integrity` rewrites `LedgerHead` as a side effect of verification;
            `current_superhash` unwraps connection/query errors (src/persist.rs).
- [ ] **Panic paths reachable from scripts.**
        - `RoleKeeper::get` unwraps an unknown role name; `add posit` with an unadded
            role panics (src/construct.rs, src/traqula.rs).
        - `Database::create_appearance_set` unwraps `AppearanceSet::new`, panicking on
            a duplicate role in one appearance set (src/construct.rs).
        - `Lookup::lookup` and `ThingLookup::lookup` unwrap missing keys
            (src/construct.rs).
- [ ] **Where-clause numerics go through binary floating point.**
        - `cmp_numeric` compares via `f64` with a `1e-9` epsilon equality
            (src/traqula.rs), contradicting the intended exact typed comparison (D017).
- [ ] **`@NOW` is evaluated per occurrence.**
        - `parse_time_constant` constructs `Time::new()` at every parse site, so two
            `@NOW`s in one script differ (src/traqula.rs). Align with D011 once decided.
- [ ] **Server/interface gaps.**
        - `timeout_ms` is accepted and ignored (`let _timeout`, src/server.rs);
            `QueryOptions::timeout` is only checked for zero before start
            (src/interface.rs).
        - CORS default is `allow_origin(Any)` (src/server.rs); tighten for the
            trusted/local beta posture (D029).
- [ ] **Repository hygiene.**
        - Stray `Cargo 2.toml` at the repository root is a stale copy of an older
            manifest; delete it.
        - `.github/copilot-instructions.md` refers to `PositoriumError`, but the
            error type is `DatabaseError` (src/error.rs); update the instructions.

## P0: Semantic Contracts

- [ ] **Specify the core model independently of Traqula and persistence.**
        - Define identity, equality, canonicalization, and ordering for every construct.
        - Formalize an appearance set as a finite partial function from Role to Thing,
            and a posit proposition as `(AppearanceSet, TypedValue, Time)` with a
            separately assigned Thing identity.
        - Define role equality by immutable role identity while enforcing one
            canonical catalog name per role and one role per canonical name.
        - Make `Eq`, `Hash`, and `Ord` obey the same logical identity rules. In
            particular, posit ordering must not include fields excluded from posit
            equality, and role hashing must not include mutable/non-identity metadata.
        - State which invariants are enforced on write and which are query policies.
- [ ] **Define appearance-set cardinality and value-slot semantics.**
        - State that an appearance set contains at most one Thing for each Role and
            therefore models named positions rather than repeatable labels.
        - Treat each exact appearance set as one value-bearing transition slot over
            time; later values do not mutate earlier posits.
        - Document that simultaneous multi-valued attributes, repeated participants,
            and symmetric collections require reified relation/membership identities.
        - Add canonical modeling examples for aliases, tags, memberships, and n-ary
            relations with repeated participant roles.
- [ ] **Formalize temporal precision and ordering.**
        - Represent year, year-month, date, and datetime precision without discarding
            the uncertainty it conveys; prefer interval/granule semantics over
            silently coercing every value to a precise point.
        - Make equality and ordering consistent across mixed temporal precisions.
        - Define explicit `definitely`, `possibly`, and overlap relations, or reject
            comparisons that are ambiguous under the chosen partial order.
        - Define snapshots as all maximal applicable posits, preserving equal-time
            conflicts and incomparable candidates rather than choosing by identity or
            append order.
- [ ] **Keep `as of` as derived Traqula syntax only.**
        - Define it as latest ordinary posit(s) per appearance set at or before a
            cutoff, with identical behavior for literal and variable cutoffs.
        - Specify operator order: a state query reduces all values for each matching
            appearance set before applying a value predicate. Provide a distinct way
            to ask for the latest historical posit that already matches a value.
        - Add a general query operation, such as grouping/maximum or an anti-join,
            through which the same query can be written without `as of`.
        - Add equivalence tests between the shorthand and its expanded query.
        - Do not make `as of` implicitly inspect assertions, positors, or certainty.
- [ ] **Stabilize binding and result semantics.**
        - Review the existing `Binding` evaluator, variable-to-variable comparisons,
            cross-search binding lifetime, multiplicity, projection, and limits.
        - Separate ordered script commands from declarative search semantics; pattern
            evaluation order must not change the meaning of a search.
        - Specify errors for unknown variables, incompatible types, and invalid recalls.
- [ ] **Freeze the reserved role vocabulary.**
        - Reconcile `classification` in the engine with `class`, `named`, `thing`,
            `posit`, and `ascertains` in the theory and documentation.
        - Treat reserved names and identities as persisted compatibility data.
        - Dedicated keyword syntax is not required for beta.
- [ ] **Document external identification.**
        - Explain that identification is a modeling/query concern built from posits,
            not a second mutable key system inside the engine.
        - Forbid destructive identity merging in the beta format; represent proposed
            equivalence using posits and resolve it through explicit query policy.
        - Define Things as store-local identities and require import/export to carry a
            store UUID and perform explicit collision-free identity remapping.
- [ ] **Validate core certainty behavior.**
        - Add boundary and contradiction tests for the signed `[-1, 1]` scale and
            `Certainty::consistent`.

## P0: Traqula Query Algebra

Keep the posit-shaped notation, but define Traqula over a small typed algebra before
freezing its surface syntax. Every shorthand must have an equivalent expansion in
this algebra.

- [ ] **Define typed variable domains and declarative unification.**
        - Distinguish Thing, Role, Posit, AppearanceSet, `Value<T>`, and Time variables
            and reject reuse of one variable across incompatible domains.
        - Use one unification rule for query variables: the first occurrence binds and
            repeated occurrences constrain, independent of pattern order.
        - Reserve allocation syntax such as `+x` or `new x` for `add`; give search
            variables a non-allocating syntax such as `?x`.
        - Bind a posit identity explicitly, for example `?p = [...]`, so it cannot be
            confused with a Thing variable inside an appearance.
- [ ] **Make variable scope explicit.**
        - Scope ordinary query variables to one search.
        - Replace implicit cross-search binding retention with an explicit `let`,
            named result, subquery, or `using` construct.
        - Keep command order meaningful for scripts without making join order part of
            declarative search semantics.
- [ ] **Distinguish exact and open appearance-set matching.**
        - Provide exact equality and contains/subset modes. A possible notation is
            `{(?thing, role)}` for exact matching and `{(?thing, role), ...}` for an
            open appearance set.
        - Allow a variable to bind the complete appearance set.
        - Allow Role variables, such as `(?thing, ?role)`, for schema-free exploration.
        - Define grouping for snapshot reduction by the complete stored appearance
            set, even when the query pattern is open.
- [ ] **Implement the beta query-algebra nucleus.**
        - Provide typed scans, natural joins, selection, projection, union, safe
            anti-join/`NOT EXISTS`, grouping/maximum, distinctness, ordering, and limit.
        - Define `NOT EXISTS` as absence of recorded evidence under an open-world
            model; it must never mean that the matched proposition is false.
        - Add OPTIONAL/left join only if beta use cases require it; its absence does
            not block the core snapshot algebra.
- [ ] **Specify typed value comparison and result cells.**
        - Preserve datatype identity in every projected cell across Rust, HTTP, SSE,
            and WASM interfaces.
        - Define exact coercion rules. Compare integers and decimals without conversion
            through binary floating point, and do not infer JSON equality from display
            text unless canonical JSON is the declared representation.
        - Make unsupported ordering and cross-type comparisons explicit errors.
- [ ] **Specify result cardinality and ordering.**
        - Choose and document set or bag semantics for joins and projection, with an
            explicit `DISTINCT` operation if bags are retained.
        - Guarantee row order only through an explicit ordering clause.
        - Apply `LIMIT` after filtering, projection/distinctness, and ordering, and
            define how streaming reports that more rows were available.
- [ ] **Define stable role and literal syntax.**
        - Specify role-name normalization and case sensitivity.
        - Add an unambiguous quoted/escaped form for role names, including names with
            spaces, punctuation, or future namespace qualifiers.
        - Separate query parameters from query variables so clients can bind values
            without constructing Traqula source text.

## P0: Append-Only Persistence

SQLite was useful for prototyping and offline inspection, but it will not be the
runtime persistence backend for beta. Replace it with a format designed for
compact sequential writes and deterministic posit replay.

- [ ] **Separate the engine from the storage implementation.**
        - Define a narrow append/replay/flush storage interface that returns all
            durability and corruption errors to the caller.
        - Remove `rusqlite` types and conversion methods from the public `DataType`
            contract.
- [ ] **Write the storage format specification before implementing it.**
        - Give every file a magic value, format version, byte order, and feature flags.
    - Give the database and every member file the same immutable store UUID so a
        role catalog, posit log, snapshot, or index from another store is rejected.
        - Give every record a type, version, sequence number, length, and checksum.
        - Define stable binary encodings for identities, appearance sets, datatype
            identifiers, values, and all time precisions.
        - Never use Rust memory layouts or `Display` output as persisted encodings.
- [ ] **Store roles in a separate append-only catalog.**
        - Persist role identity, name, reserved status, and record version.
        - Preserve the fact that role identities are also Thing identities.
        - Include a monotonic catalog sequence/high-water mark that posit commits can
            reference and validate during replay.
        - Make a catalog entry durable before committing a posit that references it;
            an unused durable role is acceptable, but a dangling role reference is not.
        - Define how datatype metadata and custom datatype codecs are registered and
            versioned, whether in this catalog or another explicit catalog.
- [ ] **Define the posit log record model.**
        - Persist posit identity, sorted appearances as `(thing, role)` identities,
            datatype identifier, typed value bytes, and appearance time.
        - Treat append sequence as physical storage order only, never as appearance
            time, assertion time, truth, preference, or a conflict tie-breaker.
        - Log canonical constructs rather than API calls. Re-adding an existing posit
            is idempotent and does not become new evidence; provenance is modeled with
            assertion posits.
        - Decide whether standalone/unreferenced Thing allocation is durable. If it
            is, include an explicit identity-allocation record; otherwise narrow the
            public persistence promise.
        - Preserve duplicate detection and canonical reconstruction during replay.
- [ ] **Define commit and acknowledgment semantics.**
        - Enforce one writer with an operating-system file lock.
        - Define atomic command or script batches using commit records or an
            equivalent framing rule.
        - Offer explicit durability modes and document when data is flushed with
            `fsync` before success is returned.
        - Update in-memory state only after the durable append succeeds, or provide a
            complete rollback path.
        - Define cross-file atomicity explicitly: a failed batch may leave an unused
            durable catalog entry, but never a committed posit with a missing role.
- [ ] **Implement deterministic recovery.**
        - Ignore or safely truncate only a torn, uncommitted tail record.
        - Refuse to serve on checksum failure or corruption in committed history.
        - Reject unknown mandatory record versions and unknown datatypes rather than
            silently skipping data.
        - Rebuild the identity generator, keepers, and all indexes identically on
            every replay.
- [ ] **Redesign the integrity chain for the new log.**
        - Decide whether per-record checksums are sufficient or whether a rolling
            hash is also required.
        - If retained, hash every committed record in explicit sequence order rather
            than hashing a textual projection of posit rows.
- [ ] **Provide migration and inspection tools.**
        - Build a one-way importer from the prototype SQLite schema to the first log
            format and verify role/thing/posit counts plus representative queries.
        - Provide a stable logical export/dump format for offline inspection and
            future migrations; the physical log need not be pleasant to query directly.
        - Keep SQLite support only in the importer until the migration window closes.
        - Treat the manifest, catalog, and log as one backup unit; take backups under
            a consistent read lock or a documented snapshot protocol.
- [ ] **Plan format evolution without premature compaction work.**
        - Reserve a path for snapshots, indexes, and compaction without requiring
            them for the first beta.
        - Require a migration or compatibility reader whenever the format version
            changes.

## P0: Reliability And Ownership

- [ ] **Propagate boundary failures.**
        - Return persistence, restore, lock, parse, and network errors instead of
            logging and continuing with partial state.
        - Remove panic/`unwrap` paths reachable through scripts, persisted bytes,
            configuration, locks, or network input. Internal proven invariants may use
            assertions rather than blanket replacement.
- [ ] **Enforce a single database owner.**
        - Serialize scripts through one worker/command queue so queries cannot observe
            half-updated keepers or indexes.
        - Specify command and multi-command script atomicity.
        - Add cooperative cancellation points inside query evaluation.
- [ ] **Make startup and shutdown explicit.**
        - Fail startup on committed corruption, unsupported formats, or failed replay.
        - Flush according to the selected durability mode during graceful shutdown.
        - Do not run destructive database recreation by default.

## P0: Compatibility Boundaries

- [ ] **Version every external contract.**
        - Version the log/catalog format, Traqula grammar, HTTP endpoint and streaming
            events, WASM interface, and logical export format independently.
        - Publish which contracts are beta-stable and how deprecations are handled.
- [ ] **Narrow the Rust public API.**
        - Put keepers, lookups, and persistence details behind stable database and
            query interfaces, or clearly mark them unstable.
        - Correct public naming mistakes such as `create_apperance` through a planned
            deprecation rather than an unannounced break.
- [ ] **Unify structured results.**
        - Use one typed result model for Rust, HTTP, streaming, and WASM instead of
            mixing structured rows with tab-separated text.
- [ ] **Stabilize datatype identifiers and codecs.**
        - Record the built-in UID registry and add checks that prevent reuse.
        - Define how datatype codec versions are migrated and how unsupported custom
            datatypes fail during replay.

## P1: Beta Validation

- [ ] **Storage contract tests.**
        - Round-trip every datatype and temporal precision across restart.
        - Test empty files, duplicate records, unsupported versions, malformed
            lengths, checksum failures, and unknown datatypes.
        - Simulate truncation at every byte boundary near the log tail and verify that
            only uncommitted tail data can be discarded.
        - Verify role-catalog-before-posit ordering and SQLite import fidelity.
- [ ] **Language contract tests.**
        - Cover mixed time precision, equal-time ties, literal and variable `as of`,
            shorthand expansion equivalence, binding multiplicity, and deterministic
            result ordering where promised.
        - Cover exact versus open appearance-set matching, complete set/Role binding,
            variable-domain errors, lexical scope, pattern reordering, safe negation,
            typed coercion, DISTINCT, and ordered LIMIT behavior.
- [ ] **Core equality/property tests.**
        - Verify the `Eq`/`Hash` and `Eq`/`Ord` laws for Role, Appearance,
            AppearanceSet, typed values, Time, and Posit.
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
        - Compare with the SQLite prototype to validate the backend switch, but do not
            make SQLite parity a release dependency.

## P1: Server And Distribution

- [ ] **Harden the trusted/local HTTP beta.**
        - Make HTTP status codes match response bodies and streaming errors.
        - Implement or remove the advertised timeout and cancellation options.
        - Define request/result limits, CORS defaults, and the versioned SSE schema.
        - Keep loopback binding as the secure default and document that authentication
            is not included in the first beta.
- [ ] **Add multi-platform CI.**
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
        - Document exact/open set matching, typed query variables, posit identity
            binding, script versus search scope, open-world negation, and result
            cardinality/order.
        - Correct examples that use a four-slot posit pattern, describe identity
            unions as role unions, or confuse a leading posit binder with an appearing
            Thing binder.
        - Add a cookbook for correction, disagreement, external identification,
            multi-valued attributes, repeated relation roles, backup/restore, and
            SQLite migration.

## Post-Beta Formalism And Ecosystem

- [ ] **Information in Effect and assertion resolution.**
        - Specify this as an explicit query/library policy with separate appearance
            and assertion cutoffs, selected positors, signed certainty semantics, and
            deterministic conflict/tie handling.
        - Keep manual assertion joins possible and do not redefine ordinary `as of`.
- [ ] **Class layer.**
        - Implement the agreed `named`/`thing`/`class` model and optional subclass
            transitive closure after the reserved vocabulary is stable.
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
