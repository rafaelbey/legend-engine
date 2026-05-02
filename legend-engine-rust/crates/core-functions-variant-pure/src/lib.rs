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

#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Embeds legend-engine's `core_functions_variant` Pure repo into the
//! binary.
//!
//! Layer-3 repo: deps `["platform", "core_functions_json"]`. Pattern
//! covers `meta::pure::functions::variant`. The caller must compose
//! `core_functions_json::repos()` ahead of this one for the dependency
//! check to be satisfiable downstream.

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/legend_pure_repos.rs"));
}

/// Embedded `core_functions_variant` repo. Combine with
/// [`legend_pure_core_platform::repo::Repo::default_embedded`],
/// `core_functions_json`, and other engine-side repos before calling
/// [`legend_pure_core_platform::repo::load`].
#[must_use]
pub fn repos() -> Vec<::legend_pure_core_platform::repo::Repo> {
    embedded::default_embedded_repos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_repo_metadata_matches_descriptor() {
        let repos = repos();
        assert_eq!(repos.len(), 1, "expected exactly one embedded repo");
        let meta = repos[0].meta().expect("embedded repo carries meta");
        assert_eq!(meta.name, "core_functions_variant");
        assert_eq!(
            meta.dependencies,
            &["platform", "core_functions_json"]
        );
        assert!(
            meta.pattern.contains("variant"),
            "expected pattern to include variant namespace, got: {}",
            meta.pattern
        );
    }

    #[test]
    fn embedded_repo_has_pure_files() {
        let repos = repos();
        let pure_count: usize = repos.iter().map(|r| r.sources().count()).sum();
        assert!(
            pure_count >= 1,
            "expected at least one .pure file in core_functions_variant, got {pure_count}"
        );
    }

    #[test]
    fn embedded_files_carry_canonical_urls() {
        let repos = repos();
        let any = repos
            .iter()
            .flat_map(|r| r.sources())
            .any(|(_, path)| path.starts_with("/core_functions_variant/"));
        assert!(
            any,
            "expected at least one /core_functions_variant/* file"
        );
    }
}
