#!/usr/bin/env bash
# CLI conformance: run jq and the jqc-compiled program with the same options,
# input and trailing arguments; compare stdout, stderr and exit status.
set -u
export JQC_STRICT=1
cd "$(dirname "$0")/.."
JQC=${JQC:-./target/release/jq-llvm}
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
printf '{"a":1}\n{"a":2}\n' > "$work/f1.json"
printf '[3]\n' > "$work/f2.json"
printf 'line one\nline two\n' > "$work/raw.txt"
pass=0; fail=0
# case <opts> <filter> <stdin> [extra args...]
case_() {
  local opts=$1 filter=$2 input=$3; shift 3
  local id=$((pass + fail))
  if ! $JQC -O0 -o "$work/p$id" "$filter" 2> "$work/cerr"; then
    echo "COMPILE-FAIL [$opts] $filter: $(head -c 200 "$work/cerr")"; fail=$((fail+1)); return
  fi
  # shellcheck disable=SC2086
  (cd "$work" && printf '%s' "$input" | jq $opts "$filter" "$@" > exp.out 2> exp.err; echo "exit $?" >> exp.out)
  # shellcheck disable=SC2086
  (cd "$work" && printf '%s' "$input" | "./p$id" $opts "$@" > got.out 2> got.err; echo "exit $?" >> got.out)
  if cmp -s "$work/exp.out" "$work/got.out" && cmp -s "$work/exp.err" "$work/got.err"; then
    pass=$((pass+1))
  else
    fail=$((fail+1))
    echo "MISMATCH [$opts] $filter $* <<< $input"
    echo "  jq:  $(tr '\n' '|' < "$work/exp.out") ERR: $(tr '\n' '|' < "$work/exp.err" | head -c 300)"
    echo "  jqc: $(tr '\n' '|' < "$work/got.out") ERR: $(tr '\n' '|' < "$work/got.err" | head -c 300)"
  fi
}
case_ "-r" '.[]' '["a","b",1,[2]]'
case_ "-j" '.[]' '["a","b",1]'
case_ "-a" '.' '"héllo ☃ 😀"'
case_ "-S" '.' '{"b":1,"a":{"d":1,"c":2}}'
case_ "--tab" '.' '{"a":[1,{"b":2}]}'
case_ "--indent 1" '.' '{"a":[1,{"b":2}]}'
case_ "--indent 0" '.' '{"a":[1,{"b":2}]}'
case_ "--indent 7" '.' '{"a":[1]}'
case_ "-c" '.' '{"a" : [1, 2]}'
case_ "-s" '.' '1 2 3'
case_ "-s" 'length' ''
case_ "-R" '.' 'line one
line two'
case_ "-Rs" '.' 'line one
line two
'
case_ "-R" 'length' 'no newline at end'
case_ "-n" '1, 2' 'ignored'
case_ "-n" 'input' '7 8'
case_ "-n" '[inputs]' '7 8'
case_ "-n" '[., input, input]' '7 8'
case_ "-e" '.' 'false'
case_ "-e" '.' 'null'
case_ "-e" '.' '1'
case_ "-e" 'empty' '1'
case_ "-e" '.[]' '[1, false]'
case_ "--exit-status" '.a' '{"a":true}'
case_ "-nr" '"a\tb"' ''
case_ "-rj" '.[]' '["x","y"]'
case_ "--raw-output0" '.[]' '["x","y"]'
case_ "-c" '$x' 'null' --arg x 1
case_ "-c" '[$x, $y]' 'null' --arg x a --argjson y '{"b":[1]}'
case_ "-c" '$ARGS' 'null' --args a b
case_ "-c" '$ARGS' 'null' --jsonargs 1 '{"a":2}' null
case_ "-nc" '$ARGS' '' --arg k v --args p q
case_ "-c" '$f' 'null' --slurpfile f f1.json
case_ "-c" '$r' 'null' --rawfile r raw.txt
case_ "-c" '.a, input_filename' '' f1.json f2.json
case_ "-c" '.' '' nonexist.json f2.json
case_ "-c" '[., input_filename]' '' f2.json f1.json
case_ "-c" 'error("custom")' '1'
case_ "-c" 'error({"a":1})' '1'
case_ "-c" 'error(null)' '1'
case_ "-c" '.a' '1 [2] {"a":3}'
case_ "-c" 'halt_error' '"bye\n"'
case_ "-c" 'halt_error' '{"x":1}'
case_ "-c" 'halt_error(3)' '"three"'
case_ "-c" '"x" | halt' 'null'
case_ "-c" '1, (2 | error), 3' 'null'
case_ "-c" '.[] | (1 / .)' '[1, 0, 2]'
case_ "-c" '.' '[1,2'
case_ "-c" '.' '1 2 {'
case_ "-c" '$ENV | type' 'null'
case_ "-c" 'env | has("HOME")' 'null'
case_ "-c" '$__loc__' 'null'
case_ "-c" 'debug' '[1]'
case_ "-c" 'debug("msg")' '[1]'
case_ "-c" 'stderr' '"x"'
case_ "-c" 'input_line_number' '1
2'
case_ "-c" '[limit(3; inputs)]' '1 2 3 4 5'
case_ "-c" 'try input catch .' '1'
case_ "-sc" '.' '1
2'
case_ "-c" '.' '﻿{"bom":1}'
case_ "-cS" '.' '{"z":{"y":1,"x":2},"a":[{"c":1,"b":2}]}'
case_ "-r" '@csv' '[1,"a",null]'
case_ "-r" '@tsv' '[1,"a\tb"]'
case_ "" '.' '{}'
case_ "" '.' '[]'
case_ "" '.' '[[],{},[{}]]'
case_ "" '.' '"\u0000\u001f "'
case_ "-c --stream" '.' '{"a":[1,{"b":2}],"c":[]} 3 []'
case_ "-nc --stream" '[inputs]' '[1,2]'
case_ "-c --stream -s" '.' '[1,[2]]'
case_ "--indent 8" '.' '1'
case_ "--indent -1" '.' '[1,{"a":2}]'
case_ "--indent 3" '.' '[1,{"a":[]}]'
case_ "-V" '.' ''
case_ "--unbuffered -c" '.' '1 2'
case_ "-c" '.' '' -- f2.json
case_ "-C" '.' '{"a":[1,null,"x",true],"b":{}}'
case_ "-Cc" '.' '{"a":[1,null,"x",true],"b":{}}'
case_ "-M" '.' '{"a":1}'
echo "cli: passed $pass / $((pass + fail))"
