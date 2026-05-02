// Copyright 2026 Goldman Sachs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Lock for the engine-side Pure repo composition.
//!
//! Combines `Repo::default_embedded()` (the upstream platform set
//! shipped by `legend-pure-rust`) with this workspace's engine-side
//! repo embeddings, then runs the Rust compiler over the union. Locks
//! the current state with regression-detection so a new error type or
//! a count change can't sneak in unnoticed.
//!
//! Two tests:
//!
//! - `engine_repos_compile_clean` (`#[ignore]`'d) — the long-term lock.
//!   Lifts when `KNOWN_ERROR_COUNT` reaches 0.
//! - `engine_repos_compile_with_known_errors` — the regression-
//!   detection lock. Asserts every error matches a cataloged pattern
//!   (`KNOWN_ERRORS`) and the total count equals `KNOWN_ERROR_COUNT`.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, Repo};
use smol_str::SmolStr;

/// Currently-known compile failures against the engine-side repos this
/// workspace embeds. Each entry is `(canonical_url_prefix, fragment_of_message)`.
/// Every entry is a real parser/compiler gap in `legend-pure-rust` (Java
/// compiles all of these cleanly). They are filed upstream and unlocked
/// as those fixes land. Categories:
///
/// 1. `core_functions_relation` / `core_functions_standard` — TDS island
///    grammar, Unicode lexer chars (⊆, ?, "), generic-type covariance
///    modifier (`+T`), annotation/list start tokens, overload narrowing
///    for `toString`/`plus`, and cascading "Cannot resolve function"
///    failures downstream of the parser errors.
/// 2. `platform_store_relational` — compiler-side overload narrowing for
///    `elementToPath` (3 candidates).
///
/// **Note:** the original 7-error catalog for `core_functions_unclassified`
/// (keyword-reservation, qualified-name-in-expression, type-with-multiplicity
/// in match patterns) is no longer needed — the parser fixes landed upstream
/// in `legend-pure-rust` and those errors are now compile-clean.
const KNOWN_ERRORS: &[(&str, &str)] = &[
    // --- core_functions_relation parser gaps ---
    // TDS island-grammar bodies inside #TDS\n...# fixtures.
    (
        "/core_functions_relation/",
        "Expected island grammar for tag 'TDS'",
    ),
    // Lexer rejects Unicode subset symbol used in type signatures.
    (
        "/core_functions_relation/",
        "Unexpected character '⊆'",
    ),
    // Lexer rejects '?' (used in optional/multiplicity notation).
    (
        "/core_functions_relation/",
        "Unexpected character '?'",
    ),
    // Lexer rejects '\"' (smart-quote / non-ASCII quote variants).
    (
        "/core_functions_relation/",
        "Unexpected character '\"'",
    ),
    // Generic-type covariance modifier: parser expects '>' before '+'.
    (
        "/core_functions_relation/",
        "Expected '>', found '+'",
    ),
    // Compiler overload narrowing not yet handling toString/plus calls.
    (
        "/core_functions_relation/",
        "Ambiguous function call 'toString'",
    ),
    (
        "/core_functions_relation/",
        "Ambiguous function call 'plus'",
    ),
    // --- core_functions_standard parser + cascading-resolution gaps ---
    (
        "/core_functions_standard/",
        "Expected island grammar for tag 'TDS'",
    ),
    (
        "/core_functions_standard/",
        "Expected '{', found '['",
    ),
    (
        "/core_functions_standard/",
        "Unexpected character '?'",
    ),
    // Cascading: parser failed to register these functions, so call
    // sites cannot resolve them. Will clear when parser gaps lift.
    (
        "/core_functions_standard/",
        "Cannot resolve function",
    ),
    // --- platform_store_relational compiler dispatch gaps ---
    // `relationalRuntime.pure` calls `elementToPath` and the compiler
    // can't narrow between 3 overloads. Upstream-side compiler issue.
    (
        "/platform_store_relational/",
        "Ambiguous function call 'elementToPath'",
    ),
];

