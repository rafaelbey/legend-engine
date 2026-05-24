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

//! Asserts the built `liblegend_engine_jni.{dylib,so,dll}` exports
//! every `Java_org_finos_legend_pure_rust_*` symbol the stock
//! `legend-pure-parser-jni` cdylib ships. Mirrors the upstream
//! `legend-pure-rust/examples/mydsl-jni-extension/tests/symbols.rs`
//! verbatim, swapping only the cdylib filename.
//!
//! Catches the regression where upstream adds a new `Java_*` entry
//! point and this crate's `src/lib.rs` forgets to forward it — Java
//! callers loading `liblegend_engine_jni` would silently lose the new
//! symbol.

use std::path::PathBuf;
use std::process::Command;

/// All `Java_*` symbols downstream cdylibs must export to be a
/// drop-in replacement for `libpure_rust_jni`. Sourced from
/// `legend-pure-rust/crates/jni/src/lib.rs` + `codegen.rs` —
/// keep in sync when adding upstream entries.
const EXPECTED_SYMBOLS: &[&str] = &[
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeInitContext",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeInitContextWithClasspath",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeEvaluate",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeGetProperty",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeGetClassifier",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeNew",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeFreeContext",
    "Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeFreeInstance",
    "Java_org_finos_legend_pure_rust_bindings_PureBindingsGenerator_nativeGenerateBindings",
];

fn cdylib_path() -> PathBuf {
    // Tests run with CWD = the crate root, so `target/` is the
    // workspace's target/ via cargo's symlink/path resolution. The
    // cdylib lands at `target/{debug,release}/<prefix><lib>.<ext>`.
    // We default to `debug` because `cargo test` builds debug by
    // default; `--release` users should set CARGO_TARGET_DIR or
    // verify by hand.
    let ext = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let prefix = if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    };
    // Workspace target/ is two levels up from this crate's manifest dir.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug")
        .join(format!("{prefix}legend_engine_jni.{ext}"))
}

#[test]
fn cdylib_exports_every_upstream_java_symbol() {
    let cdylib = cdylib_path();
    if !cdylib.exists() {
        // Fresh-clone case: `cargo test` invocation order can race
        // ahead of the cdylib's first link. Force a build.
        let status = Command::new("cargo")
            .args(["build", "-p", "legend-engine-rust-jni"])
            .status()
            .expect("cargo build must succeed");
        assert!(status.success(), "cargo build failed");
        assert!(
            cdylib.exists(),
            "cdylib still missing after cargo build: {}",
            cdylib.display(),
        );
    }

    if cfg!(target_os = "windows") {
        eprintln!(
            "symbols.rs: skipping symbol scan on Windows — verify via `dumpbin /EXPORTS`"
        );
        return;
    }
    let nm_args: Vec<&str> = if cfg!(target_os = "macos") {
        vec!["-gU"]
    } else {
        vec!["-D", "--defined-only"]
    };
    let output = Command::new("nm")
        .args(&nm_args)
        .arg(&cdylib)
        .output()
        .expect("nm must be available");
    assert!(
        output.status.success(),
        "nm failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // macOS prefixes user symbols with an extra leading underscore;
    // strip it for cross-platform matching.
    let normalize = |name: &str| {
        if cfg!(target_os = "macos") {
            format!(" _{name}")
        } else {
            format!(" {name}")
        }
    };

    let mut missing: Vec<&str> = Vec::new();
    for expected in EXPECTED_SYMBOLS {
        let needle = normalize(expected);
        if !stdout.contains(&needle) {
            missing.push(expected);
        }
    }
    assert!(
        missing.is_empty(),
        "cdylib {} is missing JNI symbols: {missing:#?}\n\n\
         Forwarder regression — every `Java_*` in EXPECTED_SYMBOLS must \
         have a matching `#[no_mangle] pub extern \"system\" fn` in \
         src/lib.rs.",
        cdylib.display(),
    );
}
