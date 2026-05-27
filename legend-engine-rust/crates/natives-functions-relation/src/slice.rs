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

//! Slice / row-navigation natives:
//!
//! ```pure
//! native offset<T>(w:Relation<T>[1], r:T[1], offset:Integer[1]):T[0..1];
//! native first<T>(w:Relation<T>[1], f:_Window<T>[1], r:T[1]):T[0..1];
//! native last<T>(w:Relation<T>[1], f:_Window<T>[1], r:T[1]):T[0..1];
//! native nth<T>(w:Relation<T>[1], f:_Window<T>[1], r:T[1], offset:Integer[1]):T[0..1];
//! native slice<T>(rel:Relation<T>[1], start:Integer[1], stop:Integer[1]):Relation<T>[1];
//! ```
//!
//! `lag` / `lead` are Pure-defined and delegate to `offset`
//! (`lag(w,r,o) = offset(w,r,-o)`, `lead(w,r,o) = offset(w,r,o)`), so
//! only `offset` needs an engine native.
//!
//! `offset` / `first` / `last` / `nth` are invoked from inside an
//! `extend(Relation, _Window, FuncColSpec)` map lambda, which passes
//! the sorted partition sub-TDS as `w`/`rel` and a row tuple carrying
//! its within-partition position (`__row_index`). Each returns the row
//! tuple at the computed partition index, or `Unit` when out of range
//! (`T[0..1]` empty). Java reference: `RelationNativeImplementation`
//! `offset` / `first` / `last` / `nth`.
//!
//! `slice(rel, start, stop)` is **not** windowed — it returns the
//! `[start, stop)` row range of the whole relation as a fresh TDS.

#![allow(clippy::needless_pass_by_value)]

use legend_pure_dsl_tds::csv::ParsedTDS;
use legend_pure_parser_pure::types::ValueSpec;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
};
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use crate::window_runtime::{
    effective_frame, frame_high, frame_low, read_frame, read_row_index, read_sort_keys,
};

/// Decode `(partition_tds, position)` from `rel` arg + `row` arg — the
/// partition sub-TDS and the row's within-partition index.
#[allow(clippy::result_large_err)]
fn decode_partition_and_position(
    rel_arg: &ValueSpec,
    row_arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    fn_label: &'static str,
) -> Result<(ParsedTDS, usize), PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let rel_value = ctx.evaluate(rel_arg)?.into_value();
    let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
    let parsed = read_parsed_tds(fn_label, &tds_obj, ctx)?;

    let row_value = ctx.evaluate(row_arg)?.into_value();
    let row_obj = unwrap_instance_value(&row_value, instance_value_id, ctx)?;
    let position = read_row_index(&row_obj, ctx).ok_or_else(|| {
        PureException::from(PureRuntimeError::EvaluationError(format!(
            "{fn_label}: row tuple is missing its window position ({}); slice-family \
             natives must be called from inside an extend(_, _Window, FuncColSpec) \
             map lambda",
            crate::window_runtime::ROW_INDEX_SLOT
        )))
    })?;
    Ok((parsed, position))
}

