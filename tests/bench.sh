#!/usr/bin/env bash
# Benchmark jqc-compiled filters against jq. usage: tests/bench.sh <big.json> <lines.json>
set -u
cd "$(dirname "$0")/.."
BIG=$1; LINES=$2
JQC=${JQC:-./target/release/jq-llvm}
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
bench() {
  local name=$1 file=$2 args=$3 filter=$4
  $JQC -O2 -o "$work/p" "$filter" || { echo "$name: compile failed"; return; }
  local t0 t1 t2
  t0=$(date +%s.%N); jq $args "$filter" < "$file" > "$work/a"; t1=$(date +%s.%N)
  "$work/p" $args < "$file" > "$work/b"; t2=$(date +%s.%N)
  local same=same; cmp -s "$work/a" "$work/b" || same=DIFFERENT
  awk -v n="$name" -v a="$t0" -v b="$t1" -v c="$t2" -v s="$same" 'BEGIN { printf "%-26s jq %6.2fs   jqc %6.2fs   speedup %5.1fx  (%s)\n", n, b-a, c-b, (b-a)/(c-b), s }'
}
bench "identity (pretty)"       "$BIG"   ""   '.'
bench "identity (compact)"      "$BIG"   "-c" '.'
bench "field sum"               "$BIG"   ""   'map(.score) | add'
bench "select+count"            "$BIG"   ""   '[.[] | select(.score > 500)] | length'
bench "map object"              "$BIG"   "-c" 'map({id, s: (.score * 2), n: .nested.y})'
bench "group_by"                "$BIG"   "-c" 'group_by(.nested.x) | map({k: .[0].nested.x, n: length})'
bench "sort_by"                 "$BIG"   "-c" 'sort_by(-.score) | .[0:3]'
bench "reduce sum"              "$BIG"   ""   'reduce .[] as $x (0; . + $x.score)'
bench "string interp (lines)"   "$LINES" "-r" '"\(.id): \(.name) [\(.tags | join(","))]"'
bench "per-line select (lines)" "$LINES" "-c" 'select(.nested.x == 3) | {id, name}'
bench "to_entries"              "$BIG"   "-c" '.[0:100000] | map(to_entries | from_entries) | length'
bench "paths count"             "$BIG"   ""   '[paths] | length'
bench "update |="               "$BIG"   "-c" 'map(.score |= . + 1) | length'
bench "tostring/tonumber"       "$BIG"   ""   'map(.id | tostring | tonumber) | add'
bench "range loop"              "$BIG"   ""   '[range(5000000)] | length'
bench "reduce range"            "$BIG"   ""   'reduce range(10000000) as $i (0; . + $i)'
