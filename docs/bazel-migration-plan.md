# Maven → Bazel Migration Plan for legend-engine

## Context

`legend-engine` is a 575-module Maven reactor (432 Java modules, 191 Pure-only
modules, 152 ANTLR grammars) that today takes 30–60 min on a 16-core runner
with `MAVEN_OPTS=-Xmx32g` and `-T4`. Most of that cost is not Java compilation
— it is the two-stage **Pure** pipeline, invoked 294 times across the build:

- `legend-pure-maven-generation-par` (129 invocations, bound to
  `generate-sources`) compiles `.pure` sources into a PAR (serialized metadata
  JAR) per "repository".
- `legend-pure-maven-generation-java` (137 invocations, bound to `compile`)
  walks the PAR and emits modular Java that `javac` then builds.
- `legend-pure-maven-generation-pct` (28 invocations) runs Platform Compiler
  Tests for DB-dialect modules.

Each mojo invocation pays JVM startup, initializes the Pure runtime, and
re-reads all upstream PARs off the classpath. The Pure plugins are coarse
(one repository = one pom module) and opaque to Maven: a single `.pure` edit
rebuilds the whole module. Dependency discovery happens at *runtime* via
`ServiceLoader<CodeRepositoryProvider>` reading `META-INF/services` entries —
Maven never sees the real Pure dep graph.

The consequences: over-rebuilding, no remote caching of Pure outputs, wall
clocks dominated by JVM spin-up, CI that must pre-split tests into 20 hand-
maintained matrix groups, and a build model that treats Pure and Java as two
separate universes glued together by `target/` directories.

**Goal:** move to Bazel so that (a) Pure and Java live in one action graph
with a correct, content-addressable dep graph; (b) a persistent-worker Pure
compiler reuses JVM + runtime state across actions; (c) the build is
remote-cacheable and incremental at `pure_library`/`java_library` granularity.
Maven Central publishing must still work after cutover.

## Decisions (confirmed with user)

- **Rollout**: incremental coexistence — Maven and Bazel both build the repo
  during migration; CI runs both in parallel until parity is met.
- **Publishing**: Bazel must keep producing Maven-Central-compatible artifacts
  with stable GAV coordinates.
- **Pure plugins**: rewrite as native Starlark rules driving a persistent
  JVM worker — not a thin genrule wrapper around the existing mojos.

## Target Architecture

### 1. Repository layout additions

```
/MODULE.bazel                       # bzlmod, deps + toolchains
/.bazelrc                           # project defaults (jvmopts, workers, RBE)
/.bazelversion
/tools/bazel/
  pure/
    BUILD.bazel
    defs.bzl                        # pure_library, pure_java_codegen, pure_test, pct_test
    toolchain.bzl                   # pure_toolchain rule + registration
    worker/                         # persistent worker entrypoint (Java)
      PureWorkerMain.java
      PureWorkRequest.proto
  java/
    defs.bzl                        # legend_java_library macro (adds checkstyle, src-jar)
    checkstyle.bzl
  antlr/
    defs.bzl                        # wrap rules_antlr or use a 10-line genrule + worker
  publishing/
    defs.bzl                        # legend_maven_publish macro (pom gen + signing)
  migration/
    pom_to_bazel.py                 # one-shot generator, see §4
    parity_diff.py                  # compares Bazel vs Maven jar contents
/third_party/
  BUILD.bazel                       # curated exports of maven_install targets
  jvm_deps.bzl                      # central pinned artifact list
```

Per-module `BUILD.bazel` files sit next to each existing `pom.xml` — the
directory structure does not change.

### 2. External dependencies

- `rules_jvm_external` via `bzlmod` (`maven.install` extension) with pinning
  to `maven_install.json` so lockfile diffs are reviewable.
- Single central artifact list in `tools/bazel/third_party/jvm_deps.bzl`
  populated by scraping the root `<dependencyManagement>` (647 entries).
