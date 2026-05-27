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

//! Shared OLAP window machinery for natives that consume `_Window<T>`:
//! `extend(_, _Window, FuncColSpec)`, `extend(_, _Window, AggColSpec)`,
//! the standalone `reduce(rel, w, row, map, agg)`, and the ranking
//! family (`rowNumber`, `rank`, `lag`, `lead`, …).
//!
//! Heap shapes consumed here:
//!
//! - `_Window<T>` (`platform/pure/grammar/.../window/window.pure`):
//!   `partition: String[*]`, `sortInfo: SortInfo<Any>[*]`,
//!   `frame: Frame[0..1]`.
//! - `Frame` is one of `Rows`, `_Range`, `_RangeInterval` — each carries
//!   `offsetFrom : FrameValue[1]` and `offsetTo : FrameValue[1]`.
//! - `FrameValue` subclasses: `FrameIntValue { value: Integer }`,
//!   `FrameNumericValue { value: Number }`, `FrameIntervalValue`,
//!   `UnboundedFrameValue {}`.
//! - `SortInfo<T>` (`window/sort.pure`):
//!   `column: ColSpec<T>[1]`, `direction: SortType[1]` — direction is
//!   the `SortType` enum with values `asc` / `desc` (or the platform's
//!   equivalent identifiers).
//!
//! The runtime stores the source TDS only as its canonical CSV string;
//! callers re-parse via `read_parsed_tds` upstream, so this module
//! works exclusively in `ParsedTDS` row-index space.

#![allow(clippy::needless_pass_by_value)]

use std::cmp::Ordering;

use legend_pure_dsl_tds::csv::{ParsedTDS, TypedCell};
use smol_str::SmolStr;

use im_rc::Vector as PVector;
use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::heap::ObjectHandle;
use legend_pure_runtime::native::EvalContextTrait;
use legend_pure_runtime::value::Value;

// ---------------------------------------------------------------------------
// Window slot readers
// ---------------------------------------------------------------------------

/// Hidden slot on a row-tuple object carrying the row's 0-based
/// position within its (sorted) window partition. Attached by
/// `extend(Relation, _Window, FuncColSpec)` so the ranking natives
/// (`rowNumber`, `rank`, …) and the standalone `reduce` can locate the
/// row without a content match. Mirrors Java's `RowContainer.getRow()`.
///
/// `__`-prefixed to match the runtime's existing reserved-slot
/// convention (`__typeArguments`, `__typeVariableValues`); not a legal
/// user TDS column name.
pub(crate) const ROW_INDEX_SLOT: &str = "__row_index";

/// Attach [`ROW_INDEX_SLOT`] to a freshly-built row tuple.
#[allow(clippy::result_large_err)]
pub(crate) fn attach_row_index(
    row_tuple: &ObjectHandle,
    index: usize,
    ctx: &mut dyn EvalContextTrait,
) -> Result<(), PureException> {
    ctx.heap_mut()
        .mutate_add(row_tuple, ROW_INDEX_SLOT, &[Value::Integer(index as i64)])
        .map_err(PureException::from)
}

/// Read [`ROW_INDEX_SLOT`] off a row tuple, if present.
pub(crate) fn read_row_index(
    row_tuple: &ObjectHandle,
    ctx: &mut dyn EvalContextTrait,
) -> Option<usize> {
    ctx.heap()
        .get_property_values(row_tuple, ROW_INDEX_SLOT)
        .ok()?
        .iter()
        .find_map(|v| match v {
            Value::Integer(i) if *i >= 0 => Some(*i as usize),
            _ => None,
        })
}

