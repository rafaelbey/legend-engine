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

//! `map(rel:Relation<T>[1], f:Function<{T[1]->V[*]}>[1]):V[*]` — engine
//! native.
//!
//! Sister of `filter` — same per-row lambda binding template, but
//! yields the lambda's *result* per row instead of using it as a
//! Boolean predicate. The return type is the platform's flat
//! collection (`V[*]`), so multi-value lambda results flatten.
//!
//! Used by PCT tests like `$res->map(x|$x.id)` to project a single
//! column out of a TDS into a flat collection.

#![allow(clippy::needless_pass_by_value)]

use im_rc::Vector as PVector;
use legend_pure_parser_pure::types::ValueSpec;

use legend_pure_runtime::error::PureException;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use legend_pure_runtime::native::relation::shared::{
    build_row_tuple, read_parsed_tds, unwrap_instance_value,
};

/// Pure `map<T,V>(rel:Relation<T>[1], f:Function<{T[1]->V[*]}>[1]):V[*]`.
#[derive(Debug)]
pub struct MapRelation;

impl NativeFunction for MapRelation {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("map (Relation)", args, 2)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // Receiver TDS -> parse cells + column metadata.
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("map", &tds_obj, ctx)?;

        // Mapper lambda.
        let lambda_val = ctx.evaluate(&args[1])?.into_value();

        // Per-row evaluation. Lambda result is `V[*]`; flatten
        // Collection results, drop Unit, otherwise push as a scalar.
        // Borrow-pattern match because `Value` now implements `Drop`
        // (iterative-Drop refactor upstream); destructure-by-value
        // would move out of a Dropful enum.
        let mut out: PVector<Value> = PVector::new();
        for row in &parsed.rows {
            let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
            let v = ctx.call_function(&lambda_val, &[Value::Object(row_tuple)])?;
            match &v {
                Value::Collection(inner) => {
                    for item in inner.iter() {
                        out.push_back(item.clone());
                    }
                }
                Value::Unit => {}
                _ => out.push_back(v.clone()),
            }
        }
        Ok(Evaluated::new(if out.is_empty() {
            Value::Unit
        } else if out.len() == 1 {
            out.pop_front().unwrap_or(Value::Unit)
        } else {
            Value::Collection(Box::new(out))
        }))
    }
}
