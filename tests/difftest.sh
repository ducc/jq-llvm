#!/usr/bin/env bash
# Differential test: compile each case with jqc and compare stdout + exit code
# against the reference `jq`.
#
# Case file format (tab separated, one per line; '#' lines are comments):
#   <jq options><TAB><filter><TAB><input JSON (may be empty)>
#
# usage: tests/difftest.sh [cases-file...]   (env: JQC, JQ, JOBS, OPT)
set -u
export JQC_STRICT=1
cd "$(dirname "$0")/.."
JQC=${JQC:-./target/release/jq-llvm}
JQ=${JQ:-jq}
JOBS=${JOBS:-$(nproc)}
OPT=${OPT:--O1}
files=("$@")
[ ${#files[@]} -eq 0 ] && files=(tests/cases/*.txt)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

run_case() {
  local id=$1 args=$2 filter=$3 input=$4
  local d="$work/$id"; mkdir -p "$d"
  printf '%s' "$input" > "$d/in"
  # shellcheck disable=SC2086
  $JQ $args "$filter" < "$d/in" > "$d/exp" 2>/dev/null; echo "exit $?" >> "$d/exp"
  if ! $JQC $OPT -o "$d/prog" "$filter" 2> "$d/cerr"; then
    if grep -q "^exit 3$" "$d/exp"; then echo ok; return; fi
    echo "COMPILE-FAIL [$args] $filter :: $(head -c 300 "$d/cerr")"
    return
  fi
  # shellcheck disable=SC2086
  timeout 10 "$d/prog" $args < "$d/in" > "$d/got" 2>/dev/null; echo "exit $?" >> "$d/got"
  if ! cmp -s "$d/exp" "$d/got"; then
    echo "MISMATCH [$args] $filter  <<< $input"
    echo "   expected: $(head -c 400 "$d/exp" | tr '\n' ' ')"
    echo "   got:      $(head -c 400 "$d/got" | tr '\n' ' ')"
  else
    echo "ok"
  fi
}
export -f run_case
export JQ JQC OPT work

n=$(cat "${files[@]}" | grep -v -e "^#" -e "^$" | wc -l)
i=0
for f in "${files[@]}"; do
  while IFS= read -r line; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    i=$((i+1))
    printf '%s\0' "$i" "$line"
  done < "$f"
done | xargs -0 -n2 -P "$JOBS" bash -c '
  IFS=$'"'"'\t'"'"' read -r args filter input <<< "$1"
  run_case "$0" "$args" "$filter" "$input"' > "$work/results"

pass=$(grep -c '^ok$' "$work/results")
grep -v '^ok$' "$work/results"
echo "passed $pass / $n"