- `legend-pure-*` artifacts pulled from Maven Central the same way — the
  compiler itself is a classpath input to the `pure_toolchain`.

### 3. Toolchains

- `java_toolchain` pinned to JDK 11 for build, release target 8
  (matches current `maven.compiler.release=8`).
- `pure_toolchain` selects the `legend.pure.version` (today 5.74.1). Version
  is resolved via `MODULE.bazel` so a bump is one line.

## Re-engineering the Pure Plugins

This is the biggest win and the biggest design surface. The current mojos
conflate "compile a repository" and "emit Java" and run in separate JVMs per
module. The new rules split concerns and collapse JVM cost.

### Rule set

```starlark
# tools/bazel/pure/defs.bzl

pure_library(
    name,                # e.g. "core_relational_postgres"
    srcs,                # glob(["src/main/resources/core_relational_postgres/**/*.pure"])
    definition,          # "src/main/resources/core_relational_postgres.definition.json"
    deps,                # other pure_library targets (NOT ServiceLoader-discovered)
    platform_deps,       # legend-pure m2/m3 grammar jars via toolchain
    visibility,
)
# Outputs:
#   <name>.par          (PAR archive, cacheable, one per Pure repo)
#   <name>-repo.jar     (META-INF/services + definition.json for runtime discovery)
# Provider: PureInfo(par, transitive_pars, repo_name, namespace_pattern, deps)

pure_java_codegen(
    name,                # e.g. "core_relational_postgres_java"
    pure_lib,            # :core_relational_postgres
    generation_type = "modular",
    single_dir = True,
)
# Outputs:
#   srcs.srcjar          → feeds a standard java_library
# Provider: JavaInfo-compatible via output_group

pure_test(
    name,
    pure_lib,
    test_pattern,        # e.g. "meta::external::store::service"
)
# Drives PureTestBuilderCompiled via the same worker; emits JUnit XML.

pct_test(
    name,
    adapter,             # e.g. "postgres", "snowflake"
    pure_lib,
    native_libs = [],    # JDBC drivers etc.
)
# Replaces legend-pure-maven-generation-pct; one target per dialect.
```

A typical leaf module BUILD (replacing e.g.
`legend-engine-xt-relationalStore-postgres-pure/pom.xml`):

```starlark
load("//tools/bazel/pure:defs.bzl", "pure_library", "pure_java_codegen")
load("//tools/bazel/java:defs.bzl", "legend_java_library")

pure_library(
    name = "core_relational_postgres",
    srcs = glob(["src/main/resources/core_relational_postgres/**/*.pure"]),
    definition = "src/main/resources/core_relational_postgres.definition.json",
    deps = [
        "//legend-engine-core/legend-engine-core-pure/legend-engine-pure-code-compiled-core:core",
        "//legend-engine-xts-relationalStore/.../core-pure:core_relational",
    ],
)

pure_java_codegen(
    name = "core_relational_postgres_java",
    pure_lib = ":core_relational_postgres",
)

legend_java_library(
    name = "legend-engine-xt-relationalStore-postgres-pure",
    srcs = [":core_relational_postgres_java"],
    resources = [":core_relational_postgres"],  # PAR + services on classpath
    deps = [...],
)
```

### Why this is faster

1. **Persistent worker.** `PureWorkerMain` stays alive per Bazel invocation
   and across invocations (`--worker_max_instances`). JVM warm-up (~2–5 s)
   + Pure runtime init (several seconds on large graphs) amortizes to ~zero
   per action. With 129 PAR builds today this is the single biggest win.

2. **Correct Pure dep graph in the build system.** Today `definition.json`
   declares Pure deps and `ServiceLoader` finds them by JAR-scan at runtime.
   The new `pure_library.deps` attribute makes that graph explicit to Bazel,
   so edits to upstream `.pure` invalidate only the transitive closure, not
   every module on the classpath. A migration-time validator reads each
   `definition.json`'s `dependencies` array and fails if it disagrees with
   the Starlark `deps` list.

