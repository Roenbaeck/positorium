# Beta architecture benchmark baseline

This is an indicative development baseline, not a service-level objective. Run
the full Criterion suite with:

```text
cargo bench --bench benchmark
```

Use `-- --quick` only for a smoke test. The suite covers the durability latency
floor across several atomic batch sizes, physical size at two scales, warm
startup replay, full and selective query shapes, historical selection, and both
dense and sparse bitmap construction.

## 2026-11-06 incremental execution

Eligible searches now use candidate iterators and depth-first nested-loop joins,
with filters applied once their operands exist, short-circuit absence checks,
and incremental projection/distinctness. An unsorted limit stops after its
one-row lookahead. Ordered limits consume all input but retain at most `limit + 1`
projected rows and prepared sort keys in a heap. Numeric and certainty keys are
parsed once per row, using exact values. Temporal reductions remain blocking.
Plans with nominal literal comparisons, effective evidence, or `or add` retain
full evaluation so early exit cannot hide errors or omit promoted identities.

### Method and reproduction

Environment: Apple M1 Pro, 10 logical CPUs, 16 GiB RAM, Darwin 27.2.0 arm64,
`rustc 1.98.0 (88d9e12ae 2026-08-18)`, optimized `--no-default-features` build.
The baseline is commit `8241a26` (the beta.3 milestone plus its first CI runner
fix), with the identical new `benches/incremental.rs` harness. Fixtures are warm,
in-memory databases. Construction and result-count assertions are outside timing;
parsing, planning, query execution, and result construction/cleanup are included.
Compilation and other test runs were kept outside measurement runs.

```sh
# Before replacing materialized execution, with the new harness present:
cargo bench --offline --no-default-features --bench benchmark -- 'incremental/' \
  --sample-size 20 --warm-up-time 0.2 --measurement-time 0.5 \
  --save-baseline before-incremental-execution --noplot

# After the change, with identical fixtures and settings:
cargo bench --offline --no-default-features --bench benchmark -- 'incremental/' \
  --sample-size 20 --warm-up-time 0.2 --measurement-time 0.5 \
  --baseline before-incremental-execution --noplot
```

The 33 cases use 128, 512, and 2,048 rows per role for scans, and 16, 64, and 256
rows per role for joins. Unconstrained joins have `N²` possible rows. Limits are
10. The exact-filter case selects one left-side value before joining; the
distinct case projects only the left value; the absence case has an uncorrelated
right-side witness. First-row cases use a sink that stops on its first row;
their timing includes query cleanup, rather than measuring only callback arrival.

The table reports Criterion **mean latency** at the largest scale. All
[33 results](INCREMENTAL_BENCHMARK_RESULTS.json) include 95% confidence intervals,
relative changes, environment, and hashes of the measured source. Raw samples
remain under `target/criterion/incremental`. Each case has 20 samples, 0.2 seconds
of warm-up, and a requested 0.5-second measurement window, extended by Criterion
when necessary.

| Workload | Scale per role | Materialized | Incremental | Speedup |
| --- | ---: | ---: | ---: | ---: |
| Scan, limit 10 | 2,048 | 0.941 ms | 0.0121 ms | 78.0× |
| Scan, first row and stop | 2,048 | 0.981 ms | 0.00696 ms | 140.9× |
| Scan, all rows | 2,048 | 0.983 ms | 0.957 ms | 1.03× |
| Scan, ordered top 10 | 2,048 | 1.555 ms | 1.327 ms | 1.17× |
| Fallible-comparison control, limit 10 | 2,048 | 1.298 ms | 1.325 ms | 0.98× |
| Cartesian join, limit 10 | 256 | 63.859 ms | 0.0210 ms | 3,041.8× |
| Cartesian join, first row and stop | 256 | 64.284 ms | 0.0199 ms | 3,233.9× |
| Join with early exact filter | 256 | 46.746 ms | 0.455 ms | 102.8× |
| Join, distinct left values, limit 10 | 256 | 72.583 ms | 2.150 ms | 33.8× |
| Join, ordered top 10 | 256 | 287.991 ms | 70.251 ms | 4.10× |
| Absence with an early witness | 256 | 27.491 ms | 0.331 ms | 83.1× |

The large ratios come from avoiding most of a Cartesian result and do not
describe a general engine-wide speedup. Full scans are nearly unchanged. The
comparison control still uses materialization and was about 2.1% slower at the
largest size (95% relative-change interval: +0.2% to +3.8%); it receives no early
limit benefit. First-row and exact-filter measurements had wider confidence
intervals in this run; the JSON preserves those intervals rather than hiding the
variation. An initial top-k implementation regressed on monotone numeric input;
preparing exact sort keys once per row removed the repeated parsing responsible
for that regression. The table uses the final implementation throughout.

