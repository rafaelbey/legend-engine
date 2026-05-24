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

//! Workspace-internal task runner. Today: `gen-classpath`.
//!
//! Invoke via the alias defined in `.cargo/config.toml`:
//! ```bash
//! cargo gen-classpath
//! ```
//!
//! ## `gen-classpath`
//!
//! Walks `legend-engine/` for every `**/src/main/resources/*.definition.json`
//! file, then writes `legend-engine-rust/legend-pure-classpath.toml`
//! with one `[[repo]] kind="filesystem"` entry per descriptor. Adds
//! the 8 `platform_*` purem-artifact repos from `legend-pure/` at the
//! top (the bare-bootstrap `platform` repo enters automatically via
//! `Repo::default_embedded()` — see the upstream
//! `compile_classpath_bytes` → `merge_with_embedded` path).
//!
//! Paths are emitted relative to the TOML's parent directory
//! (`legend-engine-rust/`), so the file is portable across clones as
//! long as `legend-engine/` and `legend-pure/` are siblings.
//!
//! Refuses to emit if any engine descriptor's `dependencies` array
//! references a `platform_*` not in the 8 — surfaces drift early so
//! the next session knows to extend the allowlist rather than chase
//! a silent compile failure inside `legend build`.

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "xtask", about = "legend-engine-rust workspace task runner")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Regenerate legend-pure-classpath.toml from descriptor files on disk.
    GenClasspath,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::GenClasspath => gen_classpath(),
    }
}

// The 8 `platform_*` purem-artifact repos that ship with
// `legend-pure-rust` but are NOT in `Repo::default_embedded()`. Source
// of truth:
// `legend-pure/legend-pure-rust/crates/core-platform-pure/Cargo.toml`,
// `[[package.metadata.legend-pure.repos]]` entries with
// `shape = "purem-artifact"`. The 9th entry there (`platform`,
// `shape = "purem-embedded"`) is intentionally excluded — it comes
// in via the upstream classpath loader's `merge_with_embedded`.
//
// If a new `platform_*` lands upstream, add its descriptor path to
// this list and re-run `cargo gen-classpath`.
const PLATFORM_DESCRIPTORS: &[&str] = &[
    "legend-pure-core/legend-pure-m3-precisePrimitives/src/main/resources/platform_precise_primitives.definition.json",
    "legend-pure-dsl/legend-pure-dsl-store/legend-pure-m2-dsl-store-pure/src/main/resources/platform_dsl_store.definition.json",
    "legend-pure-dsl/legend-pure-dsl-mapping/legend-pure-m2-dsl-mapping-pure/src/main/resources/platform_dsl_mapping.definition.json",
    "legend-pure-dsl/legend-pure-dsl-diagram/legend-pure-m2-dsl-diagram-pure/src/main/resources/platform_dsl_diagram.definition.json",
    "legend-pure-dsl/legend-pure-dsl-graph/legend-pure-m2-dsl-graph-pure/src/main/resources/platform_dsl_graph.definition.json",
    "legend-pure-dsl/legend-pure-dsl-tds/legend-pure-m2-dsl-tds-pure/src/main/resources/platform_dsl_tds.definition.json",
    "legend-pure-dsl/legend-pure-dsl-path/legend-pure-m2-dsl-path-pure/src/main/resources/platform_dsl_path.definition.json",
    "legend-pure-store/legend-pure-store-relational/legend-pure-m2-store-relational-pure/src/main/resources/platform_store_relational.definition.json",
];

/// Names the classpath loader implicitly satisfies via `default_embedded()`
/// (just `platform`) — must not trigger the platform-drift check.
const EMBEDDED_PLATFORM_REPOS: &[&str] = &["platform"];

