# 05 — Language gaps (parser-level)

Status: foundation snapshot — generated 2026-05-17 against
`master` (legend-engine) + `legend-engine-rust` branch
`legend-engine-rust`. Re-runnable end-to-end via the
"Reproduce" section below.

## What this measures

Parser-level (parse-only) status of every Pure source file under
every engine + platform repo declared in
`legend-engine-rust/legend-pure-classpath.toml`. Surfaces gaps in
`legend-pure-rust`'s parser — the DSL sections, island grammars, and
recursive structures it can't yet handle — without confusing them
with semantic / type-checker / native-lookup gaps (those land in
later passes).

Semantic + dispatch gaps are tracked separately in
[02-deferred-compile-issues.md](02-deferred-compile-issues.md) and
the per-native catalogue in [04-natives-coverage.md](04-natives-coverage.md).

## Summary

| Metric | Count |
|---|---|
| Repos in classpath | 142 (8 platform_* + 134 engine) + 1 embedded (`platform`) |
| Repos parsing clean | **94 / 142** engine + 9 / 9 platform = **103 / 143** |
| Repos with parse errors | 38 / 142 |
| Repos that overflow the parser stack | 10 / 142 |
| Total parse errors across failing repos | **740** |
| Distinct error-message clusters | 8 |

The `legend build` integration path short-circuits at the first
failure (`core_diagram_metamodel`, `Unexpected token Diagram` at
line 16, col 1) and marks the 133 downstream repos as `blocked`.
The breakdown above instead comes from running
`legend check` per-repo, which has no dependency-ordering and so
surfaces every parser gap in parallel.

## Error clusters

Ranked by number of repos affected; secondary number is total error
instances across all of those repos.

| # repos | Cluster | Total errors | Diagnosis |
|---:|---|---:|---|
| 16 | `Unexpected token Diagram` | ~85 | `###Diagram` section parser missing in `legend-pure-rust`. Stub `platform_dsl_diagram` builds clean but doesn't recognise the `###Diagram` keyword as a section header. |
| 7 | `Unexpected token Mapping` | ~70 | `###Mapping` section parser missing. `platform_dsl_mapping` builds clean (provides the metamodel) but no section parser is wired. |
| 5 | `Expected island grammar for tag 'TDS', found identifier` | **420+** | TDS island grammar (`#TDS<...>#`) doesn't accept identifier-form column refs — biggest single-cluster cost (344 errors in `core_functions_relation` alone). |
| 4 | `Unexpected token Database` | ~10 | `###Relational` / `###Database` section parser missing. Same shape as Diagram and Mapping. |
| 3 | `Expected expression, found ':'` | ~12 | Grammar feature gap around `:`-delimited expressions; concentrated in `core_analytics_quality` and the elasticsearch metamodels. |
| 1 | `Expected island grammar for tag '>', found '{'` | 70 | GraphQL-style `#>{...}>#` island handling gap in `core_dataquality_test`. |
| 1 | `Expected identifier, found '\|'` | 23 | `\|`-delimited parameter syntax gap in `core_external_query_relationalai`. |
| 1 | `Expected '>', found '\|'` | 14 | Related `\|`-handling gap in `core_external_query_sql`. |

Stack-overflow group (10 repos), suspected single root cause —
parser recursion blowup on deeper Pure constructs:

```
core
core_external_format_flatdata_java_platform_binding
core_external_format_json
core_external_language_java
core_external_language_java_feature_based_generation
core_external_language_morphir
core_java_platform_binding
core_protocol_generation
core_relational
core_relational_java_platform_binding
```

## Top-20 failing repos by error count

