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

//! `drop(rel:Relation<T>[1], size:Integer[1]):Relation<T>[1]` — skip
//! the first `size` rows of a TDS.

#![allow(clippy::needless_pass_by_value)]

use legend_pure_parser_pure::types::ValueSpec;

use legend_pure_runtime::error::PureException;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use legend_pure_dsl_tds::csv::ParsedTDS;
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, read_parsed_tds, unwrap_instance_value,
};

/// Pure
/// `drop<T>(rel:Relation<T>[1], size:Integer[1]):Relation<T>[1]`.
///
/// Returns a fresh TDS with the first `size` rows skipped. `size`
/// values larger than the row count produce an empty TDS (header
/// preserved); negative values are treated as zero, mirroring the
/// platform-side handling exercised by `<<PCT.test>>` in
/// `slice/drop.pure`.
///
/// Reads the input via [`read_parsed_tds`], slices off the leading
/// rows, and re-emits the remaining rows + source column metadata as a
/// fresh `TDS` via [`alloc_tds_from_parsed`].
#[derive(Debug)]
pub struct Drop;

impl NativeFunction for Drop {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("drop (Relation, Integer)", args, 2)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("drop", &tds_obj, ctx)?;

        let count_value = ctx.evaluate(&args[1])?.into_value();
        let count = count_value.as_integer()?;
        let skip = if count < 0 {
            0
        } else {
            usize::try_from(count)
                .unwrap_or(usize::MAX)
                .min(parsed.rows.len())
        };

        let kept: Vec<Vec<Option<legend_pure_dsl_tds::csv::TypedCell>>> =
            parsed.rows[skip..].to_vec();
        let result = ParsedTDS {
            csv: String::new(),
            columns: parsed.columns,
            rows: kept,
        };
        let tds_handle = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(tds_handle)))
    }
}