/// Number of compile errors the regression-detection lock expects.
///
/// Sum of the categories above as observed against the current repo composition
/// (`core_functions_*` engine-side + `platform_store_relational` upstream).
/// Bump when a new dispatch issue is added; reduce when a known fix lands.
const KNOWN_ERROR_COUNT: usize = 237;

fn compose_repos() -> Vec<Repo> {
    let mut repos: Vec<Repo> = Repo::default_embedded();
    repos.extend(legend_engine_rust_core_functions_json_pure::repos());
    repos.extend(legend_engine_rust_core_functions_unclassified_pure::repos());
    repos.extend(legend_engine_rust_core_functions_variant_pure::repos());
    repos.extend(legend_engine_rust_core_functions_relation_pure::repos());
    repos.extend(legend_engine_rust_core_functions_standard_pure::repos());
    repos
}

#[test]
fn engine_repos_compile_clean() {
    if KNOWN_ERROR_COUNT > 0 {
        // Skip this strict variant while there are cataloged failures.
        // The regression-detection variant carries the load.
        eprintln!(
            "skipping strict compile-clean: KNOWN_ERROR_COUNT = {KNOWN_ERROR_COUNT}; \
             see engine_repos_compile_with_known_errors"
        );
        return;
    }
    let repos = compose_repos();
    let auto_imports: Vec<SmolStr> = PLATFORM_AUTO_IMPORTS
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect();

    match repo::load(&repos, &auto_imports) {
        Ok(model) => {
            assert!(!model.chunks.is_empty(), "expected non-empty model");
            println!(
                "platform + engine-side repos compiled clean: {} chunks across {} repos",
                model.chunks.len(),
                repos.len()
            );
        }
        Err(partial) => {
            let mut report = String::new();
            for (i, e) in partial.errors.iter().enumerate() {
                report.push_str(&format!(
                    "  [{i}] {} ({}:{}-{}:{})\n    {}\n",
                    e.source_info.source,
                    e.source_info.start_line,
                    e.source_info.start_column,
                    e.source_info.end_line,
                    e.source_info.end_column,
                    e.message,
                ));
            }
            panic!(
                "expected clean compile, got {} error(s):\n{report}",
                partial.errors.len()
            );
        }
    }
}

#[test]
fn engine_repos_compile_with_known_errors() {
    let repos = compose_repos();
    let auto_imports: Vec<SmolStr> = PLATFORM_AUTO_IMPORTS
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect();

    match repo::load(&repos, &auto_imports) {
        Ok(_) if KNOWN_ERROR_COUNT == 0 => {
            // Clean compile when the catalog is empty. The strict
            // test handles this case; nothing to assert here.
        }
        Ok(_) => panic!(
            "compile is now clean — set KNOWN_ERROR_COUNT to 0 in this test, \
             clear KNOWN_ERRORS, and lift any related #[ignore]s."
        ),
        Err(partial) => {
            assert!(!partial.model.chunks.is_empty(), "partial model still has elements");

            let mut unrecognised: Vec<String> = Vec::new();
            for e in &partial.errors {
                let path = e.source_info.source.as_str();
                let known = KNOWN_ERRORS
                    .iter()
                    .any(|(prefix, frag)| path.starts_with(prefix) && e.message.contains(frag));
                if !known {
                    unrecognised.push(format!(
                        "{path}:{}:{} {}",
                        e.source_info.start_line, e.source_info.start_column, e.message
                    ));
                }
            }
            assert!(
                unrecognised.is_empty(),
                "{} unrecognised compile error(s); the engine-side repo set has \
                 regressed beyond the cataloged failures:\n{}",
                unrecognised.len(),
                unrecognised.join("\n")
            );
            assert_eq!(
                partial.errors.len(),
                KNOWN_ERROR_COUNT,
                "error count drifted from cataloged baseline of {KNOWN_ERROR_COUNT}; \
                 update KNOWN_ERROR_COUNT (and/or KNOWN_ERRORS) and re-run. \
                 Current count: {}",
                partial.errors.len()
            );
        }
    }
}
