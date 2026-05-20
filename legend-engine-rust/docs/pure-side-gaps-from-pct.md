# Pure-side gaps surfaced by the engine PCT runner

Four narrowing / lowering gaps in `legend-pure-rust` block PCT
coverage that's otherwise within reach. Each gap is locked by an
`#[ignore]`d test in
`legend-engine-rust/crates/integration-smoke/tests/pct_relation_smoke.rs`;
this document collects the diagnoses + suggested fix surfaces so an
expert can pick them up.

When a fix lands, lift the corresponding `#[ignore]` — the test
turns green without any engine-side change.

Current engine workspace: **83 passed / 0 failed / 7 ignored**.
Lifting all four would reach **~90 / 0 / 0** on the present test
set.

---

## #1 — Narrower can't tiebreak `relation::map` vs `collection::map` when arg0 is a Generic-typed receiver

**Affected PCT tests**

- `meta::pure::functions::relation::tests::sort::testSimpleSortShared`
- `meta::pure::functions::relation::tests::sort::testSimpleSort_MultipleExpressions`

**Engine-side `#[ignore]` rows**

- `pct_sort_testSimpleSortShared`
- `pct_sort_testSimpleSort_MultipleExpressions`

**Minimal Pure**

```pure
function <T|m>(f:Function<{Function<{->T[m]}>[1]->T[m]}>[1]):Boolean[1]
{
    let expr = {| #TDS id, name\n 2, A\n 1, B\n# -> sort(ascending(~id)) };
    let res = $f->eval($expr);                  // $res: Relation<{id,name}>[1]
    assertEquals([1,2], $res->map(x|$x.id));    // <-- dispatch tip-over here
}
```

**Symptom**

```
assertEquals: expected [1, 1, 2, 2, 3, 4, 5], actual []
```

`$res->map(x|$x.id)` ends up returning an empty value because the
narrower picks `meta::pure::functions::collection::map<T,V|m>(T[m],
Function<{T[1]->V[m]}>[1]):V[m]` instead of
`meta::pure::functions::relation::map<T,V>(Relation<T>[1],
Function<{T[1]->V[*]}>[1]):V[*]`. With the collection overload, the
lambda is invoked **once** on the entire TDS heap object, accesses
`.id` (which isn't a property on the TDS classifier), and returns
empty.

**Diagnosis**

`$res`'s resolved type post Z-prop is `Relation<{id:Integer,
name:String}>[1]` — concretely `Named`. Both candidates survive
Phase-1 of `narrow_candidates_by_type`:

| Candidate | param0 | Phase-1 verdict |
|---|---|---|
| `relation::map(Relation<T>[1], …)` | `Named{Relation<T>}` | `is_subtype(Relation, Relation) = true` → accept |
| `collection::map(T[m], …)` | `Generic("T")` | `is_type_compatible`'s Generic arm returns `true` unconditionally → accept |

Phase-2 (specificity rank) and Phase-3 (most-specific-param
elimination) can't compare a Named param to a Generic param —
`type_distance(Relation, Generic("T"))` has no answer — so neither
dominates and the dispatcher tips over to the wrong choice
(order-dependent on the candidate iteration; collection wins in
the current setup).

**Suggested fix surface**

`legend-pure-rust/crates/pure/src/resolve.rs::narrow_candidates_by_type`
— add a tiebreak rule (probably Phase-2 or a new Phase-3.5) of the
shape:

> Among candidates that all pass Phase-1, prefer those whose param
> at position `i` is `Named` over those whose param at position
> `i` is `Generic`, when the inferred arg type at position `i` is
> concretely `Named`.

This is the same shape as the Z-prop-era sort/extend narrowing
tiebreak, one level deeper in the ranking. Same fix at param-0,
not param-1+.

Locking test (turns green on fix):
`legend-engine-rust/crates/integration-smoke/tests/narrowing_pct_generic_sort_repro.rs::probe_sort_on_pct_generic_compiles_cleanly`
+ the two `pct_sort_*` PCT tests above.

---

## #2 — `String + Numeric->toString()` chain doesn't reduce before `plus_String` dispatch

**Affected PCT tests**

- `meta::pure::functions::relation::tests::extend::testSimpleExtendStrShared`
- `meta::pure::functions::relation::tests::extend::testSimpleExtendStr_MultipleExpressions`

**Engine-side `#[ignore]` rows**

- `pct_extend_testSimpleExtendStrShared`
- `pct_extend_testSimpleExtendStr_MultipleExpressions`

**Minimal Pure**

```pure
#TDS val:Integer, str:String\n 1, a\n#
   -> extend(~name:c | $c.str->toOne() + $c.val->toOne()->toString())
```

Expected new column value for row 1: `'a1'` (`'a' + 1->toString()`).

**Symptom**

```
Execution error
"Type mismatch: expected Object, got Integer"

Full Stack:
    extend_Relation_1__FuncColSpec_1__Relation_1_  <- extend.pure line 54
    plus_String_MANY__String_1_                    <- extend.pure line 61 column 51
```

The `plus_String_MANY__String_1_` native fires with `String + Integer`
instead of `String + String` — the `->toString()` postfix call on
the right-hand `$c.val->toOne()` chain didn't reduce to a `String`
before the outer `+` dispatched.

**Diagnosis (working hypothesis)**

Operator precedence / chain-lowering: the body `$c.str->toOne() +
$c.val->toOne()->toString()` may be lowering as `($c.str->toOne() +
$c.val->toOne())->toString()` — applying `->toString()` to the sum
rather than to the inner `$c.val->toOne()`. With that parse,
`plus_String` sees `String + Integer` and rejects.

Numeric-only extend variants (`~doubled:c|$c.val->toOne() * 2`)
pass cleanly — those don't exercise the `String + Numeric->toString()`
chain, so they don't tell us anything about the parse-precedence
question.

