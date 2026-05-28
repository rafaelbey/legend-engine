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

//! `join` native:
//!
//! ```pure
//! native join<T,V>(rel1:Relation<T>[1], rel2:Relation<V>[1],
//!     joinKind:JoinKind[1], f:Function<{T[1],V[1]->Boolean[1]}>[1]
//! ):Relation<T+V>[1];
//! ```
//!
//! Nested-loop join: evaluate the predicate for every `(left, right)`
//! row pair; emit the surviving pairs as `rel1`'s columns followed by
//! `rel2`'s columns. The `JoinKind` enum (`INNER`/`LEFT`/`RIGHT`/`FULL`)
//! controls outer behaviour — unmatched rows on the retained side are
//! emitted with `null` cells for the other side. Result row order is
//! unspecified (every PCT test sorts the output).

#![allow(clippy::needless_pass_by_value)]

use legend_pure_dsl_tds::csv::{ParsedColumn, ParsedTDS, TypedCell};
use legend_pure_parser_pure::types::{Multiplicity, ValueSpec};

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::heap::ObjectHandle;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
};
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
}

/// `join(Relation, Relation, JoinKind, Function)`.
#[derive(Debug)]
pub struct Join;

impl NativeFunction for Join {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "join (Relation, Relation, JoinKind, Function)";
        expect_args(LABEL, args, 4)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        let rel1_value = ctx.evaluate(&args[0])?.into_value();
        let tds1 = unwrap_instance_value(&rel1_value, instance_value_id, ctx)?;
        let left = read_parsed_tds("join", &tds1, ctx)?;

        let rel2_value = ctx.evaluate(&args[1])?.into_value();
        let tds2 = unwrap_instance_value(&rel2_value, instance_value_id, ctx)?;
        let right = read_parsed_tds("join", &tds2, ctx)?;

        let kind = read_join_kind(&args[2], ctx, LABEL)?;
        let predicate = ctx.evaluate(&args[3])?.into_value();

        // Pre-build one row tuple per source row (reused across pairs).
        let left_tuples = build_tuples(&left, ctx)?;
        let right_tuples = build_tuples(&right, ctx)?;

        let n_left_cols = left.columns.len();
        let n_right_cols = right.columns.len();

        let mut rows: Vec<Vec<Option<TypedCell>>> = Vec::new();
        let mut right_matched = vec![false; right.rows.len()];

        // Left-driven pass: matched pairs (all kinds) + unmatched-left
        // (LEFT / FULL).
        for (i, l_tuple) in left_tuples.iter().enumerate() {
            let mut any = false;
            for (j, r_tuple) in right_tuples.iter().enumerate() {
                if eval_predicate(&predicate, l_tuple, r_tuple, ctx)? {
                    rows.push(merge_row(
                        Some(&left.rows[i]),
                        Some(&right.rows[j]),
                        n_left_cols,
                        n_right_cols,
                    ));
                    right_matched[j] = true;
                    any = true;
                }
            }
            if !any && matches!(kind, JoinKind::Left | JoinKind::Full) {
                rows.push(merge_row(Some(&left.rows[i]), None, n_left_cols, n_right_cols));
            }
        }

        // Right-unmatched pass (RIGHT / FULL).
        if matches!(kind, JoinKind::Right | JoinKind::Full) {
            for (j, matched) in right_matched.iter().enumerate() {
                if !matched {
                    rows.push(merge_row(None, Some(&right.rows[j]), n_left_cols, n_right_cols));
                }
            }
        }

        // Result schema: left columns ++ right columns, types carried
        // from source; multiplicity widened to [0..1] where the join
        // can introduce nulls in that column.
        let mut columns: Vec<ParsedColumn> =
            Vec::with_capacity(n_left_cols + n_right_cols);
        columns.extend(left.columns.iter().cloned());
        columns.extend(right.columns.iter().cloned());
        widen_nullable_columns(&mut columns, &rows);

        let result = ParsedTDS {
            csv: String::new(),
            columns,
            rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

#[allow(clippy::result_large_err)]
fn build_tuples(
    parsed: &ParsedTDS,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Vec<ObjectHandle>, PureException> {
    parsed
        .rows
        .iter()
        .map(|row| build_row_tuple(&parsed.columns, row, ctx))
        .collect()
}

#[allow(clippy::result_large_err)]
fn eval_predicate(
    predicate: &Value,
    left: &ObjectHandle,
    right: &ObjectHandle,
    ctx: &mut dyn EvalContextTrait,
) -> Result<bool, PureException> {
    let v = ctx.call_function(
        predicate,
        &[Value::Object(left.clone()), Value::Object(right.clone())],
    )?;
    Ok(matches!(v, Value::Boolean(true)))
}

/// Build a merged result row: `left` cells (or `n_left` nulls) followed
/// by `right` cells (or `n_right` nulls).
fn merge_row(
    left: Option<&[Option<TypedCell>]>,
    right: Option<&[Option<TypedCell>]>,
    n_left: usize,
    n_right: usize,
) -> Vec<Option<TypedCell>> {
    let mut out = Vec::with_capacity(n_left + n_right);
    match left {
        Some(cells) => out.extend(cells.iter().cloned()),
        None => out.extend(std::iter::repeat_n(None, n_left)),
    }
    match right {
        Some(cells) => out.extend(cells.iter().cloned()),
        None => out.extend(std::iter::repeat_n(None, n_right)),
    }
    out
}

/// Widen a column to `[0..1]` when any result cell in it is empty —
/// outer joins introduce nulls the source schema didn't carry. Column
/// types stay as the source declared them (preserved even for an
/// all-null column from a fully-unmatched side).
fn widen_nullable_columns(columns: &mut [ParsedColumn], rows: &[Vec<Option<TypedCell>>]) {
    for (col_idx, col) in columns.iter_mut().enumerate() {
        let has_null = rows.iter().any(|row| row[col_idx].is_none());
        if has_null && col.multiplicity == Multiplicity::PureOne {
            col.multiplicity = Multiplicity::ZeroOrOne;
        }
    }
}

#[allow(clippy::result_large_err)]
fn read_join_kind(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<JoinKind, PureException> {
    let value = ctx.evaluate(arg)?.into_value();
    let member = match &value {
        Value::EnumValue { member, .. } => Some(member.clone()),
        Value::Object(obj) => ctx
            .heap()
            .get_property_values(obj, "name")
            .ok()
            .and_then(|vals| {
                vals.iter().find_map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
            }),
        Value::String(s) => Some(s.clone()),
        _ => None,
    };
    match member.as_deref() {
        Some("INNER") => Ok(JoinKind::Inner),
        Some("LEFT") => Ok(JoinKind::Left),
        Some("RIGHT") => Ok(JoinKind::Right),
        Some("FULL") => Ok(JoinKind::Full),
        other => Err(PureException::from(PureRuntimeError::EvaluationError(
            format!("{label}: unrecognised JoinKind {other:?} (expected INNER/LEFT/RIGHT/FULL)"),
        ))),
    }
}