| Errors | Repo | First error |
|---:|---|---|
| 344 | `core_functions_relation` | Expected island grammar for tag 'TDS', found identifier |
| 73 | `core_functions_standard` | Expected island grammar for tag 'TDS', found identifier |
| 70 | `core_dataquality_test` | Expected island grammar for tag '>', found '{' |
| 58 | `core_external_format_openapi` | Unexpected token Diagram |
| 23 | `core_external_query_relationalai` | Expected identifier, found '\|' |
| 21 | `core_snowflake` | Unexpected token Mapping |
| 18 | `core_analytics_lineage` | Unexpected token Mapping |
| 14 | `core_snowflake_test` | Unexpected token Mapping |
| 14 | `core_external_query_sql` | Expected '>', found '\|' |
| 12 | `core_hostedservice` | Unexpected token Mapping |
| 10 | `core_external_format_avro` | Unexpected token Diagram |
| 8 | `core_dataquality` | Expected island grammar for tag 'TDS', found identifier |
| 8 | `core_analytics_quality` | Expected expression, found ':' |
| 7 | `core_external_format_xml` | Unexpected token Mapping |
| 6 | `core_servicestore` | Unexpected token Diagram |
| 6 | `core_scenario_quant` | Expected island grammar for tag 'TDS', found identifier |
| 6 | `core_persistence` | Unexpected token Diagram |
| 5 | `core_analytics_search` | Unexpected token Diagram |
| 4 | `core_relational_snowflake` | Unexpected token Database |
| 4 | `core_elasticsearch_seven_metamodel` | Expected expression, found ':' |

Full list (38 entries) sits in `/tmp/gap-failed-by-count.txt` after a
reproduce run; commit it alongside this report if a permanent
snapshot is wanted.

## Methodology — why two complementary signals

1. **`legend-engine build --classpath … --format json --skip-tests --no-write-purem`**
   walks the classpath in topological dependency order, emits one
   NDJSON object per repo, and aborts at the first failure. Outputs
   `built: 8 + 1 passthrough (platform)`, `failed: 1
   (core_diagram_metamodel)`, `blocked: 133`. Useful for the
   *root-cause* signal (what's the first thing that breaks?) but
   uninformative about parallel gaps further down the graph.
2. **`legend-engine check <repo_source_root> --format json`** per
   repo parses every `.pure` file in isolation, no dependency
   ordering, no semantic phase. Surfaces every parser gap in
   parallel. Source roots are derived as
   `<descriptor_dir>/<descriptor_name>/` — the convention every
   engine + platform repo follows (verified by spot-checking 6
   random entries).

Both are run in the reproduce script. The first locates the
critical-path gap; the second enumerates the full inventory.

## Reproduce

```bash
docs/integration/05-language-gaps.reproduce.sh
```

The script (a) builds `legend-engine-rust-cli` if missing,
(b) regenerates `legend-pure-classpath.toml` if missing,
(c) runs `legend-engine build` against the classpath
(`/tmp/legend-engine-gap.{ndjson,stderr}` — the root-cause signal),
(d) loops `legend-engine check` over each `[[repo]]` in the TOML
(`/tmp/per-repo-check.tsv` — the full inventory),
(e) prints status distribution, top-10 failing repos, and
clusters. Refresh the prose above by hand from those outputs.

A snapshot of the most recent run sits at
`05-language-gaps.per-repo.tsv` next to this file (one row per
repo: name, status, file-count, error-count, first error message).

## What "OK" actually means here

A repo with `status: OK` parses to AST without error. It does *not*
mean the repo compiles end-to-end:

- Type-checker gaps (dispatch narrowing, generic inference) — see
  [02-deferred-compile-issues.md](02-deferred-compile-issues.md).
- Missing natives — see [04-natives-coverage.md](04-natives-coverage.md).
- DSL semantic resolution (Diagram bodies, Mapping bodies, …) —
  separate workstream once the section parsers land.

The next session's compile-level gap report (separate file,
e.g. `06-compile-gaps.md`) will measure those once enough section
parsers land for `legend build` to make it past the parser layer.

## Next-session prioritisation

Ordered by surface-area unlock (number of repos that go from FAIL
to "attempt compile" once the gap closes):

1. **`###Diagram` section parser** — 16 repos. Cheapest first
   win; almost every analytics, format, and authentication repo
   needs it.
2. **TDS island identifier-form** — 5 repos but 420+ errors,
   blocks all of `core_functions_relation`.
3. **`###Mapping` section parser** — 7 repos, foundational for the
   query→SQL chain.
4. **Parser stack-overflow group** — investigate as one cluster;
   likely a common recursion shape. Affects `core`,
   `core_relational`, and 8 others.
5. **`###Database` section parser** — 4 repos, opens the rest of
   the relational store chain (`core_relational_*`).
6. Tail: `core_external_query_*` / elasticsearch `:` and `|` gaps —
   3 repos combined, each with its own narrow feature.

After (1)–(5) land upstream, regenerate this report; the cascade
should shrink the "FAIL" column by ~30 repos and the
stack-overflow column should empty.
