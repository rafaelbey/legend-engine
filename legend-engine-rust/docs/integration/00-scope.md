# 00 — Scope of `legend-engine-rust`

## What this workspace is

The Rust foundation for integrating the Rust port of Legend Pure
(`../../legend-pure/legend-pure-rust/`) with FINOS legend-engine. Today
this workspace ships:

- `store-relational-pure` — build-time embedding of the
  `platform_store_relational` Pure metamodel (5 `.pure` files: Database,
  Schema, Table, Join, Filter, View, milestoning, DataTypes,
  RelationalOperationElement, DatabaseConnection, RelationalMapping
  types). Uses the existing `legend-pure-build` machinery; references
  the upstream Pure source directly via path-relative descriptor.
- `integration-smoke` — regression-detection lock for the embedded
  metamodel: asserts the Rust compiler produces exactly the cataloged
  set of dispatch errors against `platform + platform_store_relational`,
  no more, no less. The strict compile-clean variant is `#[ignore]`'d
  until those errors are fixed in the legend-pure-rust compiler.

## Boundary: what lives in `legend-pure-rust`, not here

The runtime side of the relational stack — the SQLite adapter and the
relational native function implementations (`executeInDb`,
`loadValuesToDbTable`, `createTempTable`, `dropTempTable`, the
`fetchDb*MetaData` family, `loadCsvToDbTable`, `logActivities`) — lives
in the `legend-pure-rust` workspace. A separate session is taking that
work. This workspace stays focused on the **legend-engine-side**
integration assets: descriptors, embeddings, and locks that prove
upstream Pure source compiles when it flows through the Rust pipeline.

## What this workspace is NOT (yet)

This is **groundwork**. The full integration ("compile and run
legend-engine's `core_relational/**/*.pure` test fixtures through Rust
end-to-end") is a separate, larger effort tracked as a follow-up plan.
Specifically out of scope today:

- Recursive-descent port of `RelationalParser.g4` (the
  `Database <db>(...)` block grammar). The 5 metamodel files we embed
  use only existing M3 grammar and don't require it.
- Mapping-body relational sub-parser
  (`~mainTable [db]Schema.Table`, `~filter [...]`, etc.).
- Embedding of `core_relational/**/*.pure` from the legend-engine repo.
- Execution-plan generation and execution.
- Java parity testing.

## Tests

```sh
cargo test --workspace                                         # all green
cargo test -p legend-engine-rust-store-relational-pure
    # 3 unit tests + 1 doctest — embedded repo metadata + file count
cargo test -p legend-engine-rust-integration-smoke --test relational_metamodel_compiles
    # platform + relational compiles with KNOWN_ERROR_COUNT errors
    # (regression-detection lock; strict variant is #[ignore]'d)
```