3. **Split PAR and Java codegen.** Many consumers need only the PAR
   (runtime metadata); the Java codegen step is optional and parallel. Today
   both stages run for every Pure module whether the Java is needed or not.

4. **Action-level remote cache.** Each `pure_library` action's cache key is
   `(toolchain, transitive PAR hashes, source hashes, definition.json)`.
   A CI hit replays a PAR in milliseconds.

5. **Finer `pure_library` than `pom` module.** Definition JSONs that declare
   multiple sub-repositories (some of the `core_*` modules do) can be
   split into multiple `pure_library` targets sharing a directory. Not
   required on day 1, but the rule API permits it.

### Worker protocol

- Standard Bazel `WorkerProtocol` (proto-based). Each `WorkRequest` carries:
  source file paths, PAR dep paths, definition JSON path, repository name,
  output PAR path, an "emit-java-srcjar" flag, and the output srcjar path.
- The worker holds a cached `PureRuntime` keyed by the transitive PAR set
  hash, so back-to-back builds of sibling modules share loaded metadata.

### Services registration

The runtime still needs `META-INF/services/....CodeRepositoryProvider`
entries so the engine's `ServiceLoader` finds repositories. The new
`pure_library` rule generates that file as part of its output resources,
keyed off the declared `repo_name`. No hand-written provider classes needed
(today each module has a tiny `*CodeRepositoryProvider.java`; those go away).

## Migration Phases

### Phase 0 — Infra bootstrap (1 sprint)

- Add `MODULE.bazel`, `.bazelrc`, `.bazelversion`, `tools/bazel/**`.
- Bring up `rules_jvm_external` and pin every artifact in the root
  `<dependencyManagement>` to `maven_install.json`.
- Build the Pure persistent worker and the four Starlark rules above.
- Write two leaf targets end-to-end to validate:
  - `legend-engine-pure-code-compiled-core` (the root Pure repo)
  - `legend-engine-xt-relationalStore-postgres-pure` (representative leaf)
- Add `.github/workflows/bazel.yml` running `bazel build //...` on those
  two targets only.

### Phase 1 — Generator and parity harness (1–2 sprints)

- `tools/bazel/migration/pom_to_bazel.py`: parses `pom.xml`, emits
  `BUILD.bazel`. Handles the common shapes: plain `java_library`, Pure
  module (par+java), ANTLR module, test-jar, shade.
- `tools/bazel/migration/parity_diff.py`: for any module, runs
  `mvn package` and `bazel build`, diffs the resulting JARs (filenames,
  class counts, service files, manifest). Used as the CI parity gate.
- Generate BUILD files for all ~191 Pure-only modules and ~50 pure Java
  modules that have no codegen. Run parity diff on every one.

### Phase 2 — Code-gen modules (2 sprints)

- ANTLR: wrap `rules_antlr` or ship a small `antlr_grammar` macro; migrate
  the 38 grammar modules. Same worker pattern as Pure (ANTLR is cheap but
  the JVM warm-up matters at this scale).
- Custom generators (`legend-engine-xt-snowflake-generator`,
  `legend-engine-xt-memsqlFunction-generator`): model each as a
  `java_binary` + `genrule` feeding downstream `java_library`.
- Web assets (Pure IDE, REPL DataCube): stop downloading at build time.
  Pre-fetch the NPM artifacts in `MODULE.bazel` via `http_archive` with
  SHA256 pins.

### Phase 3 — Shade and platform-specific (1 sprint)

- Spanner shaded JDBC driver: use `rules_jvm_external`'s built-in shading
  (`maven_install` with relocation rules) or a small Starlark wrapper
  around `jarjar_rules`. Preserve the 9 existing relocation patterns from
  `legend-engine-xt-relationalStore-spanner-jdbc-shaded/pom.xml`.
