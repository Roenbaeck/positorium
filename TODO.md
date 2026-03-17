# Positorium TODO

This document outlines the tasks required to polish Positorium, bringing it closer to the formalisms and features described in the paper "Transitional Representation: A Formalism for Conflicting and Evolving Information".

## Core Engine & Formalism
- [ ] **Native Assertion Handling**: Implement the `Information in Effect` operator as a first-class engine feature.
    - Currently, assertions using reserved roles `posit` and `ascertains` must be manually joined in Traqula queries.
    - The engine should support a global `as of` (assertion time) and automatically filter posits based on the highest certainty from a specified positor or a resolution policy.
- [ ] **Deterministic Temporal Resolution**: Implement the full resolution procedure from the paper for both ordinary posits and assertion posits.
    - Ensure correct handling of the `[-1, 1]` signed certainty scale in the selection logic.
- [ ] **Identity Management**: Formalize the separation between the representation layer and the identification process.
    - While `ThingGenerator` exists, the "external identification" mentioned in the paper (using auxiliary posits as keys) should be better documented or supported via specific query patterns.
- [ ] **Persistence Enhancements**:
    - Ensure the SQLite schema fully supports the `Strict` mode and `WAL` as described.
    - Verify that all `DataType` UIDs are stable and correctly rehydrated in `persist.rs`.

## Traqula Language
- [ ] **Reserved Role Syntax**: Add syntax sugar or dedicated keywords for the reserved roles (`posit`, `ascertains`, `named`, `thing`, `class`).
- [ ] **Class Layer Implementation**:
    - Implement the minimal class layer (`named`, `thing`, `class`) natively in the engine.
    - Support subclass reasoning (transitive closure) at query time.
- [ ] **Constraint Layer**:
    - Implement the cardinality policy mechanism described in the paper (`policy`, `posit class`, `lower bound`, `upper bound`).
    - Add a validation pass or query-time constraint checking for "Decisive Fulfillment".
- [ ] **Variable Binding Refinement**:
    - Complete the "Binding" struct scaffold in `src/traqula.rs` to replace the current projection logic.
    - Support variable-to-variable value comparisons more robustly.

## Tooling & UI
- [ ] **Web Console Polish**:
    - Improve `positorium.html` to better visualize bitemporal data (e.g., a "time travel" slider for both appearance and assertion time).
    - Add better error reporting from the Axum server to the web terminal.
- [ ] **VS Code Extension**:
    - Ensure `traqula.tmLanguage.json` is in sync with `traqula.pest`.
    - Implement LSP features like autocompletion for roles and class names.
- [ ] **Documentation**:
    - Update `TRAQULA.md` with examples of bitemporal queries and assertions.
    - Create a "Cookbook" of common patterns (e.g., how to handle a "correction" of a birth date).

## Quality & Performance
- [ ] **Benchmark Suite**: Extend `benches/benchmark.rs` to measure the overhead of assertion-time filtering and class hierarchy traversal.
- [ ] **Comprehensive Testing**:
    - Add tests for "Information in Effect" with multiple competing positors.
    - Test the `Certainty::consistent` logic against complex contradictory sets.
- [ ] **Error Handling**: Replace remaining `unwrap()` calls in `src/traqula.rs` and `src/construct.rs` with proper `DatabaseError` propagation.

## Exposure & Distribution
- [ ] **Multi-Platform CI**: Setup GitHub Actions to build and test on Linux, macOS, and Windows.
- [ ] **Automated Releases**: Implement a workflow to automatically create GitHub Releases with attached platform-specific binaries on version tags.
- [ ] **WASM Port**:
    - Investigate compiling the core engine to WebAssembly (`wasm32-unknown-unknown`).
    - *Plan*: Put `rusqlite` and `persist.rs` behind a `persistence` feature flag (default enabled).
    - *Plan*: For WASM/In-Memory builds, use `#![cfg(feature = "persistence")]` to exclude SQLite-specific code.
- [ ] **Public Testbed (GitHub Pages)**:
    - Host `positorium.html` and `positorium.css` on GitHub Pages.
    - Connect the UI to the WASM-powered engine for a zero-install, serverless exploration of Traqula.
- [ ] **Dockerization**: Provide a `Dockerfile` for easy deployment of the Positorium HTTP server.
