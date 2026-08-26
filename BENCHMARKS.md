# Beta architecture benchmark baseline

This is an indicative development baseline, not a service-level objective. Run
the Criterion suite with:

```text
cargo bench --bench benchmark -- --quick
```

The suite measures the intended architecture rather than isolated prototype
helpers: one durable atomic append batch, file-store replay/startup, a typed
Traqula scan/order/projection, and bitmap-backed result-set construction. It also
reports physical bytes per posit and representative serialized result-set sizes.

## 2026-08-26 local baseline

Environment: Darwin arm64, stable `rustc 1.95.0`, optimized Criterion build,
quick sampling mode.

| Measurement | Scale | Result |
| --- | ---: | ---: |
| Durable append, one atomic batch | 1,000 posits | 21.1 ms; 47.4k posits/s |
| Physical store size | 1,000 posits | 61,480 bytes; 61.48 bytes/posit |
| Replay/startup | 10,000 posits | 43.7 ms; 13.4 MiB/s |
| Scan, literal projection, and order | 10,000 rows | 5.71 ms; 1.75M rows/s |
| ResultSet build | 1,000 identities | 13.9 µs; at least 2,084 bytes |
| ResultSet build | 100,000 identities | 609 µs; at least 16,476 bytes |
| ResultSet build | 1,000,000 identities | 8.12 ms; at least 131,276 bytes |

“At least” includes the `ResultSet` value plus the Roaring serialization size;
allocator bookkeeping is platform-specific and excluded. Store bytes include
the manifest, lock file, log header, built-in role/codec metadata, frames, and
commit records, so the per-posit value is deliberately an end-to-end physical
measurement.

Use the full Criterion sampling defaults before making a performance decision.
Record the host, Rust version, data scale, and relevant feature set alongside
future results so regressions are comparable.