- Netty epoll / platform classifiers: `select()` on
  `@platforms//os:linux` vs `macos`.
- Three other shaded modules (H2 executor, Snowflake m2mudf, REPL DataCube)
  get the same treatment.

### Phase 4 — Bulk migration (3–4 sprints)

- Migrate the remaining ~300 Java-heavy modules in leaf→root order. Each
  PR migrates one parent-aggregator subtree and must pass parity diff.
- CI runs `mvn install -DskipTests` and `bazel build //...` in parallel;
  both must succeed on every PR until cutover.
- Tests migrate alongside: `java_test`, `pure_test`, `pct_test`. The 20
  GitHub-matrix test groups in
  `.github/workflows/resources/modulesToTest.json` become Bazel
  `test_suite` targets; Bazel's sharding replaces most of the matrix.

### Phase 5 — Publishing and cutover (1 sprint)

- `tools/bazel/publishing/defs.bzl` exposes `legend_maven_publish`, a macro
  around `rules_jvm_external`'s `pom_file` + `maven_publish`. Preserves
  GAVs, pom shape, and signing (`finos:7` parent compatibility). Dry-run
  against a staging Maven repo before cutover.
- `mvn release:prepare/perform` is replaced by a Bazel release runbook
  that produces the same set of artifacts and attaches source/javadoc jars.
- After a green cutover week, delete the Maven reactor: all `pom.xml`
  files, `checkstyle.xml` plugin hooks, and Maven-specific CI.

## Critical Files to Create or Modify

**Create (new):**
- `MODULE.bazel`, `.bazelrc`, `.bazelversion`
- `tools/bazel/pure/defs.bzl`, `toolchain.bzl`, `worker/PureWorkerMain.java`
- `tools/bazel/java/defs.bzl`
- `tools/bazel/antlr/defs.bzl`
- `tools/bazel/publishing/defs.bzl`
- `tools/bazel/migration/pom_to_bazel.py`, `parity_diff.py`
- `tools/bazel/third_party/jvm_deps.bzl`
- `BUILD.bazel` alongside each of the 575 `pom.xml`s (generated)
- `.github/workflows/bazel.yml`

**Modify (during coexistence):**
- `/home/user/legend-engine/pom.xml` — no changes until cutover; keep
  authoritative until parity is met.
- `/home/user/legend-engine/.github/workflows/build.yml` — add a Bazel
  job running in parallel; do not remove the Maven job.
- `/home/user/legend-engine/.github/workflows/resources/modulesToTest.json`
  — kept as Maven's view; Bazel derives equivalents automatically.

**Delete (at cutover only):**
- All `pom.xml` except the root (keep root minimal for downstream tooling
  that scrapes GAVs, or remove entirely if publishing is fully Bazel-driven).
- Hand-written `*CodeRepositoryProvider.java` classes replaced by the
  `pure_library` rule's generated services file.

## Reused Existing Inputs

- `**/*.definition.json` (~160 files) — consumed as-is by the new
  `pure_library` rule; no schema changes. Examples:
  - `legend-engine-core/legend-engine-core-pure/legend-engine-pure-code-compiled-core/src/main/resources/core.definition.json`
  - `legend-engine-xts-relationalStore/.../legend-engine-xt-relationalStore-postgres-pure/src/main/resources/core_relational_postgres.definition.json`
- `.github/workflows/resources/modulesToTest.json` — used by the generator
  to bucket Bazel `test_suite` tags.
- Root `pom.xml` `<dependencyManagement>` — scraped once to seed
  `jvm_deps.bzl`; artifact list then maintained in Bazel.
- `checkstyle.xml` at repo root — consumed directly by
  `legend_java_library`'s checkstyle aspect.
- The `legend-pure-maven-generation-*` *artifacts* — not the mojos, but the
  compiler library they wrap — are the Pure worker's classpath. Same JAR,
  different driver.

