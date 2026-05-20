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

//! `toString(Relation<T>[1]):String[1]` (and the 2-arg `typesAndMuls`
//! overload) — engine native that renders a TDS in the
//! PCT-test-expected shape:
//!
//! ```text
//! #TDS
//!    col1,col2,...
//!    v1a,v1b,...
//!    v2a,v2b,...
//! #
//! ```
//!
//! Overrides the Pure-defined body in
//! `core_functions_relation/relation/functions/toString.pure`. The
//! Pure body is `<<PCT.function, PCT.platformOnly>>` — explicitly a
//! fallback for runtimes without a native impl. Our native takes
//! dispatch precedence (see `eval.rs::call_function` Step 2a — the
//! `NativeRegistry` lookup runs before falling through to the
//! function body).
//!
//! Cell-rendering rules mirror the platform's
//! `meta::pure::functions::relation::s(a:Any[0..1], type:Type[0..1])`
//! Pure body:
//!
//! - empty cell -> `'null'` (or `'"null"'` for a `Variant` column).
//! - `Variant` value -> `'"' + toString + '"'` with internal `"`
//!   doubled.
//! - `String` value -> as-is, unless the content contains `{` or `[`
//!   in which case it's quoted and `"` doubled.
//! - Numeric / Boolean / Date / DateTime -> their plain string form
//!   (the same `toString` the platform `s` arm produces).
//!
//! The `typesAndMuls=true` overload appends `:<Type>[<mult>]` to each
//! header cell. Type and multiplicity render through this crate's own
//! formatting — no dependency on the platform's `_printGenericType`
//! reflection chain (which currently trips on the missing platform
//! `matches(String, String)` native).

#![allow(clippy::needless_pass_by_value)]

use legend_pure_dsl_tds::csv::{ColumnType, ParsedColumn, ParsedTDS, TypedCell};
use legend_pure_parser_pure::types::{Multiplicity, ValueSpec};
use smol_str::SmolStr;

use legend_pure_runtime::error::PureException;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::native::relation::shared::{read_parsed_tds, unwrap_instance_value};
use legend_pure_runtime::value::Value;

/// `toString(Relation<T>[1]):String[1]` — engine native.
#[derive(Debug)]
pub struct ToStringRelation;

impl NativeFunction for ToStringRelation {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("toString (Relation)", args, 1)?;
        let parsed = parse_relation_arg(&args[0], ctx, "toString")?;
        let s = render_tds(&parsed, false);
        Ok(Evaluated::new(Value::String(SmolStr::new(&s))))
    }

    fn signature(&self) -> &'static str {
        "toString(Relation<T>[1]):String[1]"
    }
}

/// `toString(Relation<T>[1], typesAndMuls:Boolean[1]):String[1]` — engine
/// native variant that decorates the header with `:Type[mult]`.
#[derive(Debug)]
pub struct ToStringRelationTyped;

impl NativeFunction for ToStringRelationTyped {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("toString (Relation, Boolean)", args, 2)?;
        let parsed = parse_relation_arg(&args[0], ctx, "toString")?;
        let typed_and_muls = match ctx.evaluate(&args[1])?.into_value() {
            Value::Boolean(b) => b,
            other => {
                return Err(PureException::from(
                    legend_pure_runtime::error::PureRuntimeError::EvaluationError(format!(
                        "toString (Relation, Boolean): arg 2 must be Boolean, got {}",
                        other.type_name()
                    )),
                ));
            }
        };
        let s = render_tds(&parsed, typed_and_muls);
        Ok(Evaluated::new(Value::String(SmolStr::new(&s))))
    }

    fn signature(&self) -> &'static str {
        "toString(Relation<T>[1], Boolean[1]):String[1]"
    }
}

#[allow(clippy::result_large_err)]
fn parse_relation_arg(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    fn_name: &'static str,
) -> Result<ParsedTDS, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let value = ctx.evaluate(arg)?.into_value();
    let tds_obj = unwrap_instance_value(&value, instance_value_id, ctx)?;
    read_parsed_tds(fn_name, &tds_obj, ctx)
}