fn gen_classpath() -> Result<()> {
    // crates/xtask/Cargo.toml -> crates -> legend-engine-rust -> legend-engine
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("can't locate workspace root from {}", manifest_dir.display()))?
        .to_path_buf();
    let engine_root = workspace_root
        .parent()
        .ok_or_else(|| anyhow!("workspace root has no parent: {}", workspace_root.display()))?
        .to_path_buf();
    let pure_root = engine_root
        .parent()
        .ok_or_else(|| anyhow!("engine root has no parent: {}", engine_root.display()))?
        .join("legend-pure");

    // (1) Engine descriptors.
    let engine_descriptors = discover_engine_descriptors(&engine_root)?;
    eprintln!(
        "found {} engine descriptor(s) under {}",
        engine_descriptors.len(),
        engine_root.display()
    );

    // (2) Platform descriptors (hard-listed; verified to exist).
    let platform_descriptors: Vec<PathBuf> = PLATFORM_DESCRIPTORS
        .iter()
        .map(|p| pure_root.join(p))
        .collect();
    for p in &platform_descriptors {
        if !p.is_file() {
            bail!("platform descriptor missing on disk: {}", p.display());
        }
    }

    // (3) Drift check: every `platform_*` referenced by any engine
    // descriptor's `dependencies` array must appear in our allowlist
    // (the 8 hard-listed paths, by repo name) OR in the embedded set.
    let allowed_platform_names: HashSet<String> = platform_descriptors
        .iter()
        .map(|p| read_descriptor_name(p))
        .collect::<Result<_>>()?;
    let referenced = collect_referenced_platforms(&engine_descriptors)?;
    let embedded: HashSet<String> = EMBEDDED_PLATFORM_REPOS.iter().map(|s| (*s).into()).collect();
    let missing: Vec<&String> = referenced
        .iter()
        .filter(|name| !allowed_platform_names.contains(*name) && !embedded.contains(*name))
        .collect();
    if !missing.is_empty() {
        bail!(
            "engine descriptors reference platform_* repo(s) not in PLATFORM_DESCRIPTORS \
             or default_embedded: {:?}. Either add the missing descriptor to xtask's \
             PLATFORM_DESCRIPTORS allowlist or update the dependency in the descriptor.",
            missing
        );
    }

    // (4) Emit TOML.
    let toml_dir = workspace_root.clone();
    let out_path = toml_dir.join("legend-pure-classpath.toml");
    let mut out = String::new();
    out.push_str("# Auto-generated by `cargo gen-classpath`. Do not hand-edit.\n");
    out.push_str("# Source: crates/xtask/src/main.rs\n");
    out.push_str("#\n");
    out.push_str("# Classpath resolution (from upstream crates/cli/src/classpath.rs):\n");
    out.push_str("#   1. --classpath <PATH>                          (highest priority)\n");
    out.push_str("#   2. LEGEND_PURE_CLASSPATH env var\n");
    out.push_str("#   3. legend-pure-classpath.toml in cwd ancestors\n");
    out.push_str("#   4. <exe_dir>/legend-pure-classpath.toml\n");
    out.push_str("#   5. <exe_dir>/snapshots/*.purem (synthetic)\n");
    out.push_str("#   6. LEGEND_PURE_BUILD_SNAPSHOTS_DIR/*.purem\n");
    out.push_str("#   7. Repo::default_embedded() — just `platform`\n");
    out.push_str("#\n");
    out.push_str("# Shadow-by-name: TOML entries replace embedded entries with the same name.\n");
    out.push_str("# Run as:\n");
    out.push_str("#   legend-engine build --classpath legend-pure-classpath.toml --format json --skip-tests\n\n");

    let mut emit = |descs: &[PathBuf], heading: &str| -> Result<usize> {
        out.push_str(&format!("# --- {heading} ---\n\n"));
        let mut entries: Vec<(String, PathBuf)> = descs
            .iter()
            .map(|d| Ok::<_, anyhow::Error>((read_descriptor_name(d)?, d.clone())))
            .collect::<Result<_>>()?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let count = entries.len();
        for (name, desc) in entries {
            let rel = pathdiff::diff_paths(&desc, &toml_dir).unwrap_or(desc.clone());
            out.push_str(&format!(
                "[[repo]]\nname = \"{name}\"\nkind = \"filesystem\"\ndescriptor = \"{}\"\n\n",
                rel.display()
            ));
        }
        Ok(count)
    };

    let platform_count = emit(&platform_descriptors, "platform_* repos (legend-pure)")?;
    let engine_count = emit(&engine_descriptors, "engine repos (legend-engine)")?;

    std::fs::write(&out_path, &out)
        .with_context(|| format!("write {}", out_path.display()))?;
    eprintln!(
        "wrote {} ({} repos: {} platform_* + {} engine)",
        out_path.display(),
        platform_count + engine_count,
        platform_count,
        engine_count,
    );
    Ok(())
}

fn discover_engine_descriptors(engine_root: &Path) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = WalkDir::new(engine_root)
        .into_iter()
        .filter_entry(|entry| {
            // Skip target/, node_modules/, .git/ etc.
            let name = entry.file_name().to_string_lossy();
            !(entry.depth() > 0
                && entry.file_type().is_dir()
                && (name == "target" || name == "node_modules" || name == ".git"))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let p = e.path().to_string_lossy();
            // Engine convention: .definition.json sits under
            // <module>/src/main/resources/<name>.definition.json.
            p.contains("/src/main/resources/")
                && e.file_name()
                    .to_string_lossy()
                    .ends_with(".definition.json")
        })
        .map(|e| e.into_path())
        .collect();
    found.sort();
    Ok(found)
}

fn read_descriptor_name(path: &Path) -> Result<String> {
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&s)
        .with_context(|| format!("parse JSON in {}", path.display()))?;
    v.get("name")
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("descriptor missing 'name' field: {}", path.display()))
}

fn collect_referenced_platforms(descriptors: &[PathBuf]) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for desc in descriptors {
        let s = std::fs::read_to_string(desc)
            .with_context(|| format!("read {}", desc.display()))?;
        let v: serde_json::Value = serde_json::from_str(&s)
            .with_context(|| format!("parse JSON in {}", desc.display()))?;
        if let Some(arr) = v.get("dependencies").and_then(|d| d.as_array()) {
            for dep in arr {
                if let Some(name) = dep.as_str()
                    && name.starts_with("platform")
                {
                    out.insert(name.to_string());
                }
            }
        }
    }
    Ok(out)
}
