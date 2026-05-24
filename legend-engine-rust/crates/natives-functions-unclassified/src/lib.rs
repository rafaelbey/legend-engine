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

//! Runtime native implementations for the
//! `core_functions_unclassified` Pure extension.
//!
//! This crate hosts the Rust impls of every native function declared in
//! `legend-engine`'s `core_functions_unclassified/lang/*.pure` source
//! tree. Sister crate
//! [`legend_engine_rust_core_functions_unclassified_pure`] embeds the
//! `.pure` source at build time; this crate ships the executable bodies.
//!
//! Consumers register these natives through the
//! [`RuntimeExtension`](legend_pure_runtime::native::RuntimeExtension)
//! SPI exposed by `legend-pure-runtime`:
//!
//! ```ignore
//! use legend_engine_rust_natives_functions_unclassified::FunctionsUnclassifiedExtension;
//! use legend_pure_runtime::native::NativeRegistry;
//!
//! let registry = NativeRegistry::with_extensions(&[&FunctionsUnclassifiedExtension]);
//! // registry now contains the platform standard set + mutateAdd.
//! ```
//!
//! Mirrors Java's `FunctionExtensionInterpreted` / `FunctionExtensionCompiled`
//! layering: the platform stays the minimum surface every Pure program
//! can call; consumer-defined natives (`mutateAdd` and friends) live in
//! their own crate and register at runtime. See
//! `legend-pure-rust/docs/runtime/metaprogramming.md` §5 for the
//! invariant this layering enforces.

use legend_pure_parser_pure::types::ValueSpec;
use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::native::{
    EvalContextTrait, Evaluated, NativeFunction, NativeRegistry, RUNTIME_EXTENSIONS,
    RuntimeExtension, expect_args, force_all,
};
use legend_pure_runtime::value::Value;
use linkme::distributed_slice;

// ---------------------------------------------------------------------------
// mutateAdd<T>(T[1], String[1], Any[*]):T[1]
// ---------------------------------------------------------------------------

/// Pure
/// `mutateAdd<T>(obj:T[1], property:String[1], value:Any[*]):T[1]`
///
/// Appends `value`'s elements to the named property on `obj`, then
/// returns `obj`. Mirrors Java
/// `legend-engine-pure-runtime-java-extension-interpreted-functions-unclassified/.../MutateAdd.java`:
/// the property name is a runtime string, the values are `Any[*]`, and
/// the mutation happens in-place through the runtime heap layer.
///
/// **Where this is allowed to be called from:** only Pure source that
/// has loaded the `core_functions_unclassified` repo. The platform
/// `NativeRegistry::standard()` does not expose this native — that's
/// the §5 invariant locked by
/// `legend-pure-rust/crates/runtime/tests/platform_invariants.rs`.
///
/// Internally this delegates to
/// `RuntimeHeap::mutate_add(handle, property, &values)`, which is the
/// same primitive `Reactivate` and `Copy` already use for their own
/// result materialisation. The new aspect of registering `mutateAdd`
/// is making that primitive *callable from Pure*, with the read-only
/// contract waived for code that opts into the extension.
#[derive(Debug)]
pub struct MutateAdd;

impl NativeFunction for MutateAdd {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        let values = force_all(args, ctx)?;
        expect_args("mutateAdd", &values, 3)?;

        let obj = match &values[0] {
            Value::Object(handle) => handle.clone(),
            other => {
                return Err(PureRuntimeError::EvaluationError(format!(
                    "mutateAdd: expected an object as the first argument, got {}",
                    other.type_name()
                ))
                .into());
            }
        };

        let property = match &values[1] {
            Value::String(s) => s.clone(),
            other => {
                return Err(PureRuntimeError::EvaluationError(format!(
                    "mutateAdd: expected a String as the second argument, got {}",
                    other.type_name()
                ))
                .into());
            }
        };

        // `Any[*]` flattens uniformly: Collection unrolls its elements,
        // Unit contributes nothing, anything else is a single appended
        // value. Mirrors how Java's MutateAdd treats `RichIterable<? extends Object>`.
        let appended: Vec<Value> = match &values[2] {
            Value::Collection(coll) => coll.iter().cloned().collect(),
            Value::Unit => Vec::new(),
            single => vec![single.clone()],
        };

        ctx.heap_mut()
            .mutate_add(&obj, property.as_str(), &appended)?;

        Ok(Evaluated::new(Value::Object(obj)))
    }
}

// ---------------------------------------------------------------------------
// FunctionsUnclassifiedExtension — RuntimeExtension binding
// ---------------------------------------------------------------------------

/// Registers every native shipped by this crate into a
/// [`NativeRegistry`]. The canonical entry point for Engine consumers
/// who want to evaluate `core_functions_unclassified` Pure source.
#[derive(Debug, Default, Clone, Copy)]
pub struct FunctionsUnclassifiedExtension;

impl RuntimeExtension for FunctionsUnclassifiedExtension {
    fn name(&self) -> &'static str {
        "core_functions_unclassified"
    }

    fn register_natives(&self, registry: &mut NativeRegistry) {
        // Mangled name follows the standard convention
        // (FunctionSignature::mangled_name). `mutateAdd<T>` →
        // `mutateAdd_T_1__String_1__Any_MANY__T_1_`.
        registry.register("mutateAdd_T_1__String_1__Any_MANY__T_1_", MutateAdd);
    }
}

/// Self-registration into the runtime's `RUNTIME_EXTENSIONS`
/// distributed slice so any binary or cdylib that depends on this
/// crate picks up the unclassified natives via
/// `NativeRegistry::discovered()` with no per-binary wiring. The
/// existing `with_extensions(&[&FunctionsUnclassifiedExtension])` path
/// (used by `integration-smoke`) is unaffected.
#[distributed_slice(RUNTIME_EXTENSIONS)]
static FUNCTIONS_UNCLASSIFIED_EXTENSION: &(dyn RuntimeExtension + Sync) =
    &FunctionsUnclassifiedExtension;
