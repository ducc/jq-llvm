#!/usr/bin/env bash
# Compute-bound benchmarks (no input parsing): jq vs jqc.
set -u
cd "$(dirname "$0")/.."
JQC=${JQC:-./target/release/jq-llvm}
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
bench() {
  local name=$1 filter=$2
  $JQC -O2 -o "$work/p" "$filter" || { echo "$name: compile failed"; return; }
  local t0 t1 t2
  t0=$(date +%s.%N); jq -nc "$filter" > "$work/a"; t1=$(date +%s.%N)
  "$work/p" -nc > "$work/b"; t2=$(date +%s.%N)
  local same=same; cmp -s "$work/a" "$work/b" || same=DIFFERENT
  awk -v n="$name" -v a="$t0" -v b="$t1" -v c="$t2" -v s="$same" 'BEGIN { printf "%-26s jq %7.3fs   jqc %7.3fs   speedup %6.1fx  (%s)\n", n, b-a, c-b, (b-a)/(c-b), s }'
}
bench "reduce sum 1e7"      'reduce range(10000000) as $i (0; . + $i)'
bench "map/add 1e6"         '[range(1000000)] | map(. * 2) | add'
bench "fib 25 (recursion)"  'def fib: if . < 2 then . else (. - 1 | fib) + (. - 2 | fib) end; 25 | fib'
bench "select count 1e6"    '[range(1000000) | select(. % 3 == 0)] | length'
bench "tostring/join 1e5"   '[range(100000) | tostring] | join(",") | length'
bench "objects 1e6"         '[range(1000000) | {a: ., b: (. * 2)}] | map(.a + .b) | add'
bench "sort_by 1e6"         '[range(1000000) | (. * 7919) % 1000003] | sort | .[0:3]'
bench "group_by 1e6"        '[range(1000000) | {k: (. % 100), v: .}] | group_by(.k) | length'
bench "string append 1e5"   'reduce range(100000) as $i (""; . + "x") | length'
bench "array append 1e6"    'reduce range(1000000) as $i ([]; . + [$i]) | length'
bench "obj update 1e5"      'reduce range(100000) as $i ({}; .["k\($i % 1000)"] += 1) | length'
bench "regex test 1e5"      '[range(100000) | tostring | select(test("^1.*7$"))] | length'
bench "gsub 1e4"            '[range(10000) | "a-b-c-\(.)" | gsub("-"; "+")] | length'
bench "paths/leaf 1e5"      '[range(100000) | {a: {b: [., .]}}] | [paths] | length'
bench "until loop 1e6"      '0 | until(. >= 1000000; . + 1)'
bench "limit/first"         '[limit(100000; repeat(1))] | length'
bench "tojson/fromjson 1e5" '[range(100000) | {a: ., b: "x"} | tojson | fromjson] | length'
bench "count by key 1e6"    'reduce range(1000000) as $i ({}; .["k\($i % 1000)"] += 1) | length'
bench "group into 1e5"      'reduce range(100000) as $i ({}; .["k\($i % 100)"] += [$i]) | length'