/// Read the window's `partition: String[*]` slot as a column-name list.
#[allow(clippy::result_large_err)]
pub(crate) fn read_partition_cols(
    window_obj: &ObjectHandle,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Vec<SmolStr>, PureException> {
    let values = ctx
        .heap()
        .get_property_values(window_obj, "partition")
        .map_err(PureException::from)?;
    Ok(values
        .iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect())
}

/// Sort key from a single `SortInfo` heap object: the column name being
/// sorted on + direction. `direction` reads as a heap object whose
/// classifier ends with `SortType`; we extract `asc` / `desc` via the
/// `name` slot the enum-instance carries.
#[derive(Debug, Clone)]
pub(crate) struct SortKey {
    pub column: SmolStr,
    pub direction: SortDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortDir {
    Asc,
    Desc,
}

/// Read the window's `sortInfo: SortInfo<Any>[*]` slot, projecting each
/// entry to `(column_name, direction)`.
#[allow(clippy::result_large_err)]
pub(crate) fn read_sort_keys(
    window_obj: &ObjectHandle,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Vec<SortKey>, PureException> {
    let sort_values = ctx
        .heap()
        .get_property_values(window_obj, "sortInfo")
        .map_err(PureException::from)?;
    let mut out = Vec::with_capacity(sort_values.len());
    for v in &sort_values {
        let Value::Object(sort_obj) = v else { continue };
        // `column: ColSpec<T>` -> read its `name`.
        let cs_values = ctx
            .heap()
            .get_property_values(sort_obj, "column")
            .map_err(PureException::from)?;
        let Some(Value::Object(cs_obj)) = cs_values.iter().find(|v| matches!(v, Value::Object(_)))
        else {
            continue;
        };
        let name_values = ctx
            .heap()
            .get_property_values(cs_obj, "name")
            .map_err(PureException::from)?;
        let column = match name_values.iter().find_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }) {
            Some(n) => n,
            None => continue,
        };

        // `direction: SortType` -> enum-instance with a `name` slot.
        // SortType values are `ASC` / `DESC` (or `asc`/`desc`).
        let dir_values = ctx
            .heap()
            .get_property_values(sort_obj, "direction")
            .map_err(PureException::from)?;
        let direction = parse_sort_dir(&dir_values, ctx).unwrap_or(SortDir::Asc);
        out.push(SortKey { column, direction });
    }
    Ok(out)
}

fn parse_sort_dir(values: &PVector<Value>, ctx: &mut dyn EvalContextTrait) -> Option<SortDir> {
    for v in values {
        match v {
            // `~o->ascending()` lowers to a `SortType` enum value;
            // the runtime represents it as `Value::EnumValue { member }`.
            // sort.rs reads this same shape via `member` (see
            // legend-engine-rust-natives-functions-relation/src/sort.rs:166).
            Value::EnumValue { member, .. } => {
                if matches_dir(member.as_str(), "DESC") {
                    return Some(SortDir::Desc);
                } else if matches_dir(member.as_str(), "ASC") {
                    return Some(SortDir::Asc);
                }
            }
            Value::String(s) => {
                if matches_dir(s.as_str(), "DESC") {
                    return Some(SortDir::Desc);
                } else if matches_dir(s.as_str(), "ASC") {
                    return Some(SortDir::Asc);
                }
            }
            Value::Object(obj) => {
                // Defensive: some runtimes wrap enum instances as heap
                // objects with a `name` slot. Read it.
                if let Ok(name_vals) = ctx.heap().get_property_values(obj, "name") {
                    for nv in &name_vals {
                        if let Value::String(s) = nv {
                            if matches_dir(s.as_str(), "DESC") {
                                return Some(SortDir::Desc);
                            } else if matches_dir(s.as_str(), "ASC") {
                                return Some(SortDir::Asc);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn matches_dir(s: &str, target: &str) -> bool {
    s.eq_ignore_ascii_case(target)
}

// ---------------------------------------------------------------------------
// Frame
// ---------------------------------------------------------------------------

/// Window frame, lifted from the heap. Captures `offsetFrom` /
/// `offsetTo` along with the `Frame` subclass (`Rows` vs `_Range` vs
/// `_RangeInterval`) so the row-selection routine knows whether
/// offsets are row-positions or value-ranges.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    pub kind: FrameKind,
    pub from: FrameOffset,
    pub to: FrameOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// `Rows(N, M)` — physical row offsets relative to current row.
    Rows,
    /// `_Range(N, M)` — logical range over the sort-key value (numeric).
    Range,
    /// `_RangeInterval(N, M)` — range with a duration unit.
    RangeInterval,
}

#[derive(Debug, Clone)]
pub(crate) enum FrameOffset {
    Unbounded,
    /// Integer offset (Rows) — negative = preceding, 0 = current,
    /// positive = following.
    Int(i64),
    /// Numeric offset (Range) — compared against the sort column's
    /// value. Captured from the heap but not yet consumed:
    /// `frame_row_indices` falls back to the full partition for Range
    /// frames (the `testRange_*` PCT variants are a follow-up). The
    /// payload is retained so that work doesn't re-plumb the reader.
    #[allow(dead_code)]
    Numeric(f64),
}

/// Read the window's `frame: Frame[0..1]` slot. Returns `None` for
/// partition-wide windows (the slot is empty).
#[allow(clippy::result_large_err)]
pub(crate) fn read_frame(
    window_obj: &ObjectHandle,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Option<Frame>, PureException> {
    let values = ctx
        .heap()
        .get_property_values(window_obj, "frame")
        .map_err(PureException::from)?;
    let Some(Value::Object(frame_obj)) = values.iter().find(|v| matches!(v, Value::Object(_)))
    else {
        return Ok(None);
    };
    let classifier = ctx
        .heap()
        .classifier(frame_obj)
        .map_err(PureException::from)?
        .clone();
    let kind = if classifier.ends_with("::Rows") {
        FrameKind::Rows
    } else if classifier.ends_with("::_Range") {
        FrameKind::Range
    } else if classifier.ends_with("::_RangeInterval") {
        FrameKind::RangeInterval
    } else {
        return Err(PureException::from(PureRuntimeError::EvaluationError(
            format!("window_runtime: unknown Frame subclass classifier '{classifier}'"),
        )));
    };
    let from = read_frame_offset(frame_obj, "offsetFrom", ctx)?;
    let to = read_frame_offset(frame_obj, "offsetTo", ctx)?;
    Ok(Some(Frame { kind, from, to }))
}

/// Resolve the *effective* window frame for a partition aggregation.
///
/// When `over(...)` carries no explicit frame the slot is empty
/// (`frame == None`), and SQL window semantics supply a default:
///
/// - **ORDER BY present** → `RANGE UNBOUNDED PRECEDING TO CURRENT ROW`
///   (a running / cumulative aggregate). We approximate this with the
///   physical-row equivalent `ROWS UNBOUNDED PRECEDING TO CURRENT ROW`;
///   the two coincide whenever the sort key is unique within the
///   partition (true for every in-scope PCT test). A genuine RANGE
///   default would also fold in tie "peers" of the current row — a
///   follow-up alongside `_range` frame support.
/// - **no ORDER BY** → the whole partition (`None`, the caller's
///   full-partition path).
pub(crate) fn effective_frame(frame: Option<Frame>, has_sort: bool) -> Option<Frame> {
    match frame {
        Some(f) => Some(f),
        None if has_sort => Some(Frame {
            kind: FrameKind::Rows,
            from: FrameOffset::Unbounded,
            to: FrameOffset::Int(0), // current row
        }),
        None => None,
    }
}

#[allow(clippy::result_large_err)]
fn read_frame_offset(
    frame_obj: &ObjectHandle,
    slot: &str,
    ctx: &mut dyn EvalContextTrait,
) -> Result<FrameOffset, PureException> {
    let values = ctx
        .heap()
        .get_property_values(frame_obj, slot)
        .map_err(PureException::from)?;
    let Some(Value::Object(off_obj)) = values.iter().find(|v| matches!(v, Value::Object(_)))
    else {
        return Err(PureException::from(PureRuntimeError::EvaluationError(
            format!("window_runtime: Frame.{slot} slot missing or not an Object"),
        )));
    };
    let classifier = ctx
        .heap()
        .classifier(off_obj)
        .map_err(PureException::from)?
        .clone();
    if classifier.ends_with("::UnboundedFrameValue") {
        return Ok(FrameOffset::Unbounded);
    }
    // FrameIntValue / FrameNumericValue / FrameIntervalValue all carry
    // a `value` slot. Read it.
    let value_values = ctx
        .heap()
        .get_property_values(off_obj, "value")
        .map_err(PureException::from)?;
    for v in &value_values {
        match v {
            Value::Integer(i) => return Ok(FrameOffset::Int(*i)),
            Value::Float(f) => return Ok(FrameOffset::Numeric(*f)),
            _ => {}
        }
    }
    Err(PureException::from(PureRuntimeError::EvaluationError(
        format!("window_runtime: Frame.{slot}.value slot missing or not Integer/Float"),
    )))
}

// ---------------------------------------------------------------------------
// Partition + sort + frame composition
// ---------------------------------------------------------------------------

/// Resolve partition column names to their indices in `parsed.columns`.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve_partition_indices(
    partition_cols: &[SmolStr],
    parsed: &ParsedTDS,
    fn_label: &'static str,
) -> Result<Vec<usize>, PureException> {
    partition_cols
        .iter()
        .map(|name| {
            parsed
                .columns
                .iter()
                .position(|c| c.name.as_str() == name.as_str())
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        parsed.columns.iter().map(|c| c.name.as_str()).collect();
                    PureException::from(PureRuntimeError::EvaluationError(format!(
                        "{fn_label}: partition column '{name}' not present in receiver; have {available:?}"
                    )))
                })
        })
        .collect()
}

/// Resolve sort-key column names to their indices in `parsed.columns`.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve_sort_indices(
    sort_keys: &[SortKey],
    parsed: &ParsedTDS,
    fn_label: &'static str,
) -> Result<Vec<(usize, SortDir)>, PureException> {
    sort_keys
        .iter()
        .map(|key| {
            parsed
                .columns
                .iter()
                .position(|c| c.name.as_str() == key.column.as_str())
                .map(|i| (i, key.direction))
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        parsed.columns.iter().map(|c| c.name.as_str()).collect();
                    PureException::from(PureRuntimeError::EvaluationError(format!(
                        "{fn_label}: sort column '{}' not present in receiver; have {available:?}",
                        key.column
                    )))
                })
        })
        .collect()
}

/// Compare two parsed-TDS rows by `(idx, dir)` sort keys, ascending by
/// `Ordering` semantics (caller flips for desc). `None` cells sort
/// AFTER `Some(_)` (matches Java Pure / SQL `ASC NULLS LAST` —
/// see `sort.rs::compare_cells`).
pub(crate) fn compare_rows_by_keys(
    lhs: &[Option<TypedCell>],
    rhs: &[Option<TypedCell>],
    keys: &[(usize, SortDir)],
) -> Ordering {
    for (idx, dir) in keys {
        let l = lhs[*idx].as_ref();
        let r = rhs[*idx].as_ref();
        let raw = compare_cells(l, r);
        let oriented = match dir {
            SortDir::Asc => raw,
            SortDir::Desc => raw.reverse(),
        };
        if oriented != Ordering::Equal {
            return oriented;
        }
    }
    Ordering::Equal
}

fn compare_cells(a: Option<&TypedCell>, b: Option<&TypedCell>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater, // NULLS LAST in ASC
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => match (a, b) {
            (TypedCell::Integer(x), TypedCell::Integer(y)) => x.cmp(y),
            (TypedCell::Float(x), TypedCell::Float(y)) => {
                x.partial_cmp(y).unwrap_or(Ordering::Equal)
            }
            (TypedCell::Integer(x), TypedCell::Float(y)) => {
                (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal)
            }
            (TypedCell::Float(x), TypedCell::Integer(y)) => {
                x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal)
            }
            (TypedCell::Boolean(x), TypedCell::Boolean(y)) => x.cmp(y),
            (TypedCell::Decimal(x), TypedCell::Decimal(y))
            | (TypedCell::String(x), TypedCell::String(y))
            | (TypedCell::StrictDate(x), TypedCell::StrictDate(y))
            | (TypedCell::DateTime(x), TypedCell::DateTime(y)) => x.as_str().cmp(y.as_str()),
            _ => type_tag(a).cmp(&type_tag(b)),
        },
    }
}

fn type_tag(c: &TypedCell) -> u8 {
    match c {
        TypedCell::Integer(_) => 0,
        TypedCell::Float(_) => 1,
        TypedCell::Decimal(_) => 2,
        TypedCell::Boolean(_) => 3,
        TypedCell::String(_) => 4,
        TypedCell::StrictDate(_) => 5,
        TypedCell::DateTime(_) => 6,
    }
}

/// Group row indices by partition-key tuple, preserving the
/// first-occurrence order of partitions.
pub(crate) fn partition_row_indices(
    parsed: &ParsedTDS,
    partition_indices: &[usize],
) -> Vec<(Vec<Option<TypedCell>>, Vec<usize>)> {
    let mut out: Vec<(Vec<Option<TypedCell>>, Vec<usize>)> = Vec::new();
    for (row_idx, row) in parsed.rows.iter().enumerate() {
        let key: Vec<Option<TypedCell>> =
            partition_indices.iter().map(|&i| row[i].clone()).collect();
        if let Some(slot) = out.iter_mut().find(|(k, _)| k == &key) {
            slot.1.push(row_idx);
        } else {
            out.push((key, vec![row_idx]));
        }
    }
    out
}

/// For each partition, sort the row-index list by the sort keys (mutates
/// in place). Stable sort: ties preserve source order.
pub(crate) fn sort_partitions_in_place(
    partitions: &mut [(Vec<Option<TypedCell>>, Vec<usize>)],
    parsed: &ParsedTDS,
    sort_keys: &[(usize, SortDir)],
) {
    if sort_keys.is_empty() {
        return;
    }
    for (_, indices) in partitions.iter_mut() {
        indices.sort_by(|&a, &b| {
            compare_rows_by_keys(&parsed.rows[a], &parsed.rows[b], sort_keys)
        });
    }
}

/// Return the row indices that make up the window frame for a given
/// `position_in_partition` inside `partition_row_indices`. The returned
/// vec is the in-frame subset (already in sort order).
///
/// `Rows`-only (and `None` = full partition). Range frames need the
/// sort-column values and go through [`frame_indices_for_row`].
pub(crate) fn frame_row_indices(
    partition_row_indices: &[usize],
    position_in_partition: usize,
    frame: Option<&Frame>,
) -> Vec<usize> {
    let n = partition_row_indices.len();
    let Some(frame) = frame else {
        return partition_row_indices.to_vec();
    };
    if !matches!(frame.kind, FrameKind::Rows) {
        // Range / RangeInterval need values — caller should route
        // through `frame_indices_for_row`. Defensive full-partition
        // fallback so a mis-wired caller mismatches rather than panics.
        return partition_row_indices.to_vec();
    }
    let lo = match resolve_offset(&frame.from, position_in_partition, n, FrameSide::From) {
        OffsetResult::At(p) => p,
        OffsetResult::Empty => return Vec::new(),
    };
    let hi = match resolve_offset(&frame.to, position_in_partition, n, FrameSide::To) {
        OffsetResult::At(p) => p,
        OffsetResult::Empty => return Vec::new(),
    };
    if lo > hi {
        return Vec::new();
    }
    partition_row_indices[lo..=hi].to_vec()
}

/// The in-frame source-row indices (in sort order) for the row at
/// `position` within a sorted partition, dispatching on frame kind:
///
/// - `None` -> full partition; `Rows` -> [`frame_row_indices`].
/// - `Range` (numeric) -> value-based membership over the single sort
///   column, replicating Java `RelationNativeImplementation`'s
///   per-row range predicate (incl. the NULL rules: a NULL current
///   value frames only NULL peers; NULL peers join unbounded
///   boundaries on the NULLS-FIRST/LAST side). Requires exactly one
///   sort column.
/// - `RangeInterval` -> not yet implemented; full-partition fallback.
pub(crate) fn frame_indices_for_row(
    parsed: &ParsedTDS,
    sorted: &[usize],
    position: usize,
    frame: Option<&Frame>,
    sort_indices: &[(usize, SortDir)],
) -> Vec<usize> {
    match frame {
        None => sorted.to_vec(),
        Some(f) if matches!(f.kind, FrameKind::Rows) => {
            frame_row_indices(sorted, position, frame)
        }
        Some(f) if matches!(f.kind, FrameKind::Range) => {
            range_frame_indices(parsed, sorted, position, f, sort_indices)
        }
        // RangeInterval: follow-up (date + duration arithmetic).
        Some(_) => sorted.to_vec(),
    }
}

/// Numeric `Range` frame membership. For the current row at `position`
/// in the sorted partition, return the source indices of all rows whose
/// single sort-column value falls in the value range
/// `[current ± offsetFrom, current ± offsetTo]` (sign per sort
/// direction), preserving sort order. Mirrors the Java numeric branch
/// of `performMapReduce`.
fn range_frame_indices(
    parsed: &ParsedTDS,
    sorted: &[usize],
    position: usize,
    frame: &Frame,
    sort_indices: &[(usize, SortDir)],
) -> Vec<usize> {
    // Range requires exactly one sort column; if absent, fall back.
    let Some(&(col, dir)) = sort_indices.first() else {
        return sorted.to_vec();
    };
    let value_at = |sorted_pos: usize| -> Option<f64> {
        cell_as_f64(parsed.rows[sorted[sorted_pos]][col].as_ref())
    };
    let current = value_at(position);
    let from = frame_offset_as_f64(&frame.from); // None = unbounded
    let to = frame_offset_as_f64(&frame.to);

    let mut out: Vec<usize> = Vec::new();
    for k in 0..sorted.len() {
        let peer = value_at(k);
        if in_numeric_range(current, peer, from, to, dir) {
            out.push(sorted[k]);
        }
    }
    out
}

/// The numeric `Range` inclusion predicate for a single peer value,
/// given the current row's value, the frame offsets (`None` =
/// unbounded) and the sort direction. Faithful to Java.
fn in_numeric_range(
    current: Option<f64>,
    peer: Option<f64>,
    from: Option<f64>,
    to: Option<f64>,
    dir: SortDir,
) -> bool {
    // NULL current -> only NULL peers are in frame.
    let Some(cur) = current else {
        return peer.is_none();
    };
    match (from, to) {
        // UNBOUNDED .. UNBOUNDED -> everything.
        (None, None) => true,
        // UNBOUNDED PRECEDING .. N
        (None, Some(off)) => match dir {
            SortDir::Asc => peer.is_some_and(|v| v <= cur + off),
            // DESC: NULLS FIRST join the unbounded-preceding side.
            SortDir::Desc => match peer {
                None => true,
                Some(v) => cur - off <= v,
            },
        },
        // N .. UNBOUNDED FOLLOWING
        (Some(off), None) => match dir {
            SortDir::Asc => match peer {
                // ASC: NULLS LAST join the unbounded-following side.
                None => true,
                Some(v) => cur + off <= v,
            },
            SortDir::Desc => peer.is_some_and(|v| v <= cur - off),
        },
        // N .. M
        (Some(f), Some(t)) => {
            let (lo, hi) = match dir {
                SortDir::Asc => (cur + f, cur + t),
                SortDir::Desc => (cur - t, cur - f),
            };
            peer.is_some_and(|v| lo <= v && v <= hi)
        }
    }
}

fn cell_as_f64(cell: Option<&TypedCell>) -> Option<f64> {
    match cell {
        Some(TypedCell::Integer(i)) => Some(*i as f64),
        Some(TypedCell::Float(f)) => Some(*f),
        _ => None,
    }
}

fn frame_offset_as_f64(offset: &FrameOffset) -> Option<f64> {
    match offset {
        FrameOffset::Unbounded => None,
        FrameOffset::Int(i) => Some(*i as f64),
        FrameOffset::Numeric(f) => Some(*f),
    }
}

/// Low boundary index of `frame` for a row at `position` in a partition
/// of size `n`. Mirrors Java `Rows.getLow`: `fromUnbounded ? 0 :
/// max(0, position + offsetFrom)`. Used by `first` / `nth`. `None`
/// frame (no ORDER BY) -> 0 (whole partition starts at row 0).
pub(crate) fn frame_low(frame: Option<&Frame>, position: usize, _n: usize) -> usize {
    match frame {
        None => 0,
        Some(f) => match &f.from {
            FrameOffset::Unbounded => 0,
            FrameOffset::Int(d) => (position as i64 + d).max(0) as usize,
            FrameOffset::Numeric(_) => position, // Range: follow-up
        },
    }
}

/// High boundary index of `frame` for a row at `position` in a
/// partition of size `n`. Mirrors Java `Rows.getHigh`: `toUnbounded ?
/// n-1 : min(n-1, position + offsetTo)`. Used by `last` / `nth`.
/// `None` frame (no ORDER BY) -> `n-1` (whole partition).
pub(crate) fn frame_high(frame: Option<&Frame>, position: usize, n: usize) -> usize {
    let last = n.saturating_sub(1);
    match frame {
        None => last,
        Some(f) => match &f.to {
            FrameOffset::Unbounded => last,
            FrameOffset::Int(d) => {
                let v = position as i64 + d;
                v.clamp(0, last as i64) as usize
            }
            FrameOffset::Numeric(_) => position, // Range: follow-up
        },
    }
}

#[derive(Clone, Copy)]
enum FrameSide {
    From,
    To,
}

enum OffsetResult {
    /// Offset resolved cleanly within `[0, n)`.
    At(usize),
    /// Offset is out of bounds on the "wrong" side for this boundary —
    /// `from` past the end of the partition, or `to` before the start.
    /// Treat as empty frame.
    Empty,
}

/// Resolve a `FrameOffset` to a position relative to `position` in
/// `[0, n)`. Directional clamping: for `from`, below-0 clamps to 0
/// and past-end is empty; for `to`, below-0 is empty and past-end
/// clamps to `n-1`. `Unbounded` resolves to start (`from`) or end
/// (`to`); `Numeric` (Range) is not yet implemented and falls back
/// to current position (caller-facing fallback).
fn resolve_offset(
    offset: &FrameOffset,
    position: usize,
    n: usize,
    side: FrameSide,
) -> OffsetResult {
    match offset {
        FrameOffset::Unbounded => match side {
            FrameSide::From => OffsetResult::At(0),
            FrameSide::To => OffsetResult::At(n.saturating_sub(1)),
        },
        FrameOffset::Int(delta) => {
            let signed = position as i64 + delta;
            if signed < 0 {
                match side {
                    FrameSide::From => OffsetResult::At(0),
                    FrameSide::To => OffsetResult::Empty,
                }
            } else if (signed as usize) >= n {
                match side {
                    FrameSide::From => OffsetResult::Empty,
                    FrameSide::To => OffsetResult::At(n.saturating_sub(1)),
                }
            } else {
                OffsetResult::At(signed as usize)
            }
        }
        FrameOffset::Numeric(_) => OffsetResult::At(position), // Range not yet impl
    }
}

// ---------------------------------------------------------------------------
// Collection-flatten helper (shared by reduce + extend OLAP natives)
// ---------------------------------------------------------------------------

/// Append a Value (possibly a Collection) to `out`, flattening one
/// Collection layer and dropping Unit. Mirrors the shape `filter` /
/// `map` rely on.
pub(crate) fn push_flat(out: &mut PVector<Value>, v: &Value) {
    match v {
        Value::Collection(inner) => {
            for item in inner.iter() {
                out.push_back(item.clone());
            }
        }
        Value::Unit => {}
        scalar => out.push_back(scalar.clone()),
    }
}

// ---------------------------------------------------------------------------
// Row-tuple identification (for the standalone `reduce` native)
// ---------------------------------------------------------------------------

/// Match a row-tuple heap object back to a source row index by
/// comparing each TDS column's slot value against the parsed cells.
///
/// Returns the first matching row index. Used by `reduce(rel, w, row,
/// …)` to locate `row`'s position within its partition (the lambda
/// inside `extend` passes the row as a `Value::Object` whose slots
/// were populated by `build_row_tuple`).
#[allow(clippy::result_large_err)]
pub(crate) fn row_tuple_to_source_index(
    row_tuple: &ObjectHandle,
    parsed: &ParsedTDS,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Option<usize>, PureException> {
    // Snapshot each TDS column's slot value off the row-tuple.
    let mut slot_values: Vec<Option<Value>> = Vec::with_capacity(parsed.columns.len());
    for col in &parsed.columns {
        let values = ctx
            .heap()
            .get_property_values(row_tuple, col.name.as_str())
            .map_err(PureException::from)?;
        slot_values.push(
            values
                .iter()
                .find(|v| !matches!(v, Value::Unit))
                .cloned(),
        );
    }
    for (idx, row) in parsed.rows.iter().enumerate() {
        if row_matches(row, &slot_values) {
            return Ok(Some(idx));
        }
    }
    Ok(None)
}

fn row_matches(row: &[Option<TypedCell>], slot_values: &[Option<Value>]) -> bool {
    for (cell, slot) in row.iter().zip(slot_values.iter()) {
        let cell_v = cell.as_ref().map(typed_cell_to_value_ref);
        match (&cell_v, slot) {
            (None, None) => continue,
            (Some(cv), Some(sv)) if values_eq(cv, sv) => continue,
            _ => return false,
        }
    }
    true
}

fn typed_cell_to_value_ref(cell: &TypedCell) -> Value {
    match cell {
        TypedCell::Integer(i) => Value::Integer(*i),
        TypedCell::Float(f) => Value::Float(*f),
        TypedCell::Boolean(b) => Value::Boolean(*b),
        TypedCell::String(s)
        | TypedCell::Decimal(s)
        | TypedCell::StrictDate(s)
        | TypedCell::DateTime(s) => Value::String(s.clone()),
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        _ => false,
    }
}
