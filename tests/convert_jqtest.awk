# Convert jq's tests/*.test format into difftest cases: "-c<TAB>program<TAB>input"
# Tests are blocks separated by blank lines: program, input, expected outputs.
# %%FAIL blocks (compile errors) become cases with -n and no input.
function flush() {
  if (prog != "") {
    if (fail) printf "-n\t%s\t\n", prog
    else printf "-c\t%s\t%s\n", prog, input
  }
  prog = ""; input = ""; n = 0; fail = 0
}
BEGIN { prog = ""; n = 0; fail = 0 }
/^#/ { next }
/^[ \t]*$/ { flush(); next }
/^%%FAIL/ { flush(); fail = 1; next }
{
  if (n == 0) prog = $0
  else if (n == 1) input = $0
  n++
}
END { flush() }
