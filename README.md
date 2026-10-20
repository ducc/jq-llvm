# jq-llvm

> [!WARNING]
> **This is vibecoded slop. Do not use it for anything real.**
>
> The whole thing was written by an AI (Claude) in a single long chat session, and
> nobody has carefully reviewed it. It passes jq's test suite, but beyond that it's
> an experiment. Expect bugs, memory-safety issues and wrong answers in cases the
> tests don't cover. Don't feed it untrusted input, and don't put it anywhere near
> production. If you want jq, use [jq](https://github.com/jqlang/jq).

**jq-llvm** compiles [jq](https://jqlang.org) programs to native executables via LLVM.
The resulting binary behaves like `jq <filter>`: it takes the usual jq command-line
options, reads JSON from files or stdin and writes the same output, but runs the
filter as optimised machine code. It is typically 5–10× faster than jq on I/O-heavy
work and 10–100× faster on compute-heavy filters.

```sh
$ jq-llvm -o top3 'map(select(.score > 500)) | sort_by(-.score) | .[0:3]'
$ ./top3 -c < data.json
```

The compiler is written in Rust. **There is no Rust (or C) runtime library**: the jq
runtime is generated as LLVM IR by the compiler itself. That covers values, reference
counting, the JSON parser and printer, regular expressions, builtins and the CLI.
Compiled programs depend only on libc and libm.

## Installing

### With Nix (flakes)

```sh
nix run github:ducc/jq-llvm -- '.a | add' --run <<< '{"a":[1,2]}'   # one-off
nix build                       # ./result/bin/jq-llvm, wrapped with clang 19
nix develop                     # dev shell: cargo, clang 19, jq, valgrind, perf
nix flake check                 # builds and runs the differential test suite
```

The packaged `jq-llvm` already knows where its clang lives; nothing else is needed.

### With Cargo

You need a Rust toolchain (edition 2024) and `clang` (LLVM 15 or later for opaque
pointers; tested with 19):

```sh
cargo build --release
export JQC_CLANG=$(which clang)    # or put clang on PATH
./target/release/jq-llvm '.[] | .name' --run < people.json
```

## Usage

```
jq-llvm [options] <filter>
jq-llvm [options] -f <file.jq>

  -o <path>       output executable (default: ./a.out)
  -f <file>       read the filter from a file
  --emit-llvm     write textual LLVM IR instead of an executable
  -O0..-O3        optimisation level passed to clang (default -O2)
  --clang <path>  clang to use (default: $JQC_CLANG, else `clang` on PATH)
  --run ...       compile to a temporary file and run it; the remaining
                  arguments go to the program
```

A compiled program accepts jq's options and file arguments, just like `jq` without
the filter argument:

```
-n -r -j -a -c -s -e -S -R -C -M --tab --indent n --arg --argjson --slurpfile
--rawfile --args --jsonargs --raw-output0 --stream --seq --unbuffered -V --
```

Input is streamed. Values are processed as they arrive, so `tail -f log.json | ./prog`
works, and inputs don't need to fit in memory.

### Libraries

`include "name";` loads `name.jq` from the directory of the `-f` file, from `-L`
directories, or from the current directory. Other module features (`import`,
`modulemeta`) aren't supported.

## Compatibility

The target is the behaviour of jq **1.8.1**. That includes its error messages and exit
codes, and its JSON parse errors (`jq: parse error: … at line L, column C`). Number
literals are preserved: `1.0`, `1E+1000` and large integers print exactly as written
unless they are modified. Also covered:

- path expressions, all assignment operators, `del`/`paths`/`getpath`/`pick`
- `?//` destructuring, `label`/`break`, `limit`/`first`/`last`/`skip`
- regular expressions (`test`, `match`, `capture`, `scan`, `split`, `sub`, `gsub`)
- `@format` strings and date functions
- `$ENV`, `$__loc__`, `input`/`inputs`, `input_filename`, `input_line_number`
- `--stream`, and colour output (`JQ_COLORS`, `NO_COLOR`)

Testing is differential against the real `jq`:

| suite | result |
|---|---|
| jq's own test suite (`jq.test`, `man.test`, `onig.test`, `base64.test`, `optional.test`) + local cases | 1057 / 1061 |
| CLI behaviour (stdout, stderr and exit status) | 79 / 79 |
| valgrind memcheck over the local cases | no errors |

The remaining differences are by design:

- **Modules:** `import` and `modulemeta` are not supported. `include "name";` is.
- **Undefined `$variables`** are a runtime error instead of a compile error, because
  `--arg`/`--argjson` names are only known when the compiled program runs.
- `--seq` affects output only; input is parsed as ordinary JSON.
- Arithmetic uses doubles, as in jq. Preserved literals compare exactly with each other,
  as jq's decNumber build does.

## Performance

Outputs are byte-identical to jq's in every benchmark.

(benchmarks were run while i was playing dota, so take them with a grain of salt)

90 MB input (1M objects), from `tests/bench.sh`:

| filter | jq 1.8.1 | jq-llvm |
|---|---|---|
| `.` (pretty) | 6.25 s | 1.08 s |
| `map(.score) \| add` | 4.94 s | 0.81 s |
| `[.[] \| select(.score > 500)] \| length` | 4.64 s | 0.79 s |
| `group_by(.nested.x) \| map({k: .[0].nested.x, n: length})` | 8.28 s | 1.17 s |
| `sort_by(-.score) \| .[0:3]` | 8.07 s | 1.04 s |
| `"\(.id): \(.name) [\(.tags \| join(","))]"` (per line) | 7.48 s | 0.73 s |
| `[paths] \| length` | 15.32 s | 2.17 s |

No input, from `tests/bench_compute.sh`:

| filter | jq 1.8.1 | jq-llvm |
|---|---|---|
| `reduce range(1e7) as $i (0; . + $i)` | 1.99 s | 0.028 s |
| `[range(1e6) \| select(. % 3 == 0)] \| length` | 0.49 s | 0.006 s |
| `reduce range(1e6) as $i ({}; .["k\($i % 1000)"] += 1)` | 4.74 s | 0.13 s |
| `def fib: …; 30 \| fib` | 0.96 s | 0.01 s |
| `[range(1e5) \| tostring] \| join(",")` | 0.75 s | 0.012 s |
| `[range(1e5) \| tostring \| select(test("^1.*7$"))] \| length` | 0.30 s | 0.04 s |

## How it works

```
filter ──lexer/parser──▶ AST ──code generator (CPS)──▶ LLVM IR ◀── runtime IR
                                                          │        (generated)
                                                 clang -O2▼
                                                  native executable
```

- **Values** are a 16-byte `{tag, bits}` pair passed in registers. Numbers are unboxed
  doubles. Strings, arrays, objects and errors are reference-counted heap objects with
  copy-on-write, and objects keep insertion order with a hash index. A preserved number
  literal hangs off the tag word, so arithmetic stays unboxed.
- **Control flow** is compiled in continuation-passing style at compile time. Generating
  code for an expression takes a continuation describing what to do with each output.
  Generators (`.[]`, `range`, `,`) become plain loops whose bodies are the rest of the
  pipeline. `reduce`/`foreach` become loops over an accumulator. A continuation that is
  needed in several places becomes an LLVM function that takes a pointer to its
  enclosing stack frame.
- **Errors and `break`** are status values. Each emission point knows its handler: return,
  jump to a `catch` block, or exit a `label`. A `try` costs nothing unless something fails.
- **Functions**: non-recursive `def`s are inlined along with their closure arguments.
  Recursive ones become LLVM functions. If analysis shows a function produces at most one
  output, it is compiled in direct style as an ordinary call returning a value.
  Programs run on a thread with a large, lazily committed stack.
- **Path expressions** (`path()`, `|=`, `+=`, `del`, …) compile a variant that tracks
  `(path, value)` pairs, with jq's lazy "Invalid path expression" errors. When a
  `reduce` accumulator is updated through a simple path (`.[$k] += 1`), the update
  happens in place.
- **The runtime** is written against a small IR-builder DSL in Rust (`src/ir.rs`,
  `src/rt/`). Builtins that jq defines in jq come from a prelude mirroring jq 1.8.1's
  `builtin.jq` (`src/prelude.jq`). The regex compiler is jq code itself
  (`src/regex.jq`), compiled by jq-llvm and run on a backtracking VM written in IR.
- **Memory**: free lists for small blocks, interned object keys while parsing, and
  transparent huge pages for the heap. The binary re-execs itself once with glibc
  malloc tunables; set `JQC_NO_TUNE=1` to disable that.

## Development

```sh
nix develop                       # or: install cargo, clang, jq yourself
cargo build --release
tests/difftest.sh                 # differential tests against jq (all suites)
tests/cli.sh                      # CLI options, stderr and exit codes
tests/bench.sh big.json lines.json
tests/bench_compute.sh
```

`JQC_STRICT=1` makes the IR builder panic if an instruction is emitted after a
terminator. The test scripts set it.

## License

MIT, see [LICENSE](LICENSE).

jq-llvm is a reimplementation of [jq](https://github.com/jqlang/jq), by Stephen Dolan
and the jq contributors, and is derived from it throughout: the grammar, semantics,
builtins, error messages and CLI follow jq 1.8.1, and parts of the runtime are ports
of jq's C source. The prelude (`src/prelude.jq`) mirrors jq's `builtin.jq`, and the
`tests/cases/upstream_*.txt` suites are converted from jq's test suite and manual
examples. jq's copyright and license notice (MIT; CC BY 3.0 for its manual) is
included in [LICENSE](LICENSE).
