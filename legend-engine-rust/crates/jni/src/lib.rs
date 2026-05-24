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

//! `legend_engine_jni` — engine-flavoured JNI cdylib.
//!
//! Two responsibilities, mirroring the upstream worked example at
//! `legend-pure-rust/examples/mydsl-jni-extension/`:
//!
//! 1. **Force-link engine extension crates** so their
//!    `#[distributed_slice(RUNTIME_EXTENSIONS)]` statics reach the
//!    final cdylib's link graph. Without these `use … as _;` lines
//!    rustc's linker drops the crates (no reachable code →
//!    no contribution to `RUNTIME_EXTENSIONS` / `COMPILER_EXTENSIONS`
//!    / `DSL_POPULATORS`) and the JNI dispatch path inside the
//!    upstream rlib silently sees an empty extension set.
//!
//! 2. **Re-export every `Java_*` entry point** via thin
//!    `#[no_mangle] pub extern "system" fn` forwarders that delegate
//!    to the upstream rlib's implementations. This is the canonical
//!    Rust-JNI cdylib-on-rlib idiom — rustc's default cdylib build
//!    dead-code-eliminates unreachable rlib code, so the upstream's
//!    `#[no_mangle]` doesn't carry through. The forwarders'
//!    `#[no_mangle]` is what guarantees the symbols ship in the
//!    final cdylib's export table.
//!
//! After build, verify symbol parity with the stock upstream cdylib:
//!
//! ```bash
//! cargo build --release -p legend-engine-rust-jni
//! nm -gU target/release/liblegend_engine_jni.dylib | grep ' _Java_' | wc -l
//! # expect 8 — matches libpure_rust_jni.dylib's count
//! ```
//!
//! See `tests/symbols.rs` for the in-tree assertion.

#![allow(unused_imports)]

// (1) Force-link engine extensions. Same set as crates/cli; same rationale.
use legend_engine_rust_natives_functions_relation::RelationFunctionsExtension as _;
use legend_engine_rust_natives_functions_unclassified::FunctionsUnclassifiedExtension as _;

// (2) Forwarder declarations — one per upstream `Java_*` entry point.
// Each forwarder is a thin `#[no_mangle] pub extern "system" fn` that
// delegates to the upstream rlib's implementation. The upstream fns are
// also `pub extern "system" fn`, which Rust can invoke like any function.
//
// When upstream adds a new `Java_*` entry, add a forwarder here. The
// `tests/symbols.rs` test catches missing forwarders in CI.

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString};
use jni::sys::jlong;

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeInitContext<
    'local,
>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
) -> jlong {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeInitContext(env, class)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeInitContextWithClasspath<
    'local,
>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    classpath_bytes: JByteArray<'local>,
) -> jlong {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeInitContextWithClasspath(
        env,
        class,
        classpath_bytes,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeEvaluate<'local>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    context_ptr: jlong,
    function_path: JString<'local>,
    args: JObjectArray<'local>,
) -> JObject<'local> {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeEvaluate(
        env,
        class,
        context_ptr,
        function_path,
        args,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeGetProperty<
    'local,
>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    context_ptr: jlong,
    complex_ptr: jlong,
    property_name: JString<'local>,
    args: JObjectArray<'local>,
) -> JObject<'local> {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeGetProperty(
        env,
        class,
        context_ptr,
        complex_ptr,
        property_name,
        args,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeGetClassifier<
    'local,
>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    context_ptr: jlong,
    complex_ptr: jlong,
) -> JString<'local> {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeGetClassifier(
        env,
        class,
        context_ptr,
        complex_ptr,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeNew<'local>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    context_ptr: jlong,
    classifier_fqn: JString<'local>,
    property_names: JObjectArray<'local>,
    property_values: JObjectArray<'local>,
) -> jlong {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeNew(
        env,
        class,
        context_ptr,
        classifier_fqn,
        property_names,
        property_values,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeFreeContext<
    'local,
>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    context_ptr: jlong,
) {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeFreeContext(
        env,
        class,
        context_ptr,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeFreeInstance<
    'local,
>(
    env: JNIEnv<'local>,
    class: JClass<'local>,
    context_ptr: jlong,
    complex_ptr: jlong,
) {
    pure_rust_jni::Java_org_finos_legend_pure_rust_PureRustEvaluator_nativeFreeInstance(
        env,
        class,
        context_ptr,
        complex_ptr,
    )
}

// `Java_org_finos_legend_pure_rust_bindings_PureBindingsGenerator_nativeGenerateBindings`
// lives in an upstream-private module (`pure_rust_jni::codegen`) and
// can't be forwarded by name. The `#[no_mangle]` on its upstream
// definition keeps the symbol in the rlib's object output, and the
// linker preserves `#[no_mangle]` symbols through cdylib link without
// DCE — so it ships in `liblegend_engine_jni` even without an
// explicit forwarder here. `tests/symbols.rs` asserts presence; if the
// symbol disappears (e.g., upstream removes `#[no_mangle]` or relocates
// the fn), the test fails and we add an explicit forwarder once the
// upstream item is made reachable.
