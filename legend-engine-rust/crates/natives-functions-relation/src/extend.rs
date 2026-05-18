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

//! `extend<T,Z>(r:Relation<T>[1], f:FuncColSpec<{T[1]->Any[0..1]},Z>[1])
//!   :Relation<T+Z>[1]`.
//!
//! Lambda-per-row template: invoke the FuncColSpec's `function` slot
//! with `$x` bound to a synthetic row-tuple object (same as `filter`),
//! collect the per-row return values, and emit a fresh TDS whose body
//! is the source rows + one appended column.
//!
//! Rendered through `render_csv_from_columns_and_rows` rather than
//! line-slicing the source CSV — the new column changes the cell count
//! per row, so the canonical-CSV rebuild is the simpler shape.

#![allow(clippy::needless_pass_by_value)]

use legend_pure_dsl_tds::csv::{ColumnType, ParsedColumn, TypedCell};
use legend_pure_parser_pure::types::Multiplicity;
use legend_pure_parser_pure::types::ValueSpec;
use smol_str::SmolStr;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use legend_pure_runtime::native::relation::shared::{
    build_row_tuple, read_parsed_tds, render_csv_from_columns_and_rows, unwrap_instance_value,
};

/// `extend(Relation<T>[1], FuncColSpec<{T[1]->Any[0..1]},Z>[1])
///   :Relation<T+Z>[1]`. See module docs for the row-binding template.
#[derive(Debug)]
pub struct ExtendFuncColSpec;

impl NativeFunction for ExtendFuncColSpec {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("extend (Relation, FuncColSpec)", args, 2)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // -- Source TDS --------------------------------------------------
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("extend", &tds_obj, ctx)?;

        // -- FuncColSpec: name + function --------------------------------
        let fcs_value = ctx.evaluate(&args[1])?.into_value();
        let fcs_obj = unwrap_instance_value(&fcs_value, instance_value_id, ctx)?;
        let name_values = ctx
            .heap()
            .get_property_values(&fcs_obj, "name")
            .map_err(PureException::from)?;
        let new_col_name: SmolStr = name_values
            .iter()
            .find_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                PureException::from(PureRuntimeError::EvaluationError(
                    "extend: FuncColSpec.name slot is missing or not a String".into(),
                ))
            })?;
        let function_values = ctx
            .heap()
            .get_property_values(&fcs_obj, "function")
            .map_err(PureException::from)?;
        let function_value = function_values
            .iter()
            .find(|v| matches!(v, Value::Function(_)))
            .ok_or_else(|| {
                PureException::from(PureRuntimeError::EvaluationError(
                    "extend: FuncColSpec.function slot is missing or not a Function".into(),
                ))
            })?
            .clone();

        // -- Per-row evaluation ------------------------------------------
        let mut new_cells: Vec<Option<TypedCell>> = Vec::with_capacity(parsed.rows.len());
        for row in &parsed.rows {
            let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
            let val = ctx.call_function(&function_value, &[Value::Object(row_tuple)])?;
            new_cells.push(value_to_typed_cell(&val));
        }

        // -- New column metadata + extended rows -------------------------
        let (inferred_type, inferred_mult) = infer_column_type_and_mult(&new_cells);
        let mut new_columns = parsed.columns.clone();
        new_columns.push(ParsedColumn {
            name: new_col_name,
            type_tag: inferred_type,
            multiplicity: inferred_mult,
        });
        let mut new_rows: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(parsed.rows.len());
        for (row, new_cell) in parsed.rows.iter().zip(new_cells.into_iter()) {
            let mut extended = row.clone();
            extended.push(new_cell);
            new_rows.push(extended);
        }

        let new_csv = render_csv_from_columns_and_rows(&new_columns, &new_rows);
        let new_tds = ctx.heap_mut().alloc_dynamic(m3_paths::TDS);
        ctx.heap_mut()
            .mutate_add(&new_tds, "csv", &[Value::String(new_csv.into())])
            .map_err(PureException::from)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }

    fn signature(&self) -> &'static str {
        "extend(Relation<T>[1], FuncColSpec<{T[1]->Any[0..1]},Z>[1]):Relation<T+Z>[1]"
    }
}

/// Convert a runtime [`Value`] into the [`TypedCell`] form
/// `render_csv_from_columns_and_rows` consumes. `Unit` collapses to
/// `None` (empty cell). `Object`, `Function`, `Element`, etc. are out
/// of scope for the first FuncColSpec native — extend bodies in PCT
/// tests produce scalar results.
fn value_to_typed_cell(value: &Value) -> Option<TypedCell> {
    match value {
        Value::Unit => None,
        Value::Integer(i) => Some(TypedCell::Integer(*i)),
        Value::Float(f) => Some(TypedCell::Float(*f)),
        Value::Boolean(b) => Some(TypedCell::Boolean(*b)),
        Value::String(s) => Some(TypedCell::String(s.clone())),
        // Multi-value results, objects, functions, elements:
        // unrepresentable as a single cell. Drop to None for now
        // (better error reporting is a follow-up; first FuncColSpec
        // PCT sites produce scalars).
        _ => None,
    }
}

/// Pick a column type + multiplicity from a freshly-extended column's
/// cell values. Mirrors the CSV parser's lightweight inference:
///
/// - Type: pick the first non-empty cell's type, defaulting to `String`
///   if every cell is empty. Mixed cell types collapse to `String`.
/// - Multiplicity: `[1]` if every cell is `Some`, else `[0..1]`.
fn infer_column_type_and_mult(cells: &[Option<TypedCell>]) -> (ColumnType, Multiplicity) {
    let mut tag: Option<ColumnType> = None;
    let mut any_empty = false;
    for cell in cells {
        let Some(cell) = cell else {
            any_empty = true;
            continue;
        };
        let cell_tag = match cell {
            TypedCell::Integer(_) => ColumnType::Integer,
            TypedCell::Float(_) => ColumnType::Float,
            TypedCell::Boolean(_) => ColumnType::Boolean,
            TypedCell::String(_) => ColumnType::String,
            TypedCell::Decimal(_) => ColumnType::Decimal,
            TypedCell::StrictDate(_) => ColumnType::StrictDate,
            TypedCell::DateTime(_) => ColumnType::DateTime,
        };
        tag = Some(match tag {
            None => cell_tag,
            Some(prev) if prev == cell_tag => prev,
            _ => ColumnType::String,
        });
    }
    let mult = if any_empty {
        Multiplicity::ZeroOrOne
    } else {
        Multiplicity::PureOne
    };
    (tag.unwrap_or(ColumnType::String), mult)
}
