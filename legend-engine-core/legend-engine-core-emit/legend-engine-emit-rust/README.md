# legend-engine-emit-rust

Rust ⇄ Java EMIT parity harness.

Runs every `*.emit.yaml` model through **both** the existing Java
[`EMITRunner`](../legend-engine-emit/src/main/java/org/finos/legend/engine/test/emit/EMITRunner.java)
and the in-progress Rust engine (loaded in-process via the
`liblegend_engine_jni` cdylib from
[`legend-engine-rust/crates/jni`](../../../legend-engine-rust/crates/jni)),
then surfaces per-phase divergences as JUnit dynamic tests and a TSV
report. Initial Rust coverage is one merged `PARSE_AND_COMPILE` cell
(the current JNI surface — `nativeInitContextWithClasspath` — does
parse and compile atomically); later phases are scaffolded as
`UNSUPPORTED` until corresponding Rust entry points land upstream in
legend-pure-rust.

## Quick start

```bash
# 1. Build the engine cdylib (one-time, when crates/jni or its deps change)
cd legend-engine-rust
cargo build -p legend-engine-rust-jni

# 2. Regenerate the workspace classpath TOML (one-time per repo set change)
cargo gen-classpath

# 3. Build + run the parity harness
cd ..
mvn install -pl legend-engine-core/legend-engine-core-emit/legend-engine-emit-rust -am
```

Without steps 1–2, the harness is designed to skip cleanly — JUnit
Assumption-skips the Rust-side parity tests with a clear reason
("cdylib unavailable: ..." or "baseline classpath TOML not found
at ..."). Default `mvn install` does NOT require cargo on PATH.

> **Verification status (initial scaffold):** Standalone `javac` against
> our 12 main + 1 test Java files confirms syntactic and structural
> correctness (only unresolved-symbol errors for upstream EMIT types
> that aren't yet in local m2). The full Maven build chain on the
> `legend-engine-rust` branch is currently red upstream at
> `legend-engine-pure-platform-dsl-tds-java` (a `stringToTDS`
> Java-codegen gap in `legend-pure 5.85.1-SNAPSHOT`) regardless of
> `legend.pure.version`. End-to-end test verification (skip-path
> behavior, descriptor synthesis accepted by `legend-pure-build`) is
> deferred until that chain resolves — see the plan's verification
> section and the "Descriptor format" caveat below.

## What's emitted

For every `*.emit.yaml` under the `emit-models/` classpath root, the
`@TestFactory` in `RustEMITTestSuite` produces:

- **`[<model>] Diff: PARSE_AND_COMPILE`** — fails JUnit only when Rust
  succeeded but Java failed (regression) or the JNI crashed. Known
  gaps (Java passes, Rust fails) print to stderr with `KNOWN GAP …`
  but pass — recorded in the TSV.
- **`[<model>] Diff: MODEL_GENERATION` / `FILE_GENERATION` /
  `TEST_EXECUTION` / `PLAN_GENERATION`** — Assumption-skipped today
  ("Rust phase not yet implemented") so the matrix is complete from
  day one.
- **`[<model>] Report`** — appends one TSV row per phase to
  `target/emit-rust-diff.tsv`:

```
model	phase	java_status	rust_status	match	java_ms	rust_ms	divergence
```

Override the report path with `-Demit.rust.report=/abs/path/to/report.tsv`.

## How it works

`EMITDiffRunner` loads each YAML via `EMITModelLoader` (the existing
shared component), then:

1. **Java side**: drives `new EMITRunner().run(descriptor)` verbatim.
2. **Rust side**: `EMITRustBridge` synthesizes a temp directory mirroring
   the real engine descriptor layout:
   ```
   <tmp>/<model>.definition.json          # {name, pattern:".*", dependencies:["platform"]}
   <tmp>/<model>/<virtualPath...>.pure    # staged copies of the EMIT source files
   ```
   then builds a per-model classpath TOML by reading the workspace
   baseline (`legend-engine-rust/legend-pure-classpath.toml`),
   resolving every `descriptor = "<rel>"` to an absolute path, and
   appending the synthetic `[[repo]]` entry. The UTF-8 bytes are
   handed to `nativeInitContextWithClasspath`. Success ⇒
   `PARSE_AND_COMPILE` passes; throwable ⇒ fails with the message
   surfaced in the TSV.
3. `EMITDiffResult.build` conjoins Java's `PARSE` + `COMPILE` results
   and pairs them against the merged Rust cell, producing one
   `EMITDiffPhaseResult` per conceptual phase.

The local `org.finos.legend.pure.rust.PureRustEvaluator` stub declares
just two native methods (`nativeInitContextWithClasspath`,
`nativeFreeContext`). It deliberately uses the upstream FQN so the
cdylib's `Java_org_finos_legend_pure_rust_PureRustEvaluator_*` symbols
resolve. **Must NOT coexist on the classpath with
`legend-pure-runtime-rust-evaluator`'s much larger `PureRustEvaluator`
class** — both define the same FQN.

## Adding fixtures

Drop new `*.emit.yaml` (plus its referenced `.pure` sources) under
`src/test/resources/emit-models/`. Each yaml is automatically picked
up by the `@TestFactory` discovery.

## Descriptor format caveat

The synthesized `<model>.definition.json` (`{name, pattern: ".*",
dependencies: ["platform"]}`) is modeled on
`core_diagram_metamodel.definition.json` and similar engine-side
descriptors. The permissive `pattern: ".*"` choice assumes the
classpath resolver picks the most-specific repo by stricter
`pattern` — true on inspection but not yet smoke-tested against
`legend-pure-build`. The plan's verification step calls for:

```bash
cd legend-engine-rust
cargo run -p legend-engine-rust-cli -- check --classpath <synth.toml>
```

against `simple-class.emit.yaml` to confirm the shape is accepted
before the harness is trusted in CI. This is the first thing to
do once the cdylib is built.

## Follow-up work

- Add a non-`PureRustEvaluator` JNI symbol alias to `legend-engine-rust/crates/jni`
  so the local stub can move to a unique FQN, eliminating classpath-collision risk.
- Split `nativeParse` / `nativeCompile` upstream in `legend-pure-rust`
  so the Rust side reports two phases instead of one merged cell.
- Wire in larger corpora (e.g. the relational fixtures from
  `legend-engine-xt-relationalStore-emit`) via test-jar packaging or a
  dedicated shared-fixtures module.
- Stress modes: concurrency + iteration counts (`-Demit.rust.threads=N`,
  `-Demit.rust.iterations=N`) for JNI thread-safety + leak validation
  — out of scope for v1, see plan file for rationale.