fn render_tds(parsed: &ParsedTDS, types_and_muls: bool) -> String {
    let mut out = String::with_capacity(64 + parsed.rows.len() * 32);
    out.push_str("#TDS\n");
    out.push_str("   ");
    let header: Vec<String> = parsed
        .columns
        .iter()
        .map(|c| render_header_cell(c, types_and_muls))
        .collect();
    out.push_str(&header.join(","));
    for row in &parsed.rows {
        out.push('\n');
        out.push_str("   ");
        let cells: Vec<String> = parsed
            .columns
            .iter()
            .zip(row.iter())
            .map(|(col, cell)| render_cell(cell.as_ref(), &col.type_tag))
            .collect();
        out.push_str(&cells.join(","));
    }
    out.push_str("\n#");
    out
}

fn render_header_cell(col: &ParsedColumn, types_and_muls: bool) -> String {
    let name = col.name.as_str();
    let name_simple = is_simple_identifier(name);
    let quoted = if name_simple {
        name.to_string()
    } else {
        format!("'{name}'")
    };
    if !types_and_muls {
        return quoted;
    }
    format!(
        "{quoted}:{}[{}]",
        pure_type_path(&col.type_tag),
        render_multiplicity(&col.multiplicity),
    )
}

fn is_simple_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn pure_type_path(t: &ColumnType) -> String {
    match t {
        ColumnType::Integer => "Integer".into(),
        ColumnType::Float => "Float".into(),
        ColumnType::Decimal => "Decimal".into(),
        ColumnType::Boolean => "Boolean".into(),
        ColumnType::String => "String".into(),
        ColumnType::StrictDate => "StrictDate".into(),
        ColumnType::DateTime => "DateTime".into(),
        ColumnType::Other { package, name } => match package {
            Some(p) => format!("{p}::{name}"),
            None => name.to_string(),
        },
    }
}

fn render_multiplicity(m: &Multiplicity) -> String {
    match m {
        Multiplicity::PureOne => "1".into(),
        Multiplicity::ZeroOrOne => "0..1".into(),
        Multiplicity::OneOrMany => "1..*".into(),
        Multiplicity::ZeroOrMany => "*".into(),
        Multiplicity::Range {
            lower,
            upper: Some(u),
        } => format!("{lower}..{u}"),
        Multiplicity::Range { lower, upper: None } => format!("{lower}..*"),
        Multiplicity::Variable(name) => name.to_string(),
    }
}

/// Mirrors the platform `s(a:Any[0..1], type:Type[0..1]):String[1]`
/// rules. The column's `ColumnType` stands in for the `type` arg —
/// `Variant` here means a column whose declared `type_tag` is the
/// `Other { name: "Variant", .. }` shape.
fn render_cell(cell: Option<&TypedCell>, column_type: &ColumnType) -> String {
    let is_variant_col = matches!(column_type, ColumnType::Other { name, .. } if name == "Variant");
    let Some(cell) = cell else {
        return if is_variant_col {
            "\"null\"".into()
        } else {
            "null".into()
        };
    };
    match cell {
        TypedCell::Integer(i) => i.to_string(),
        TypedCell::Float(f) => {
            // Match Java Pure's Float.toString: whole-valued floats
            // render as `1.0`, not `1` — keeps reparse-as-Float
            // consistent.
            let s = format!("{f:?}");
            s
        }
        TypedCell::Decimal(s) => s.to_string(),
        TypedCell::Boolean(true) => "true".into(),
        TypedCell::Boolean(false) => "false".into(),
        TypedCell::String(s) => {
            if is_variant_col {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else if s.contains('{') || s.contains('[') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        }
        TypedCell::StrictDate(s) | TypedCell::DateTime(s) => s.to_string(),
    }
}
