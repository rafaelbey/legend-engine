# 02 — Deferred compile issues in `platform_store_relational`

> **Status:** Discovered during groundwork (G2). Locked by
> `crates/integration-smoke/tests/relational_metamodel_compiles.rs`.
> Out of scope to fix in the groundwork plan; tracked for the
> follow-up integration plan.

Compiling `platform + platform_store_relational` through the Rust pipeline today produces **5 dispatch-narrowing errors**. They are real bugs in the Rust compiler's overload resolution, not in the upstream Pure source — the Java compiler resolves both function calls cleanly.

## The 5 errors

```
/platform_store_relational/functions.pure (302:60-302:77)
  Ambiguous function call 'filterBySchemaName': found 2 overloads with 2 args (narrowed from 2 candidates)

/platform_store_relational/relationalRuntime.pure (72:104-72:105)
  Ambiguous function call 'elementToPath': found 3 overloads with 1 args (narrowed from 3 candidates)

/platform_store_relational/relationalRuntime.pure (76:113-76:114)
  Ambiguous function call 'elementToPath': found 3 overloads with 1 args (narrowed from 3 candidates)

/platform_store_relational/relationalRuntime.pure (92:92-92:93)
  Ambiguous function call 'elementToPath': found 3 overloads with 1 args (narrowed from 3 candidates)

/platform_store_relational/relationalRuntime.pure (96:89-96:90)
  Ambiguous function call 'elementToPath': found 3 overloads with 1 args (narrowed from 3 candidates)
```

## Issue 1 — `elementToPath` on `Function<Any>[1]` (×4)

**Calls:** `relationalRuntime.pure:72,76,92,96` — all in qualified-property bodies that read `$this.<funcSlot>->toOne()->elementToPath()` where `<funcSlot>` is typed `Function<Any>[0..1]` or `ConcreteFunctionDefinition<Any>[0..1]`.

**Receiver after `->toOne()`:** `Function<Any>[1]` (or `ConcreteFunctionDefinition<Any>[1]`).

**Platform overload count:** 3. Of those, `meta::pure::functions::meta::elementToPath(PackageableElement[1]):String[1]` is the structurally correct match — `Function` extends `FunctionDefinition` extends `PackageableElement`.

**Likely Rust-compiler gap:** the dispatch scoring isn't preferring the most-specific applicable overload when supertype walking is involved. Cross-reference BACKLOG `crates/pure` items "Full generic unification (`Z` propagation)" and "Return type influence on dispatch."

## Issue 2 — `filterBySchemaName` (×1)

**Call:** `functions.pure:302`.

```
filterBySchemaName($aliasesByTableAliasName, $tableAlias.relationalElement->cast(@NamedRelation));
```

**Argument types:** `JoinTreeNode[*]` and `NamedRelation[1]`.

**Platform overloads (2):** both apparently match the `(*, [1])` arity shape; the compiler emits "narrowed from 2 candidates." The "true ambiguity" reading is unlikely (Java picks one); more likely a multiplicity-narrowing or generic-binding gap.

## Why this is groundwork value, not a setback

The smoke test was specifically built to catch issues like these *before* anyone tries to use the relational metamodel for real (constructing a `Database`, writing a relational mapping, attempting to compile a test fixture). Discovery is cheap and concrete; we now have:

- A bounded list of 5 specific compile errors blocking relational integration.
- File-and-line citations into upstream Pure source.
- A regression-detection lock (`platform_plus_relational_compiles_with_known_errors`) that fails if NEW errors appear.
- A latent strict lock (`platform_plus_relational_metamodel_compiles_clean`, `#[ignore]`) that fires the moment all 5 are fixed.

## What "fix" looks like

The fixes live in `legend-pure-rust/crates/pure/` (the compiler crate), not here. The follow-up integration plan opens with:

1. Reproduce each error against a minimal `.pure` fixture (1 file, 1 function, calls the ambiguous overload). Land those fixtures in `crates/pure/tests/dispatch_relational_lock.rs`.
2. Trace the dispatch scoring path; identify why the equivalent Java overload selection isn't picked.
3. Land the structural fix in `resolve_function_call` per the dispatch design doc (`crates/pure/FUNCTION_DISPATCH.md`).
4. Lift the `#[ignore]` on `platform_plus_relational_metamodel_compiles_clean`.

CLAUDE.md "no tactical test-pass hacks" applies: we do not work around these by editing the Pure source, by adding type ascriptions, or by special-casing function names in the Rust compiler. Each fix targets the dispatch algorithm.
