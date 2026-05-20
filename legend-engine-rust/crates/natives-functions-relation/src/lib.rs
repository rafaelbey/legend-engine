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

//! Runtime native implementations for the `core_functions_relation`
//! Pure extension.
//!
//! This crate hosts the Rust impls of every native function declared in
//! legend-engine's `core_functions_relation/relation/**/*.pure` source
//! tree. Sister crate
//! [`legend_engine_rust_core_functions_relation_pure`] embeds the
//! `.pure` source at build time; this crate ships the executable
//! bodies.
//!
//! Mirrors Java's `FunctionExtensionInterpreted` / `FunctionExtensionCompiled`
//! layering: the platform stays the minimum surface every Pure program
//! can call; consumer-defined natives (`filter` on Relation, `sort` on
//! Relation, `extend`, etc.) live in their own crate and register at
//! runtime via the
//! [`RuntimeExtension`](legend_pure_runtime::native::RuntimeExtension)
//! SPI.
//!
//! ```ignore
//! use legend_engine_rust_natives_functions_relation::RelationFunctionsExtension;
//! use legend_pure_runtime::native::NativeRegistry;
//!
//! let registry = NativeRegistry::with_extensions(&[&RelationFunctionsExtension]);
//! // registry now contains the platform standard set + relation
//! // natives like filter/sort/extend/distinct/…
//! ```
//!
//! The shared helpers these natives rely on (`build_row_tuple`,
//! `read_parsed_tds`, `render_csv_from_columns_and_rows`,
//! `unwrap_instance_value`, …) live in
//! `legend_pure_runtime::native::relation::shared`. The two
//! platform-defined relation natives that stay in legend-pure-rust
//! (`addColumns` and `stringToTDS`) also consume them — keeping the
//! helpers there keeps the abstraction in one place.

mod ascending;
mod columns;
mod concatenate;
mod descending;
mod distinct;
mod drop;
mod extend;
mod filter;
mod limit;
mod map;
mod rename;
mod select;
mod size;
mod sort;
mod sort_info;
mod tostring;

pub use ascending::Ascending;
pub use columns::Columns;
pub use concatenate::Concatenate;
pub use descending::Descending;
pub use distinct::{Distinct, DistinctColSpecArray};
pub use drop::Drop;
pub use extend::{ExtendFuncColSpec, ExtendFuncColSpecArray};
pub use filter::Filter;
pub use limit::Limit;
pub use map::MapRelation;
pub use rename::Rename;
pub use select::{SelectAll, SelectColSpec, SelectColSpecArray};
pub use size::Size;
pub use sort::Sort;
pub use tostring::{ToStringRelation, ToStringRelationTyped};

use legend_pure_runtime::native::{NativeRegistry, RuntimeExtension};

/// Registers every native shipped by this crate into a
/// [`NativeRegistry`]. The canonical entry point for Engine consumers
/// who want to evaluate `core_functions_relation` Pure source.
#[derive(Debug, Default, Clone, Copy)]
pub struct RelationFunctionsExtension;

impl RuntimeExtension for RelationFunctionsExtension {
    fn name(&self) -> &'static str {
        "core_functions_relation"
    }

    fn register_natives(&self, registry: &mut NativeRegistry) {
        registry.register("size_Relation_1__Integer_1_", Size);
        registry.register("distinct_Relation_1__Relation_1_", Distinct);
        registry.register(
            "distinct_Relation_1__ColSpecArray_1__Relation_1_",
            DistinctColSpecArray,
        );
        registry.register("map_Relation_1__Function_1__V_MANY_", MapRelation);
        registry.register(
            "concatenate_Relation_1__Relation_1__Relation_1_",
            Concatenate,
        );
        registry.register("filter_Relation_1__Function_1__Relation_1_", Filter);
        registry.register(
            "extend_Relation_1__FuncColSpec_1__Relation_1_",
            ExtendFuncColSpec,
        );
        registry.register(
            "extend_Relation_1__FuncColSpecArray_1__Relation_1_",
            ExtendFuncColSpecArray,
        );
        registry.register("limit_Relation_1__Integer_1__Relation_1_", Limit);
        registry.register("drop_Relation_1__Integer_1__Relation_1_", Drop);
        registry.register(
            "rename_Relation_1__ColSpec_1__ColSpec_1__Relation_1_",
            Rename,
        );
        registry.register("select_Relation_1__Relation_1_", SelectAll);
        registry.register("select_Relation_1__ColSpec_1__Relation_1_", SelectColSpec);
        registry.register(
            "select_Relation_1__ColSpecArray_1__Relation_1_",
            SelectColSpecArray,
        );
        registry.register("columns_Relation_1__Column_MANY_", Columns);
        registry.register("sort_Relation_1__SortInfo_MANY__Relation_1_", Sort);
        registry.register("ascending_ColSpec_1__SortInfo_1_", Ascending);
        registry.register("descending_ColSpec_1__SortInfo_1_", Descending);
        // `toString(Relation)` is Pure-defined as
        // `<<PCT.function, PCT.platformOnly>>` in
        // `core_functions_relation/.../toString.pure`. We register
        // engine natives with the same mangled FQNs; the runtime's
        // native lookup (in `eval.rs::call_function` step 2a) runs
        // before falling through to the function body, so these
        // override the Pure impl at runtime. The Pure body stays in
        // source for compile-time signature checking.
        registry.register("toString_Relation_1__String_1_", ToStringRelation);
        registry.register(
            "toString_Relation_1__Boolean_1__String_1_",
            ToStringRelationTyped,
        );
    }
}