## Verification

**Per-module parity (Phases 1–4):**
- `python tools/bazel/migration/parity_diff.py <module>` must show:
  identical `.class` file set, identical `META-INF/services/**`, identical
  PAR metadata digest. Fails the PR if not.

**Whole-repo correctness:**
- `bazel build //...` clean + warm; clean must succeed in <30 min on the
  16-core runner, warm in <2 min (expected remote-cache hits).
- `bazel test //...` passes with the same test inventory Maven runs today
  (a script reconciles `surefire-reports-aggregate` with Bazel JUnit XML).
- Smoke: boot `legend-engine-server-http-server` from the Bazel-built jar
  and execute a reference service plan via the REPL.

**Pure rule correctness:**
- `bazel test //tools/bazel/pure:worker_tests` — golden tests that compile
  a fixed `.pure` corpus and diff the PAR byte-for-byte against the
  mojo-produced PAR from a pinned `legend.pure.version`.
- Worker determinism: run the same action twice, assert identical output
  hash (guards against Pure's internal iteration-order bugs; fix any that
  surface by sorting emitted collections).

**Publishing parity:**
- Stage a release to a local Maven repo via both paths; diff every
  produced `.pom`, `.jar`, `.sources.jar`, `.javadoc.jar`, and `.asc`
  signature metadata. GAV + transitive `<dependency>` closure must match.

**Performance goals:**
- Cold full build: ≤15 min on a 16-core runner (vs 30–60 today).
- No-op incremental build: ≤10 s.
- Single-file `.pure` edit rebuild: ≤5 s for the touched repo and its
  direct consumers only.

---

# Alternative Plan B — Bazel + Rust Pure via JNI

This alternative keeps the entire Bazel migration (Phases 0–5 above) but
**replaces the Pure toolchain**: instead of re-engineering the JVM-based
`legend-pure-maven-generation-*` mojos as Bazel rules with a persistent
JVM worker, we adopt the Rust re-implementation of Pure that already
exists at `rafaelbey/legend-pure@legend-pure-rust` and reach the Java
engine through JNI. The Maven → Bazel migration itself is unchanged; what
changes is Section "Re-engineering the Pure Plugins" and every downstream
consequence (Java codegen, `ServiceLoader`, runtime).

## Context

Plan A accepts the Pure compiler as a JVM dependency. Its ceiling is
bounded by JVM startup + the `legend-pure` project's current
architecture. The Rust port (`legend-pure-rust`) has already shipped
eight crates and passes its full test suite:

- `legend-pure-parser-lexer`, `-parser-parser`, `-parser-ast`,
  `-parser-protocol`, `-parser-compose`, `-parser-pure`
  (semantic IR), `legend-pure-runtime` (interpreter with `im-rc`
  persistent structures + `slotmap` generational heap, optional codegen
  for hot paths), `legend-pure-parser-jni`, and the `legend` CLI binary
  (`legend parse|check|init`).
- Per-repo compiled model is serialized via `bincode` for
  "near-instant startup" — this is the concrete per-repo binary artifact.
- JNI crate today exposes parser/AST/protocol-JSON to Java; the runtime
  crate is Rust-native.

Adopting this path gives us: no JVM for Pure compilation, ms-scale per-
repo compile times, a single action graph where Pure artifacts are
genuinely content-addressable, and long-term decoupling from the JVM
`legend-pure` project. The cost is a new JNI surface, a value-marshalling
boundary, and two-way parity testing during the rollout.

## What exists vs. what we must build

**Exists (reuse as-is, pinned to a specific `legend-pure-rust` commit):**
- All parse + semantic + runtime crates.
- `legend parse`, `legend check`, `legend init`.
- JNI surface for parsing (Java ↔ protocol JSON).
- Bincode model cache format.

