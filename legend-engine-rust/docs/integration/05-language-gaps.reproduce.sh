#!/usr/bin/env bash
# Copyright 2026 Goldman Sachs
# Apache-2.0
#
# Regenerate the inputs that back 05-language-gaps.md. Run from the
# legend-engine-rust workspace root (or anywhere — the script
# resolves paths absolutely).
#
# Two complementary signals:
#  (1) `legend-engine build` — topo-ordered build via the classpath
#      TOML; aborts at first failure, reports root-cause + cascade
#      counts to /tmp/legend-engine-gap.{ndjson,stderr}.
#  (2) Per-repo `legend-engine check` — parse-only sweep over every
#      [[repo]] in the classpath; no dependency ordering; surfaces
#      every parser gap in parallel. Output:
#      /tmp/per-repo-check.tsv (TSV: repo, status, files, errors, top_error)
#      A snapshot lives at 05-language-gaps.per-repo.tsv alongside
#      this script for reference; rerun this script to refresh it.
#
# After running, refresh the prose summary in 05-language-gaps.md
# (counts table, clusters table, top-20 table, next-session
# prioritisation) by hand.

set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CLI="$WORKSPACE_ROOT/target/debug/legend-engine"
TOML="$WORKSPACE_ROOT/legend-pure-classpath.toml"
ENGINE_ROOT="$(dirname "$WORKSPACE_ROOT")"

if [[ ! -x "$CLI" ]]; then
    echo "==> Building legend-engine-rust-cli (one-shot; ~8 min cold, seconds warm)"
    (cd "$WORKSPACE_ROOT" && cargo build -p legend-engine-rust-cli)
fi

if [[ ! -f "$TOML" ]]; then
    echo "==> Regenerating classpath TOML"
    (cd "$WORKSPACE_ROOT" && cargo gen-classpath)
fi

# ---------------------------------------------------------------------
# Signal 1 — topological build
# ---------------------------------------------------------------------

echo "==> Signal 1: legend-engine build (topo-ordered, fail-fast)"
"$CLI" build \
    --classpath "$TOML" \
    --format json --skip-tests --no-write-purem \
    > /tmp/legend-engine-gap.ndjson \
    2> /tmp/legend-engine-gap.stderr \
    || true     # non-zero exit is the point — capture the failure
echo "    wrote /tmp/legend-engine-gap.{ndjson,stderr}"
tail -2 /tmp/legend-engine-gap.stderr || true

# ---------------------------------------------------------------------
# Signal 2 — per-repo parser-only sweep
# ---------------------------------------------------------------------

OUT=/tmp/per-repo-check.tsv
printf "repo\tstatus\tfiles\terrors\ttop_error\n" > "$OUT"

echo ""
echo "==> Signal 2: per-repo legend-engine check"

paste -d'|' \
    <(awk -F\" '/^name = "/ {print $2}' "$TOML") \
    <(awk -F\" '/^descriptor = "/ {print $2}' "$TOML") \
| while IFS='|' read -r name desc; do
    desc_dir=$(dirname "$desc")
    abs="$WORKSPACE_ROOT/$desc_dir/$name"

    if [[ ! -d "$abs" ]]; then
        printf "%s\t%s\t0\t0\t-\n" "$name" "NO_SOURCE_DIR" >> "$OUT"
        continue
    fi

    if ! json=$("$CLI" check "$abs" --format json 2>/dev/null); then
        # Non-zero exit (e.g. parser stack overflow) — no summary line.
        printf "%s\t%s\t0\t0\t-\n" "$name" "NO_SUMMARY" >> "$OUT"
        continue
    fi

    summary=$(printf '%s\n' "$json" | awk '/^\{"errors":/ {print; exit}')
    if [[ -z "$summary" ]]; then
        printf "%s\t%s\t0\t0\t-\n" "$name" "NO_SUMMARY" >> "$OUT"
        continue
    fi

    files=$(printf '%s' "$summary"  | python3 -c 'import sys,json; print(json.load(sys.stdin)["files"])')
    errors=$(printf '%s' "$summary" | python3 -c 'import sys,json; print(json.load(sys.stdin)["errors"])')
    top_error=$(printf '%s\n' "$json" \
        | awk -F'"' '/^\{"code":"parseFailure"/ {for(i=1;i<=NF;i++) if($i=="message") {print $(i+2); exit}}' \
        | head -1)

    if [[ "$errors" == "0" ]]; then
        printf "%s\tOK\t%s\t0\t-\n" "$name" "$files" >> "$OUT"
    else
        printf "%s\tFAIL\t%s\t%s\t%s\n" "$name" "$files" "$errors" "${top_error:-?}" >> "$OUT"
    fi
done

echo "    wrote $OUT ($(wc -l < "$OUT") lines)"

# ---------------------------------------------------------------------
# Quick analysis printout
# ---------------------------------------------------------------------

echo ""
echo "==> Status distribution"
awk -F'\t' 'NR>1 {n[$2]++; if ($2=="FAIL") tot+=$4} END {
    for (s in n) printf "  %-15s %d\n", s, n[s];
    printf "  %-15s %d\n", "total_errors", tot
}' "$OUT"

echo ""
echo "==> Top 10 failing repos by error count"
awk -F'\t' 'NR>1 && $2=="FAIL" {print $4 "\t" $1 "\t" $5}' "$OUT" \
    | sort -rn | head -10

echo ""
echo "==> Error clusters"
awk -F'\t' 'NR>1 && $2=="FAIL" {print $5}' "$OUT" | sort | uniq -c | sort -rn

echo ""
echo "Refresh 05-language-gaps.md by hand from the above + the OUT file."
echo "To snapshot, copy: cp $OUT $(dirname "${BASH_SOURCE[0]}")/05-language-gaps.per-repo.tsv"
