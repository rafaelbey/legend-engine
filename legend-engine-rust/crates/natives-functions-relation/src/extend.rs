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

use legend_pure_dsl_tds::csv::{ColumnType, ParsedColumn, ParsedTDS, TypedCell};
use legend_pure_parser_pure::types::{ColSpecLiteralKind, ExprKind, Multiplicity, ValueSpec};
use smol_str::SmolStr;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
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

        let result = ParsedTDS {
            csv: String::new(),
            columns: new_columns,
            rows: new_rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

/// `extend(Relation<T>[1], FuncColSpecArray<{T[1]->Any[*]},Z>[1])
///   :Relation<T+Z>[1]`.
///
/// Multi-column variant: applies one init lambda per requested
/// column, materialising each as a new column on the result relation.
///
/// **Reads the column list directly from the AST.** The native's
/// `args[1]` is the `ColSpecArrayLiteral` ValueSpec carrying the
/// per-column `RelationColumnLowered { name, init_lambda, … }`
/// triples. Evaluating it via `ctx.evaluate` would round-trip through
/// `eval.rs`'s ColSpecArrayLiteral arm, which currently materialises
/// the heap form via the plain `alloc_col_spec_array_literal` and
/// drops the per-column `function` slots (the FuncColSpecArray heap
/// allocator hasn't landed pure-side). Side-stepping the heap form
/// keeps this overload engine-only.
///
/// **Wrapped receivers** (`let cs = ~[...]; rel->extend($cs)`) fall
/// through the `_` arm and produce a "FuncColSpecArray AST not found"
/// error pointing at the same gap. Until the pure-side allocator
/// lands, only direct-literal `~[...]` arguments are supported.
#[derive(Debug)]
pub struct ExtendFuncColSpecArray;

impl NativeFunction for ExtendFuncColSpecArray {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("extend (Relation, FuncColSpecArray)", args, 2)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // -- Source TDS --------------------------------------------------
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("extend", &tds_obj, ctx)?;

        // -- Walk the AST for the ColSpecArrayLiteral's column triples ---
        let cols_ast = match args[1].kind.as_ref() {
            ExprKind::ColSpecArrayLiteral {
                columns,
                kind: ColSpecLiteralKind::Func,
            } => columns,
            other => {
                return Err(PureException::from(PureRuntimeError::EvaluationError(
                    format!(
                        "extend (Relation, FuncColSpecArray): arg 2 must be a direct \
                         ~[name:lam, …] literal; wrapped receivers (let-bound, function-\
                         returned) aren't supported until the pure-side \
                         FuncColSpecArray heap allocator lands. Got {other:?}"
                    ),
                )));
            }
        };

        // -- For each (column-name, init-lambda) materialise the lambda
        //    Value once, then evaluate per row.
        let mut col_handles: Vec<(SmolStr, Value)> = Vec::with_capacity(cols_ast.len());
        for col in cols_ast {
            let Some(init) = col.init_lambda.as_ref() else {
                return Err(PureException::from(PureRuntimeError::EvaluationError(
                    format!(
                        "extend (Relation, FuncColSpecArray): column '{}' has no init \
                         lambda; FuncColSpecArray entries must carry one",
                        col.name
                    ),
                )));
            };
            let lambda_val = ctx.evaluate(init)?.into_value();
            col_handles.push((col.name.clone(), lambda_val));
        }

        // -- Per-row evaluation, per new column --------------------------
        let n_rows = parsed.rows.len();
        let mut new_columns_cells: Vec<Vec<Option<TypedCell>>> =
            vec![Vec::with_capacity(n_rows); col_handles.len()];
        for row in &parsed.rows {
            let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
            for (col_idx, (_, lambda_val)) in col_handles.iter().enumerate() {
                let val = ctx.call_function(lambda_val, &[Value::Object(row_tuple.clone())])?;
                new_columns_cells[col_idx].push(value_to_typed_cell(&val));
            }
        }

        // -- New column metadata + extended rows -------------------------
        let mut new_columns = parsed.columns.clone();
        for ((name, _), cells) in col_handles.iter().zip(new_columns_cells.iter()) {
            let (inferred_type, inferred_mult) = infer_column_type_and_mult(cells);
            new_columns.push(ParsedColumn {
                name: name.clone(),
                type_tag: inferred_type,
                multiplicity: inferred_mult,
            });
        }
        let mut new_rows: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(n_rows);
        for (row_idx, row) in parsed.rows.iter().enumerate() {
            let mut extended = row.clone();
            for col_cells in &new_columns_cells {
                extended.push(col_cells[row_idx].clone());
            }
            new_rows.push(extended);
        }

        let result = ParsedTDS {
            csv: String::new(),
            columns: new_columns,
            rows: new_rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

/// Convert a runtime [`Value`] (an extend body's per-row result) into a
/// [`TypedCell`] for the new column. `Unit` collapses to `None` (empty
/// cell). `Object`, `Function`, `Element`, etc. are out of scope for the
/// first FuncColSpec native — extend bodies in PCT tests produce scalar
/// results.
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