**Build (scoped as this plan's deliverables):**
1. `legend compile --repo <name> --definition <path> --deps <paths> -o <out>.pure-bin`
   — new CLI subcommand that takes a repo's sources + its upstream
   `.pure-bin` deps and writes one per-repo artifact. Wraps the semantic
   analyzer + bincode serializer already in `legend-pure-parser-pure`.
2. **JNI runtime surface** — extend `legend-pure-parser-jni` with
   entry points that load a `.pure-bin` into a shared Rust `PureRuntime`,
   resolve Pure function handles by qualified name, and invoke them
   with marshalled arguments. Mirrors what
   `CompiledExecutionSupport` / `Pure.functionByName(..)` does today.
3. **Java adapter library** — `legend-engine-pure-runtime-rust`. A new
   Java module that:
   - `System.loadLibrary("legend_pure")` loads the JNI `.so`/`.dylib`/`.dll`.
   - Exposes a `RustPureExecutionSupport` implementing the same
     interface contract as `CompiledExecutionSupport` so existing
     callers (found by ServiceLoader today) work unchanged.
   - Backs `PureObjectRef` with opaque Rust `ObjectId` handles,
     cleaned up via `Cleaner`.
4. **Value marshalling layer** — primitives direct; strings via
   modified UTF-8; collections via shared `ByteBuffer` for bulk paths
   and per-element JNI calls for small ones; Pure instances as opaque
   handles. Explicit rules live in
   `tools/bazel/pure/rust/marshal.md`.
5. **Java-implemented Pure extension bridge** — legend-engine today
   implements many Pure native functions in Java (JDBC execution, date
   math, etc.). Rust runtime must call back into the JVM for these. Use
   a registration table populated at boot; Rust side uses
   `AttachCurrentThread` when no JVM thread is on the stack.
6. **Bazel rules** (replaces `pure_library` / `pure_java_codegen` /
   `pure_test` / `pct_test` from Plan A):
   ```starlark
   # tools/bazel/pure/rust/defs.bzl
   pure_rust_library(
       name, srcs, definition, deps,
   )
   # Action: legend compile -> <name>.pure-bin
   # Provider: PureRustInfo(bin, transitive_bins, repo_name)

   pure_rust_runtime_jni(
       name = "legend_pure_jni",
       libs = [":core", ":core_relational", ...],  # transitive closure
   )
   # Action: builds libpure_runtime_jni.{so,dylib,dll} embedding all selected
   # .pure-bin artifacts. select() per @platforms//os.

   pure_rust_test(name, pure_lib, test_pattern)
   pct_rust_test(name, adapter, pure_lib, native_libs = [])
   ```
7. **`rules_rust` integration** — pin toolchain in `MODULE.bazel`;
   `crate_universe` locks `legend-pure-rust` crates via
   `Cargo.Bazel.lock.json`. Cross-compile for linux-x86_64, linux-aarch64,
   darwin-x86_64, darwin-aarch64, windows-x86_64 — same matrix the
   Netty-epoll classifiers currently cover.
8. **Feature flag** — `-Dlegend.pure.runtime=rust|java` at JVM boot,
   resolved per Pure repo via a static table. Java path stays on the
   classpath until the last repo is migrated.

## Rollout Phases

Runs inside the Bazel migration already underway. Rust phases are
gated on Plan A's Phase 0/1 being complete (we need the Bazel harness
to even register the new rules).

### Phase R0 — Toolchain & vendor (1 sprint, parallelizable with Plan A Phase 1)
- Add `rules_rust` to `MODULE.bazel`.
- Vendor `legend-pure-rust` by pinning to a specific commit via
  `crate_universe` from a Git source, or (preferred) publish the crates
  to crates.io under a legend-prefixed namespace.
- Implement the `legend compile` subcommand upstream in
  `legend-pure-rust` (PRs to `rafaelbey/legend-pure:legend-pure-rust`);
  fall back to a local patch if upstream cadence is slow.
