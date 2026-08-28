# Positorium documentation

Start with [Getting started](GETTING_STARTED.md) if you are installing or
evaluating the beta. The documents below separate practical guidance from the
contracts that define compatibility and the design record used by maintainers.

## Guides

| Document | Use it for |
| --- | --- |
| [Getting started](GETTING_STARTED.md) | Install, run, query, restart, troubleshoot, and report beta feedback |
| [Operations](guides/OPERATIONS.md) | Configuration, durability, backup, restore, failure handling, and resource limits |
| [Cookbook](guides/COOKBOOK.md) | Worked modeling, correction, assertion, identification, backup, and transfer recipes |

## Reference

| Document | Contract |
| --- | --- |
| [Traqula](reference/TRAQULA.md) | Language grammar and query semantics |
| [Core model](reference/MODEL.md) | Identity, literal, temporal, snapshot, and classification semantics |
| [Contracts](reference/CONTRACTS.md) | Independent versions and compatibility policy |
| [Storage](reference/STORAGE.md) | Append-only store format and recovery rules |
| [Transfer](reference/TRANSFER.md) | Logical export, import, identity remapping, and physical backup |
| [Terrain](reference/TERRAIN.md) | Structural report and visualization contract |

## Design and development

| Document | Audience |
| --- | --- |
| [Theory](design/THEORY.md) | Conceptual foundations of Transitional Modeling |
| [Roadmap](development/ROADMAP.md) | Completed beta gates and planned work |
| [Decisions](development/DECISIONS.md) | Accepted design decisions and rationale |
| [Extending Traqula](development/EXTEND_TRAQULA.md) | Maintainer workflow for language changes |
| [Benchmarks](development/BENCHMARKS.md) | Repeatable benchmark method and indicative baseline |
| [Terrain backend specification](development/TERRAIN_BACKEND_SPEC.md) | Historical implementation specification for the Terrain backend |

## Releases

- [0.1.4-beta.1 notes](releases/v0.1.4-beta.1.md)

Normative documents say so explicitly. Development plans and historical design
records are informative when they differ from an implemented, versioned
contract.
