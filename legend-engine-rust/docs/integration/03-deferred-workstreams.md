# 03 — Deferred workstreams

Tracking the work that follows the groundwork phase. Each item below
becomes its own plan when picked up. Each is tagged with which
workspace it lands in (`legend-pure-rust` or `legend-engine-rust`) so
sessions don't get confused about repo boundaries.

## Compiler-side — `legend-pure-rust`

### CMP-1 — Fix the 5 dispatch-narrowing failures *(blocking)*

See `02-deferred-compile-issues.md`. Two distinct ambiguities:
`elementToPath` on `Function<Any>[1]` (×4) and `filterBySchemaName`
(×1). Until these resolve, the relational metamodel doesn't compile
clean and any downstream Pure code referencing those functions is
unusable.

Critical path for: every other deferred workstream below. **Pick this
up first.**

## Runtime-side — `legend-pure-rust` (separate session)

### RT-* — SQLite adapter + relational native functions

A different session owns this. Lands in
`legend-pure-rust/crates/runtime` (or a sibling crate per its preferred
layout). Covers the SQLite adapter wrapping `rusqlite` and the 11
relational native functions enumerated in `04-natives-coverage.md`.

Blocked on: CMP-1 for full Pure-side wiring (the natives need to
construct `Value::Object`s of `DatabaseConnection`, `Table`, `Column`,
`ResultSet` — those classes must compile clean to be resolvable).
Adapter and native bodies can be written and unit-tested independently
of CMP-1; only the `NativeRegistry` registration is gated.

## DSL-side — new crates in `legend-pure-rust`

### DSL-1 — Recursive-descent port of `RelationalParser.g4`

338 lines of ANTLR4 grammar at
`../../legend-pure/legend-pure-store/legend-pure-store-relational/legend-pure-m2-store-relational-grammar/src/main/antlr4/.../RelationalParser.g4`.

Lands as `legend-pure-rust/crates/dsl-relational/` (mirroring
`dsl-store`'s shape). Only required when we want users to author
`Database <db>(...)` blocks directly. The metamodel (the 5 files we
embed today) is independent.

### DSL-2 — Mapping-body relational sub-parser

Extends `legend-pure-rust/crates/dsl-mapping`'s sub-parser registry to
handle `Relational` mapping bodies (`~mainTable [db]Schema.Table`,
`~filter [...]`, etc.). Depends on a traits-only
`dsl-mapping-extension-api` crate to break the otherwise-circular
dependency between `dsl-mapping` and `dsl-relational`.

## Integration-side — `legend-engine-rust`

### INT-1 — Embed `core_relational` from legend-engine

Adds a new repo crate analogous to `store-relational-pure`, pointing at
the legend-engine descriptor. Pulls in the SQL-gen + plan + mapping
execution Pure code that does the heavy lifting per the load-bearing
insight in the original plan.

Blocked on: CMP-1 (the metamodel is the type universe `core_relational`
references); DSL-2 (mapping bodies in test fixtures use the relational
sub-grammar).

### INT-2 — End-to-end `boolean.pure:testGet` lock

The acceptance test for the full integration milestone: run
`testGet` through Rust, assert it produces the same 14 rows Java does.
Blocked on: every other deferred item above plus a `legend test`
extension-registration story in `legend-pure-rust`'s CLI.

### INT-3 — Java baseline parity

Capture `$result.values` from Java legend-engine; check JSON into
fixtures; byte-diff against Rust output. Blocked on INT-2.

## Order to attack

```
CMP-1 ─┬─ RT-* (separate session)
       └─ DSL-2 ── INT-1 ── INT-2 ── INT-3
                   │
                   DSL-1 (independent — needed only if user-authored
                          Database blocks become a requirement)
```

CMP-1 is the gate. Everything else can run in parallel once the
metamodel compiles clean.