- Integrate `cargo build`-produced `legend` binary as a Bazel tool.

### Phase R1 — Parse & semantic parity (2 sprints)
- Parser parity: for **every** `.pure` file across the 2,851 files in
  the repo, diff `legend parse --emit protocol-json` against the
  current Java/ANTLR parser's output. Zero diffs before progressing.
- Semantic parity: run
  `legend-engine-pure-code-functions-*` test corpora through the Rust
  semantic analyzer; assert identical resolved-symbol tables and
  identical error messages (within a declared format).
- Driven by a new `tools/bazel/migration/rust_parity_diff.py`.

### Phase R2 — JNI runtime surface (2–3 sprints)
- Extend `legend-pure-parser-jni` with load/invoke/free entry points.
- Define the stable C ABI: `pure_runtime_new`, `pure_runtime_load_bin`,
  `pure_function_handle`, `pure_invoke`, `pure_object_release`,
  `pure_extension_register`, `pure_runtime_free`. Documented in
  `tools/bazel/pure/rust/abi.md` with semver discipline.
- Build `RustPureExecutionSupport` on the Java side. Implements the
  same interface as `CompiledExecutionSupport`, so any code that takes
  a `CompiledExecutionSupport` works with either backend.
- Ship `pure_rust_runtime_jni` Bazel rule; wire it into a single test
  binary that runs the Pure test corpus via JNI. Pass rate must equal
  the Java path's pass rate.

### Phase R3 — Extension bridge (2 sprints)
- Build the Java-side extension registry: enumerate every Pure native
  function that today has a Java implementation (grep
  `@NativeFunction`, `*NativeFunctions.java`, ~several hundred).
- Generate a registration block at engine boot that populates the
  Rust-side callback table via JNI.
- Soak-test: 1 h mixed workload (ingest, JDBC execution plans, lambda
  evaluation) with the extension bridge hot. Gate: zero native crashes,
  stable RSS, no JVM classloader leaks.

### Phase R4 — Per-repo cutover (3–4 sprints)
- Flip the feature flag one Pure repo at a time, starting with leaves
  that have the fewest Java callers (good candidates: `core_relational_*`
  dialect repos, `core_external_format_*`, DSL repos under `xts-text`,
  `xts-diagram`, `xts-data-space`).
- Each flip requires a green run of:
  - `bazel test //...` on the Rust path for that repo.
  - A before/after perf benchmark on `legend-engine-server-http-server`
    for a fixed set of service plans touching that repo; regression
    budget: 0% p50, 10% p99 (documented).
- Rollback is a one-line flag flip.

### Phase R5 — Retire the Java Pure runtime (1 sprint)
- After the last repo flips, delete:
  - The hand-written `*CodeRepositoryProvider.java` classes.
  - The `pure_library` / `pure_java_codegen` rules from Plan A.
  - The `legend-pure-maven-generation-*` Bazel-wrapped toolchain.
  - `CompiledExecutionSupport` and its direct callers (migrated to
    `PureExecutionSupport` facade which now has only one impl).
- Keep the Java parser for IDE tooling **only** (Pure IDE features)
  until Rust LSP lands — out of scope here.

## Comparison — Plan A vs Plan B