Correctness checks include 52 query/modifier comparisons against the original
materialized executor, stable ordering across exact numeric ties, union and bag
semantics, temporal/external-binding joins, distinct lookahead, empty and zero
limits, maximum-size limits, option caps, cancellation, late comparison errors,
fallback identity promotion, and native/HTTP/SSE/WASM framing. Deterministic
tests verify three candidate visits for a two-pattern `limit 1` query, one
absence witness per outer row, and a top-k buffer of at most `k + 1` rows.

Peak RSS, total query memory, and production per-operator counters were not
measured. The heap bound excludes distinct seen-state, bitmap candidate postings,
sorted full-scan identities, and temporal reduction buffers. Hard work/memory
budgets, hash joins, effective-slice indexing/cache bounds, and incremental
validation for currently materialized plans remain roadmap work.

## 2026-11-06 shared temporal reduction

The shared reducer replaces pairwise dominance scans in ordinary and
latest-matching snapshots, effective target/assertion maxima, and Terrain's
current frame. It groups query rows by exact appearance set and finds a witness
with the greatest **interval start**, then checks each candidate against that
witness. A single witness suffices to test whether any later interval exists;
the result still retains every equal or incomparable maximum, in input order,
with duplicate bindings intact. Retractions use the same rule within each
positor/target pair before either effective-evidence reduction.

Each grouped reduction performs `2(n - groups)` temporal ordering comparisons
and uses `O(n + groups)` auxiliary memory. Terrain already has grouped records,
so its per-group reduction needs only a constant-sized witness beyond its output.
Cancellation and comparison limits apply during both passes. The effective
comparison counter now includes interval-start comparisons used to select a
witness; it measures executed work, so optimized requests can fit budgets that
the pairwise implementation exhausted. The stored time representation, cutoff
semantics, and result contracts are unchanged.

### Method and reproduction

Environment: Apple M1 Pro, 10 logical CPUs, 16 GiB RAM, Darwin 27.2.0 arm64,
`rustc 1.98.0 (88d9e12ae 2026-08-18)`, optimized build with
`--no-default-features`. Databases are in memory and warm. The identical
benchmark harness was run first with the pairwise reducers after the initial
typed-plan implementation, then with the shared reducer. Compilation and other
test runs were kept outside the measurement runs.

```sh
# Run before changing the reducer to capture the reference.
cargo bench --offline --no-default-features --bench benchmark -- 'temporal/' \
  --sample-size 20 --warm-up-time 0.2 --measurement-time 0.5 \
  --save-baseline before-shared-temporal --noplot

# Run after the change, on the same host and with the same fixtures.
cargo bench --offline --no-default-features --bench benchmark -- 'temporal/' \
  --sample-size 20 --warm-up-time 0.2 --measurement-time 0.5 \
  --baseline before-shared-temporal --noplot
```

The 45 cases in `benches/temporal.rs` cover 128, 512, and 2,048 input rows.
Fixture construction and result-count assertions run outside the timed loop;
normal query parsing/planning, materialization, and result creation are included.
For evidence cases, the size is the assertion count. Ordered target histories
have one retained row, while equal and overlapping target histories retain
every row. The retraction case has old zero-certainty assertions followed by
later reassertions, forcing the original implementation to exhaust its
retraction lists. The independent-set control has no dominance work within a
group. Each case verifies its expected result count before timing.

These are Criterion **mean latencies**, with 20 samples, 0.2 seconds of warm-up,
and a requested 0.5-second measurement window (Criterion extends it when needed).
[All 45 measurements](TEMPORAL_BENCHMARK_RESULTS.json) include 95% confidence
intervals for the means and the effective-evidence comparison counters. Raw
Criterion samples remain under `target/criterion/temporal_*`. The table shows
the largest scale, 2,048 rows:

