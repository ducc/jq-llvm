# jq-llvm

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

## Sockets (jq-llvm extension)

jq has no network I/O. jq-llvm adds a few low-level socket builtins so protocols
can be written in plain jq on top of them. File descriptors are plain numbers, data
is passed as strings, and failures are ordinary jq errors you can `try`/`catch`.

| builtin | description |
|---|---|
| `socket_connect($host; $port)` | TCP connect (IPv4/IPv6); returns an fd |
| `socket_listen($host; $port)` | bind and listen (`$host` may be `null` for any address); returns an fd |
| `socket_accept($fd)` | accept a connection; returns the client's fd |
| `socket_send($fd; $data)` | send the whole string; returns the byte count |
| `socket_recv($fd; $max)` | receive up to `$max` bytes; `""` at end of stream |
| `socket_shutdown($fd)` | close the sending side |
| `socket_close($fd)` | close the fd |

```jq
socket_connect("example.com"; 80) as $fd
| socket_send($fd; "GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
| socket_recv($fd; 4096) | split("\r\n")[0]
```

These builtins don't exist in jq, so programs that use them only run when compiled with
jq-llvm.

### An HTTP client and server in jq

[`examples/http.jq`](examples/http.jq) is a small HTTP/1.0 client written in jq on top
of those builtins. It handles URL parsing, query strings, headers, JSON bodies and
response parsing. There is no TLS, so it only supports `http://` URLs.

```sh
jq-llvm -L examples -o get 'include "http"; http_get($url) | .status, .headers["content-type"]'
./get -n --arg url http://example.com/

jq-llvm -L examples 'include "http";
  http({method: "POST", url: "http://localhost:8080/items",
        headers: {"X-Token": "abc"}, query: {page: 2}, body: {name: "x"}}) | http_json' --run -n
```

[`examples/http_server.jq`](examples/http_server.jq) is a tiny HTTP server, also in jq,
that answers every request with a JSON description of what it received. `tests/net.sh`
runs the client against it.

### jqpress: an Express-style web framework in jq

[`examples/jqpress.jq`](examples/jqpress.jq) is a web framework written in jq. An app
is a filter from a request to responses. Routes are alternatives joined with `,`, and
the first route that matches wins. Middleware is a filter that wraps another filter:

```jq
include "jqpress";

def app:
    get("/"; html("<h1>hello from jq</h1>"))
  , get("/users/:id"; json({id: .params.id, query: .query}))
  , post("/echo"; json(.json) | status(201))
  , get("/files/*"; text("you asked for \(.params["*"])"))
  , mount("/admin"; bearer_auth("s3cret"; get("/stats"; json({ok: true}))))
  ;

serve(8080; cors(app))
```

- **Request:** `{method, path, query, params, headers, cookies, body, json, state}`.
- **Responses:** `text`, `html`, `json`, `redirect`, `status(code)`, `header(k; v)`
  and `set_cookie`. A handler can also return a plain string, which is sent as
  text/plain, or any other value, which is sent as JSON.
- **Middleware:** `cors`, `bearer_auth`, `catch_errors` and `mount`. Uncaught errors
  become a JSON 500, and unmatched requests a JSON 404.
- **State:** jq has no mutation, so `serve_state($port; $init; app)` threads state
  through the request loop. Handlers read `.state` and return a response with
  `set_state(...)` to replace it.

[`examples/todo_app.jq`](examples/todo_app.jq) is a complete to-do REST API (CRUD, 201s
and 404s, CORS) built this way:

```sh
jq-llvm -f examples/todo_app.jq -o todo && ./todo -n --arg port 8080
curl -s localhost:8080/api/todos -H 'Content-Type: application/json' -d '{"title":"write a web framework in jq"}'
curl -s localhost:8080/api/todos
```

The server handles one connection at a time and speaks HTTP/1.0 with
`Connection: close`, so it's a toy rather than production software.

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
tests/net.sh                      # sockets, jq HTTP client/server, jqpress
tests/bench.sh big.json lines.json
tests/bench_compute.sh
```

`JQC_STRICT=1` makes the IR builder panic if an instruction is emitted after a
terminator. The test scripts set it.