| Dimension | Plan A (Bazel + JVM Pure worker) | Plan B (Bazel + Rust Pure + JNI) |
|---|---|---|
| Per-repo Pure artifact | PAR (serialized Java metadata) + generated Java srcjar | `.pure-bin` (bincode) — no Java codegen |
| Compiler locality | JVM persistent worker | Native CLI, microseconds of startup |
| Cold CI build target | ≤15 min | ≤8–10 min (no JVM cascade on Pure actions) |
| Incremental `.pure` edit | ≤5 s | ≤1–2 s |
| Runtime perf | JVM JIT, well understood | Rust interp + optional codegen; needs benchmarking |
| Engineering cost (new code) | Worker + 4 Starlark rules | Worker-sized work **plus** JNI surface, extension bridge, marshalling layer, Java adapter |
| Risk profile | Moderate — parity on PAR bytes | High — parity on parser, semantics, runtime, AND JNI ABI stability |
| Rollback unit | Git revert of Bazel rule | Per-repo feature flag flip |
| Decouples from legend-pure JVM? | No | Yes |
| Impact on `legend-engine` Java code | Small (services file move) | Large (every caller of `CompiledExecutionSupport` goes through a facade) |
| Impact on Pure IDE / tooling | None | Deferred; IDE stays on JVM parser until LSP lands |
| Crash blast radius | JVM exception | Native crash → JVM process death (mitigated via out-of-proc `legend serve` fallback) |
| Team skill mix required | JVM + Bazel | JVM + Bazel + Rust + JNI |
| Total calendar time | ~3 months (Plan A alone) | ~9–12 months on top of Plan A |

## Critical Files (Plan B specific)

**Create:**
- `tools/bazel/pure/rust/defs.bzl`, `toolchain.bzl`, `abi.md`, `marshal.md`
- `tools/bazel/migration/rust_parity_diff.py`
- Java module `legend-engine-pure-runtime-rust/` with
  `RustPureExecutionSupport`, `PureObjectRef`, `PureRuntimeHandle`.
- `MODULE.bazel` additions: `rules_rust`, `crate_universe` lockfile.

**Modify:**
- `legend-pure-rust` upstream: add `legend compile` subcommand and
  runtime JNI entry points (PRs to `rafaelbey/legend-pure`).
- All call sites of `CompiledExecutionSupport` → facade
  `PureExecutionSupport` (scripted refactor, ~hundreds of sites).
- `.github/workflows/bazel.yml`: add Rust toolchain, parity diff job,
  per-OS JNI build matrix.

**Delete (at R5):**
- All outputs from Plan A's Pure rules.
- Hand-written `*CodeRepositoryProvider.java` classes.
- `legend-pure-maven-generation-*` toolchain wiring.

## Reused Existing Artifacts (Plan B)

- `legend-pure-rust` crates — the entire parser + semantic + runtime
  stack already exists; nothing to rewrite there.
- `**/*.definition.json` — still authoritative for repo boundaries and
  dependency order. Consumed by `legend compile`.
- `.github/workflows/resources/modulesToTest.json` — unchanged.
- Every existing Pure test suite — runs unchanged through the
  `PureExecutionSupport` facade.

## Verification (Plan B)

- **Parse parity (Phase R1):** for each of the 2,851 `.pure` files,
  `diff <(legend parse --emit protocol-json f) <(java-parser f)` must
  be empty. Checked in CI as a job that fails if any file diverges.
- **Semantic parity (Phase R1):** every file's resolved-symbol table
  must match (compared via a canonical dump tool). Error messages
  compared structurally (error kind + source span), not string-exact.
- **Runtime parity (Phase R2):** full Pure test corpus passes on the
  Rust path with the same pass/fail set as the Java path; any flake
  must be resolved, not skipped.
- **Extension bridge soak (Phase R3):** 1-hour mixed workload, zero
  native crashes, RSS drift < 50 MB, zero JVM classloader leaks
  (measured via heap histograms).
- **Per-repo integration (Phase R4):** golden service plans replayed
  against `legend-engine-server-http-server` with the flag flipped;
  byte-exact output parity with the Java path.
- **Perf gates:** cold Bazel build ≤10 min; `legend compile` p50 < 200 ms;
  single-file `.pure` rebuild ≤2 s; engine p50 latency regression ≤0%,
  p99 ≤10% (tightened as confidence grows).
- **ABI stability (Phase R2+):** every change to the JNI headers
  increments a semver; a CI check diffs the generated `legend_pure.h`
  against the tagged baseline and fails if a breaking change isn't
  accompanied by a major bump.

