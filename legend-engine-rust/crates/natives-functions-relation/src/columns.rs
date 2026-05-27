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
//
//! `columns(rel:Relation<T>[1]):Column<T>[*]` — read TDS column metadata.
//!
//! Returns a Pure-side multi-value of `Column` heap objects, one per
//! column in the receiver TDS. Each Column object carries at least the
//! `name: String[1]` slot (the canonical PCT test reads `$t->columns().name`).
//! Re-parses the receiver's `csv` slot via [`legend_pure_runtime::native::relation::shared::read_parsed_tds`]
//! to enumerate columns; allocator does not retain a parsed structure
//! on the heap object — see the note in `shared.rs`.
//!
//! The output multiplicity `[*]` is encoded as `Value::Collection`
//! (RRB-tree of `Value::Object`); a single-column TDS still returns a
//! one-element Collection (Pure's Many representation, not unwrapped to
//! a bare `Value::Object`).

#![allow(clippy::needless_pass_by_value)]

use im_rc::Vector as PVector;
use legend_pure_parser_pure::ids::ElementId;
use legend_pure_parser_pure::types::ValueSpec;

use legend_pure_runtime::error::PureException;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use legend_pure_runtime::native::relation::shared::{read_parsed_tds, unwrap_instance_value};
use legend_pure_runtime::relation::{alloc_multiplicity, column_type_element};

/// Pure
/// `columns<T>(rel:Relation<T>[1]):Column<T>[*]`.
#[derive(Debug)]
pub struct Columns;

impl NativeFunction for Columns {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("columns (Relation)", args, 1)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
        let value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("columns", &tds_obj, ctx)?;

        // Pre-resolve every element-id we need from the model before
        // we touch the heap mutably — `EvalContextTrait` exposes
        // `model()` and `heap_mut()` via `&self` / `&mut self`, so
        // through the dyn trait Rust won't let us hold both refs at
        // once. After this block, the loop body needs only `heap_mut`.
        let column_class_value =
            m3_paths::resolve(ctx.model(), m3_paths::COLUMN).map_or(Value::Unit, Value::Element);
        let per_column_type_ids: Vec<ElementId> = parsed
            .columns
            .iter()
            .map(|c| {
                column_type_element(ctx.model(), &c.type_tag)
                    .unwrap_or(legend_pure_parser_pure::bootstrap::ANY_ID)
            })
            .collect();

        // Each Column heap object carries `name`, `nameWildCard`, and
        // `classifierGenericType = ^GT(rawType=Column,
        // typeArguments=[Unit, ^GT(rawType=<value-type>)],
        // multiplicityArguments=[^Multiplicity(<mult>)])`. The
        // classifierGenericType is what the `assertTdsEquivalent`
        // reflection walk in `tdsEquivalent.pure:31` reads via
        // `$col.classifierGenericType.typeArguments->at(1).rawType
        //   ->toOne()->subTypeOf(Number)`. The shape mirrors
        // `legend_pure_runtime::relation::alloc_column`; inlined here
        // because that helper takes `&mut RuntimeHeap` and `&PureModel`
        // simultaneously, which the dyn trait can't expose without
        // `unsafe` (forbid(unsafe_code) on this crate).
        let mut out: PVector<Value> = PVector::new();
        for (col, &type_id) in parsed.columns.iter().zip(per_column_type_ids.iter()) {
            let heap = ctx.heap_mut();
            let mult_obj = alloc_multiplicity(heap, &col.multiplicity)?;

            let inner_gt = heap.alloc_dynamic(m3_paths::GENERIC_TYPE);
            heap.mutate_add(&inner_gt, "rawType", &[Value::Element(type_id)])
                .map_err(PureException::from)?;

            let outer_gt = heap.alloc_dynamic(m3_paths::GENERIC_TYPE);
            heap.mutate_add(&outer_gt, "rawType", &[column_class_value.clone()])
                .map_err(PureException::from)?;
            heap.mutate_add(
                &outer_gt,
                "typeArguments",
                &[Value::Unit, Value::Object(inner_gt)],
            )
            .map_err(PureException::from)?;
            heap.mutate_add(
                &outer_gt,
                "multiplicityArguments",
                &[Value::Object(mult_obj)],
            )
            .map_err(PureException::from)?;

            let column_obj = heap.alloc_dynamic(m3_paths::COLUMN);
            heap.mutate_add(&column_obj, "name", &[Value::String(col.name.clone())])
                .map_err(PureException::from)?;
            heap.mutate_add(&column_obj, "nameWildCard", &[Value::Boolean(false)])
                .map_err(PureException::from)?;
            heap.mutate_add(
                &column_obj,
                "classifierGenericType",
                &[Value::Object(outer_gt)],
            )
            .map_err(PureException::from)?;
            out.push_back(Value::Object(column_obj));
        }
        Ok(Evaluated::new(Value::Collection(Box::new(out))))
    }
}
