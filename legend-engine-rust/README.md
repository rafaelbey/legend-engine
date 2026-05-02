# legend-engine-rust

Rust workspace for the **legend-engine** integration with the Rust port of
Legend Pure. Path-deps onto the sibling `../../legend-pure/legend-pure-rust`
workspace; nothing here forks the Pure runtime — we consume it as a
library.

## Status

Groundwork phase. The goal of this workspace today is to prove the
foundations are in place for a larger integration effort (relational DSL
grammar port, mapping-body sub-parser, plan generation, end-to-end Pure
test execution against legend-engine sources). Tracking under
`docs/integration/`.

## Crates

| Crate | Purpose |
|-------|---------|
| `store-relational-pure` | Build-time embedding of `platform_store_relational` (the 5 metamodel `.pure` files from upstream `legend-pure-store-relational`). Uses `legend-pure-build`. |
| `integration-smoke`     | Regression-detection lock that the embedded relational metamodel still compiles to the cataloged set of dispatch errors — no more, no less. Strict compile-clean variant is `#[ignore]`'d until those errors are fixed. |

## Out of scope here (moved to `legend-pure-rust`)

The SQLite adapter and the relational native function implementations
(`executeInDb`, `loadValuesToDbTable`, `createTempTable`, `dropTempTable`,
the `fetchDb*MetaData` family, `loadCsvToDbTable`, `logActivities`) live
in the `legend-pure-rust` workspace, not here. This workspace is for
**legend-engine-side** integration assets: descriptors, embeddings, and
locks that prove the upstream Pure source compiles when consumed.

## Build

```sh
cd legend-engine-rust
cargo build --workspace
cargo test  --workspace
```

The path-deps point at `../../legend-pure/legend-pure-rust`. Both projects
must be checked out as siblings under a common parent for builds to
resolve. CI / packaging story is deferred to a follow-up plan.

## Out-of-scope (deferred)

See `docs/integration/03-deferred-workstreams.md` for the full list. Major
items not in this workspace yet:

- Port of `RelationalParser.g4` (the `Database <db>(...)` grammar).
- Mapping-body relational sub-parser (`~mainTable`, `~filter`, etc.).
- Embedding of `core_relational/**/*.pure` from the legend-engine repo.
- Execution-plan generation and execution.
- Java parity testing.
