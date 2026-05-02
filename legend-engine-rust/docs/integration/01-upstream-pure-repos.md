# 01 — Upstream Pure repos to integrate

This catalogues the upstream Pure sources we'll need to consume to
reach the full-integration milestone (running legend-engine's
relational mapping tests through Rust end-to-end). Today this workspace
embeds only the first repo in this list; the rest are deferred.

## Embedded today

### `platform_store_relational` ✅

Source: `../../legend-pure/legend-pure-store/legend-pure-store-relational/legend-pure-m2-store-relational-pure/src/main/resources/`

Descriptor:
```json
{
  "name": "platform_store_relational",
  "pattern": "(meta::relational|meta::external::store::relational|meta::pure::functions::io)(::.*)?",
  "dependencies": ["platform", "platform_dsl_mapping", "platform_dsl_store"]
}
```

Files (5):

| File | Role |
|------|------|
| `grammar/relational.pure` | Core M2 metamodel — Database, Schema, Table, View, Column, Join, Filter, RelationalOperationElement, DataType hierarchy, milestoning, SelectSQLQuery (~605 lines) |
| `grammar/relationalMapping.pure` | Mapping types — RootRelationalInstanceSetImplementation, RelationalPropertyMapping, FilterMapping, GroupByMapping (~83 lines) |
| `functions.pure` | Native function declarations — `executeInDb`, `loadValuesToDbTable`, `createTempTable`, `dropTempTable`, `fetchDb*MetaData` (×5), `loadCsvToDbTable` (~300 lines) |
| `relationalRuntime.pure` | Runtime types — `enum DatabaseType` (24 dialects), `DatabaseConnection`, `PostProcessor` (~100 lines) |
| `runtimeLogging.pure` | Activity-tracking native (`logActivities`) (~16 lines) |

## Deferred

### `core_relational` (legend-engine — heavy Pure code)

Source: `../legend-engine-xts-relationalStore/legend-engine-xt-relationalStore-generation/legend-engine-xt-relationalStore-pure/legend-engine-xt-relationalStore-core-pure/src/main/resources/core_relational/`

What lives here: the **execution plan model**, **SQL generation pipeline**,
and **mapping execution** — all written in Pure. This is the body of
code that does the SQL-emission and plan-routing work. Embedding it in
Rust is what unlocks the "Rust runs the Pure code that generates SQL
and routes execution" model assumed by the integration plan.

Major files (non-exhaustive):

| File | Role |
|------|------|
| `relational/executionPlan/executionPlan.pure` | ExecutionNode hierarchy: SQLExecutionNode, RelationalTdsInstantiationExecutionNode, FunctionParametersValidationNode, etc. |
| `relational/pureToSQLQuery/pureToSQLQuery.pure` | The SQL generator — Pure → dialect-specific SQL strings |
| `relational/relationalMappingExecution.pure` | Mapping execution glue — wires plan nodes through the runtime |
| `relational/relationalExtension.pure` | Extension SPI — `relationalExtension():Extension` |
| `relational/storeContract.pure` | Store-contract interface for relational |
| `relational/tests/mapping/boolean.pure` | The MVP fixture: `Interaction.all()` returning 14 rows |

### `legend-engine-xt-relationalStore-test` (legend-engine — fixture sources)

Source: `../legend-engine-xts-relationalStore/legend-engine-xt-relationalStore-generation/legend-engine-xt-relationalStore-pure/legend-engine-xt-relationalStore-test/src/main/resources/core_relational_test/`

Test sources used by legend-engine's test runners. Embedded once the
follow-up plan reaches end-to-end testing.

## Composition

When all three are embedded, the `Vec<Repo>` for a full compile becomes:

```rust
let mut repos: Vec<Repo> = Repo::default_embedded();        // 7 platform repos
repos.extend(legend_engine_rust_store_relational_pure::repos());  // +1: M2 metamodel
repos.extend(legend_engine_rust_core_relational_pure::repos());   // +1: SQL gen + plan + mapping execution
repos.extend(legend_engine_rust_relational_test_pure::repos());   // +1: mapping fixtures
// 10 repos total. Topo-sort by Repo::meta().dependencies handles ordering.
```