**Suggested fix surface**

`legend-pure-rust/crates/pure/src/lower/` (or the parser one layer
up) — verify that `a + b->c()` lowers as `a + (b->c())`, not
`(a + b)->c()`. The Pure grammar binds `->` postfix tighter than
arithmetic `+` per the m3 reference; if the Rust parser disagrees
that's the bug.

Reproducer that doesn't need the PCT shape:

```pure
function pure_repro::p(): Boolean[1]
{
    assertEquals('a1', 'a' + 1->toString())
}
```

If the above fails the same way, the issue is independent of
extend / relation natives.

---

## #3 — `~'name'` (quoted-name ColSpec literal) keeps the `'` quotes in the lowered `name` slot

**Affected PCT tests**

- `meta::pure::functions::relation::tests::select::testSingleSelectWithQuotedColumn`
- `meta::pure::functions::relation::tests::select::testSingleSelectWithQuotedColumn_MultipleExpressions`

**Engine-side `#[ignore]` rows**

- `pct_select_testSingleSelectWithQuotedColumn`
- `pct_select_testSingleSelectWithQuotedColumn_MultipleExpressions`

**Minimal Pure**

```pure
#TDS val, str, 'other kind'\n 1, a, x\n#
   -> select(~'other kind')
```

The TDS header `'other kind'` declares a column whose name contains
a space. `~'other kind'` is the ColSpec literal that references it.

**Symptom**

```
Execution error
"select: column ''other kind'' not present in receiver;
 have [\"val\", \"str\", \"other kind\"]"
```

The receiver's columns are `val`, `str`, `other kind` (quotes
stripped by the TDS header parser — correct). The lookup arrives
with `'other kind'` (quotes retained) — mismatch.

**Diagnosis**

The ColSpec lower for `~'name'` is writing the surface-syntax form
into the `name` slot of the lowered `RelationColumnLowered`. The
surrounding `'` are part of the surface syntax (they exist to allow
special characters in identifiers) and shouldn't appear in the
canonical name.

The TDS header path strips them correctly:

```
#TDS  val, str, 'other kind'   ->  columns: ["val", "str", "other kind"]
```

So the bug is local to ColSpec lowering, not TDS header parsing.

**Suggested fix surface**

`legend-pure-rust/crates/pure/src/lower/relation.rs` —
`lower_relation_columns_from_specs` (or the `lower_column` arm
that builds the `RelationColumnLowered.name`). When the source
column name is wrapped in `'…'`, strip the surrounding quotes.
Match the TDS header parser's behaviour.

Reproducer that doesn't need the engine catalog:

```pure
function pure_repro::p(): Boolean[1]
{
    let r = #TDS  'col one'\n  1\n#;
    let cs = $r->select(~'col one');
    true
}
```

If `select` reports `column ''col one'' not present in receiver`,
the lowering retained the quotes.

---

## #4 — `eval(ColSpec, row)` Pure body hits an empty `columns` slot

**Affected PCT test**

- `meta::pure::functions::relation::tests::eval::testSimpleEval`

**Engine-side `#[ignore]` row**

- `pct_eval_testSimpleEval`

**Symptom**

```
Execution error
"The system is trying to get an element at offset 0 where the
 collection is of size 0"
```

**Diagnosis**

`eval(ColSpec, row)` is Pure-defined in
`core_functions_relation/relation/functions/eval.pure:18`:

```pure
function meta::pure::functions::relation::eval<Z,T>(
    col:ColSpec<(?:Z)⊆T>[1], row:T[1]
) : Z[0..1]
{
  $col->genericType().typeArguments->at(0).rawType->toOne()
       ->cast(@RelationType<Any>).columns->toOne()
       ->cast(@Column<Nil,Z|0..1>)->eval($row);
}
```

The body walks the reflective chain through the ColSpec's
`classifierGenericType.typeArguments[0].rawType.columns`. In our
runtime, `alloc_col_spec_literal` builds the ColSpec heap shape
with:

- `name: String[1]` ✓
- `classifierGenericType.rawType = ColSpec` ✓
- `classifierGenericType.typeArguments[0].rawType = Column` ✓ (or
  the column type — see `relation.rs::alloc_col_spec_literal`)
- but no `RelationType` wrapping with a populated `columns: Column[*]`
  slot — that level the chain expects isn't materialised.

`->at(0)` on an empty `columns` collection fires the indexing
error.

**Suggested fix surface**

Two viable paths, expert's call:

(a) **Engine fix** — extend `alloc_col_spec_literal` (in
`legend-pure-rust/crates/runtime/src/relation.rs`) so the inner
`classifierGenericType.typeArguments[0]` chain populates a
`RelationType` wrapper whose `columns: Column[*]` slot carries the
column the ColSpec refers to. Mirrors what the Java
`_ColSpec.getColSpecInstance` builds.

(b) **Pure fix** — simplify the `eval(ColSpec, row)` body to read
`$col->name()` (the `name` slot we already populate) and project the
row directly, without walking the `typeArguments->rawType.columns`
chain. The chain is currently a Java-Pure-interpreted-mode artefact;
a simpler body would work on both Java and Rust runtimes.

The cleaner pure-side fix is (b) — the heap-shape work (a) is
nominally engine but the reflection contract that requires the
`columns` slot belongs to the platform metamodel. Either way the
test goes green.

---

## Locking guard

A separate engine-side regression probe lives at

```
legend-engine-rust/crates/integration-smoke/tests/narrowing_pct_generic_sort_repro.rs
```

It already passes (locking the Z-propagation fix). Adding similar
focused probes for #2 / #3 / #4 — independent of the PCT corpus — is
straightforward when the time comes.