/// Build the row tuple for `parsed.rows[idx]`, or `Unit` when out of
/// range — the `T[0..1]` empty case.
#[allow(clippy::result_large_err)]
fn row_at(
    parsed: &ParsedTDS,
    idx: i64,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Value, PureException> {
    if idx < 0 || idx as usize >= parsed.rows.len() {
        return Ok(Value::Unit);
    }
    let row_tuple = build_row_tuple(&parsed.columns, &parsed.rows[idx as usize], ctx)?;
    Ok(Value::Object(row_tuple))
}

/// Read the window's effective frame (with the SQL ORDER-BY default
/// applied) from `args[1]`.
#[allow(clippy::result_large_err)]
fn read_effective_frame(
    window_arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Option<crate::window_runtime::Frame>, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let window_value = ctx.evaluate(window_arg)?.into_value();
    let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
    let sort_keys = read_sort_keys(&window_obj, ctx)?;
    let frame = read_frame(&window_obj, ctx)?;
    Ok(effective_frame(frame, !sort_keys.is_empty()))
}

// ---------------------------------------------------------------------------
// offset (powers lag / lead)
// ---------------------------------------------------------------------------

/// `offset<T>(w:Relation<T>[1], r:T[1], offset:Integer[1]):T[0..1]`.
/// Returns the row at `position + offset` within the partition, or
/// `Unit` when out of range. Frame-independent (operates on physical
/// row position).
#[derive(Debug)]
pub struct Offset;

impl NativeFunction for Offset {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("offset (Relation, row, offset)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[1], ctx, "offset")?;
        let offset = match ctx.evaluate(&args[2])?.into_value() {
            Value::Integer(n) => n,
            other => {
                return Err(PureException::from(PureRuntimeError::type_mismatch(
                    "Integer", &other,
                )));
            }
        };
        let target = position as i64 + offset;
        Ok(Evaluated::new(row_at(&parsed, target, ctx)?))
    }
}

// ---------------------------------------------------------------------------
// first / last / nth (frame-relative)
// ---------------------------------------------------------------------------

/// `first<T>(w:Relation<T>[1], f:_Window<T>[1], r:T[1]):T[0..1]` —
/// the row at the frame's low boundary.
#[derive(Debug)]
pub struct First;

impl NativeFunction for First {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("first (Relation, _Window, row)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "first")?;
        let frame = read_effective_frame(&args[1], ctx)?;
        let low = frame_low(frame.as_ref(), position, parsed.rows.len());
        Ok(Evaluated::new(row_at(&parsed, low as i64, ctx)?))
    }
}

/// `last<T>(w:Relation<T>[1], f:_Window<T>[1], r:T[1]):T[0..1]` —
/// the row at the frame's high boundary.
#[derive(Debug)]
pub struct Last;

impl NativeFunction for Last {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("last (Relation, _Window, row)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "last")?;
        let frame = read_effective_frame(&args[1], ctx)?;
        let high = frame_high(frame.as_ref(), position, parsed.rows.len());
        Ok(Evaluated::new(row_at(&parsed, high as i64, ctx)?))
    }
}

/// `nth<T>(w:Relation<T>[1], f:_Window<T>[1], r:T[1], offset:Integer[1]):T[0..1]`
/// — the `offset`-th (1-based) row of the frame, counting from its low
/// boundary; `Unit` when that index is past the frame's high boundary.
/// Mirrors Java `TestTDS.nth`: `low + offset - 1`, valid iff `<= high`.
#[derive(Debug)]
pub struct Nth;

impl NativeFunction for Nth {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("nth (Relation, _Window, row, offset)", args, 4)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "nth")?;
        let frame = read_effective_frame(&args[1], ctx)?;
        let nth = match ctx.evaluate(&args[3])?.into_value() {
            Value::Integer(n) => n,
            other => {
                return Err(PureException::from(PureRuntimeError::type_mismatch(
                    "Integer", &other,
                )));
            }
        };
        let n = parsed.rows.len();
        let low = frame_low(frame.as_ref(), position, n) as i64;
        let high = frame_high(frame.as_ref(), position, n) as i64;
        let target = low + nth - 1;
        if target > high {
            return Ok(Evaluated::new(Value::Unit));
        }
        Ok(Evaluated::new(row_at(&parsed, target, ctx)?))
    }
}

// ---------------------------------------------------------------------------
// slice (whole-relation row range)
// ---------------------------------------------------------------------------

/// `slice<T>(rel:Relation<T>[1], start:Integer[1], stop:Integer[1])
///   :Relation<T>[1]` — the `[start, stop)` row range (stop exclusive),
/// clamped to the relation bounds. Java `TestTDS.slice`.
#[derive(Debug)]
pub struct Slice;

impl NativeFunction for Slice {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("slice (Relation, start, stop)", args, 3)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("slice", &tds_obj, ctx)?;

        let start = eval_int(&args[1], ctx, "slice.start")?;
        let stop = eval_int(&args[2], ctx, "slice.stop")?;

        let n = parsed.rows.len() as i64;
        let lo = start.clamp(0, n) as usize;
        let hi = stop.clamp(start.max(0), n) as usize;
        let rows = parsed.rows[lo..hi].to_vec();

        let result = ParsedTDS {
            csv: String::new(),
            columns: parsed.columns.clone(),
            rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

#[allow(clippy::result_large_err)]
fn eval_int(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    label: &'static str,
) -> Result<i64, PureException> {
    match ctx.evaluate(arg)?.into_value() {
        Value::Integer(n) => Ok(n),
        other => Err(PureException::from(PureRuntimeError::EvaluationError(
            format!("{label}: expected Integer, got {other:?}"),
        ))),
    }
}