| Workload | Pairwise | Shared reducer | Speedup |
| --- | ---: | ---: | ---: |
| Snapshot / independent sets | 54.349 ms | 1.568 ms | 34.7× |
| Snapshot / ordered history | 68.385 ms | 0.554 ms | 123.4× |
| Snapshot / equal times | 82.433 ms | 1.293 ms | 63.7× |
| Snapshot / overlapping precision | 81.295 ms | 1.291 ms | 63.0× |
| Snapshot / skewed history | 60.675 ms | 0.663 ms | 91.5× |
| Latest matching / ordered history | 70.734 ms | 0.796 ms | 88.9× |
| Terrain / ordered history | 43.689 ms | 0.257 ms | 170.0× |
| Terrain / equal times | 9.716 ms | 0.242 ms | 40.2× |
| Terrain / overlapping precision | 23.028 ms | 0.276 ms | 83.4× |
| Effective evidence / independent sets | 1.343 ms | 0.864 ms | 1.6× |
| Effective evidence / ordered target history | 54.321 ms | 0.458 ms | 118.6× |
| Effective evidence / equal target times | 9.512 ms | 0.817 ms | 11.6× |
| Effective evidence / overlapping target precision | 22.907 ms | 0.818 ms | 28.0× |
| Effective evidence / assertion history | 60.297 ms | 0.566 ms | 106.5× |
| Effective evidence / older retractions | 44.016 ms | 0.488 ms | 90.3× |

All measured cases improved in this run. The large ratios describe these
history-heavy shapes, rather than a general speedup for every query. For 2,048
equal target times, temporal comparisons fell from 4,192,256 to 4,094; for the
retraction case they fell from 2,620,927 to 6,139. Regression tests compare the
kernel against the public pairwise temporal relation and enforce linear work
budgets independently of machine timing.

These temporal measurements predate incremental execution and do not cover peak
memory, first-row latency, or cache eviction. The incremental measurements above
cover early limits and first-row delivery. Effective-slice indexing and
per-operator runtime profiling remain separate roadmap work.

## 2026-08-27 local baseline

Environment: Darwin arm64, stable `rustc 1.95.0`, optimized Criterion build in a
local `/private/tmp` copy, quick sampling mode. Values below are the reported
point estimates and should not be treated as latency percentiles.

### Storage

| Measurement | Scale | Result |
| --- | ---: | ---: |
| Durable atomic append | 1 posit | 13.1 ms; 76.6 posits/s |
| Durable atomic append | 10 posits | 14.0 ms; 717 posits/s |
| Durable atomic append | 100 posits | 15.4 ms; 6.49k posits/s |
| Durable atomic append | 1,000 posits | 22.9 ms; 43.7k posits/s |
| Physical store size | 10,000 posits | 612,288 bytes; 61.23 bytes/posit |
| Physical store size | 100,000 posits | 6,192,289 bytes; 61.92 bytes/posit |
| Warm replay/startup | 10,000 posits | 24.8 ms; 23.6 MiB/s |
| Warm replay/startup | 100,000 posits | 413 ms; 14.3 MiB/s |

Every append result includes the durable log write, atomic manifest replacement,
and directory synchronization. The nearly fixed 13 ms at small batch sizes is
therefore the expected durability floor, not parser throughput. Store bytes
include the manifest, lock file, log header, built-in role/codec metadata,
frames, and commit records.

### Query

| Measurement | Database / result scale | Result |
| --- | ---: | ---: |
| Full scan, literal projection, and order | 100,000 / 100,000 | 77.5 ms; 1.29M rows/s |
| Selective named role | 100,100 / 100 | 49.7 µs |
| Selective natural join | 100,100 / 100 | 254 µs |
| Same join, unselective pattern written first | 100,100 / 100 | 257 µs |
| Correlated `not exists` | 100,100 / 0 | 278 µs |
| Ordinary historical `as of` | 1,000 / 100 | 6.99 ms |
| Latest-matching historical `as of` | 1,000 / 100 | 7.34 ms |

The selective fixture contains 100,000 common-role posits and 100 rare-role
posits. Natural-join planning chooses the lower-cardinality indexed pattern
independently of source order. The history fixture contains ten versions for
each of 100 appearance sets; its results expose the intentionally still-simple
historical maximum algorithm.

### Result sets

| Layout | Scale | Build time | Represented bytes (at least) |
| --- | ---: | ---: | ---: |
| Dense sequential | 1,000 | 14.1 µs | 2,084 |
| Dense sequential | 100,000 | 602 µs | 16,476 |
| Dense sequential | 1,000,000 | 8.12 ms | 131,276 |
| Sparse deterministic | 1,000 | 110 µs | 22,064 |
| Sparse deterministic | 100,000 | 12.3 ms | 2,200,064 |

“At least” includes the `ResultSet` value plus the Roaring serialization size;
allocator bookkeeping is platform-specific and excluded. The sparse case is a
deliberately adverse distribution and shows why the dense result alone is not a
general memory estimate.

The replay measurements are warm-cache results because portable Criterion code
cannot reliably evict operating-system caches. Peak resident memory and true
cold-start measurements require an external process harness and remain separate
operational measurements. Use full Criterion sampling, and record the host,
Rust version, data scale, cache state, and feature set before making a performance
decision.
