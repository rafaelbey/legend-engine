# 04 — Relational native coverage

11 native functions are declared across `platform_store_relational/`.
**None ship from this workspace** — relational native implementations
are scoped to `legend-pure-rust`, where the `crates/runtime` native
registry lives. A separate session is implementing them there. This
file catalogues them only so a future reviewer of `legend-engine-rust`
can find the home crate quickly.

| Native | Pure declaration | Future home |
|--------|------------------|-------------|
| `executeInDb` | `meta::relational::metamodel::execute::executeInDb(sql:String[1], conn:DatabaseConnection[1], timeout:Integer[1], fetchSize:Integer[1]):ResultSet[1]` | `legend-pure-rust/crates/runtime/src/native/relational/execute_in_db.rs` (TBD path) |
| `loadValuesToDbTable` | `…::loadValuesToDbTable(tableData:List<List<Any>>[*\|1], table:Table[1], conn:DatabaseConnection[1]):Nil[0]` (2 overloads) | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `createTempTable` | `…::createTempTable(tableName:String[1], cols:Column[*], sql:Function[1], (relyOnFinallyForCleanup:Boolean[1])?, conn:DatabaseConnection[1]):Nil[0]` (2 overloads) | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `dropTempTable` | `…::dropTempTable(tableName:String[1], conn:DatabaseConnection[1]):Nil[0]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `loadCsvToDbTable` | `…::loadCsvToDbTable(...):Nil[0]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `fetchDbTablesMetaData` | `…::fetchDbTablesMetaData(conn, …):TableMetaData[*]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `fetchDbColumnsMetaData` | `…::fetchDbColumnsMetaData(conn, …):ColumnMetaData[*]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `fetchDbSchemasMetaData` | `…::fetchDbSchemasMetaData(conn, …):SchemaMetaData[*]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `fetchDbPrimaryKeysMetaData` | `…::fetchDbPrimaryKeysMetaData(conn, …):PrimaryKeyMetaData[*]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `fetchDbImportedKeysMetaData` | `…::fetchDbImportedKeysMetaData(conn, …):ImportedKeyMetaData[*]` | `legend-pure-rust/crates/runtime/src/native/relational/` |
| `logActivities` | `meta::relational::runtime::logActivities(activities:Activity[*]):Nil[0]` | `legend-pure-rust/crates/runtime/src/native/relational/` |

## Java reference implementations

Each native has a Java reference in:
`../../legend-pure/legend-pure-store/legend-pure-store-relational/legend-pure-runtime-java-extension-interpreted-store-relational/src/main/java/.../RelationalExtensionInterpreted.java`

That's the source of truth for the Rust port — match each function's
behaviour exactly.

## Why this is scoped to `legend-pure-rust`

- The native registry (`NativeRegistry::standard()`) lives in
  `legend-pure-rust/crates/runtime`.
- Native implementations need `legend-pure-runtime`'s `Value` /
  `ObjectHandle` / `EvalContextTrait` types — they're tightly coupled
  to the runtime crate.
- The SQLite adapter sits *under* the natives as an implementation
  detail; placing it in `legend-pure-rust` keeps the runtime-side
  dependency graph self-contained.

## Coordination

If a future session needs the 4 simpler-to-implement natives unblocked
before the full set, suggest splitting along the same lines used in
`02-deferred-compile-issues.md`'s priorities: `executeInDb`,
`loadValuesToDbTable`, `createTempTable`, `dropTempTable` first;
`fetchDb*MetaData` and `logActivities` second.
