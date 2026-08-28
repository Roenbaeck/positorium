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
