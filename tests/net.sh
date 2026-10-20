#!/usr/bin/env bash
# Socket builtins + the jq HTTP library: run the jq HTTP server on a local
# port and talk to it with the jq HTTP client.
set -u
cd "$(dirname "$0")/.."
export JQC_STRICT=1
JQC=${JQC:-./target/release/jq-llvm}
PORT=${PORT:-18765}
work=$(mktemp -d); trap 'kill $srv 2>/dev/null; rm -rf "$work"' EXIT
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: expected $3, got $2"; fail=1; fi; }

$JQC -O1 -f examples/http_server.jq -o "$work/server" || exit 1
$JQC -O1 -L examples -o "$work/client" 'include "http";
  http({method: "POST", url: "http://127.0.0.1:\($port)/items?x=1", query: {q: "a b"},
        headers: {"X-Token": "abc"}, body: {name: "héllo"}})
  | [.status, (http_json | .method, .path, .query.x, .headers["x-token"], (.body | fromjson | .name))]' || exit 1
$JQC -O1 -o "$work/echo" 'socket_connect("127.0.0.1"; $port) as $fd
  | socket_send($fd; "GET /e HTTP/1.0\r\n\r\n") | socket_shutdown($fd)
  | [socket_recv($fd; 4096) | split("\r\n")[0]] | socket_close($fd) // .' || exit 1

"$work/server" -n --arg port "$PORT" > /dev/null 2>&1 &
srv=$!
sleep 0.3

check "http client POST" "$("$work/client" -nc --arg port "$PORT")" '[200,"POST","/items","1","abc","héllo"]'
check "raw socket request" "$("$work/echo" -nc --arg port "$PORT")" '["HTTP/1.0 200 OK"]'
# jqpress: the to-do app, driven by the jq HTTP client
TPORT=$((PORT + 1))
$JQC -O1 -f examples/todo_app.jq -o "$work/todo" || exit 1
$JQC -O1 -L examples -o "$work/todo_client" 'include "http";
  "http://127.0.0.1:\($port)" as $b
  | [ (http_get($b + "/hello/jq") | .status, .body),
      (http_post($b + "/api/todos"; {title: "one"}) | .status, (http_json | .id)),
      (http_post($b + "/api/todos"; {title: "two"}) | .status),
      (http({method: "PATCH", url: ($b + "/api/todos/1"), body: {done: true}}) | http_json | .done),
      (http_delete($b + "/api/todos/2") | .status),
      (http_get($b + "/api/todos") | http_json | map(.title)),
      (http_get($b + "/api/todos/2") | .status),
      (http_get($b + "/boom") | .status),
      (http_get($b + "/search?q=a+b") | http_json | .you_searched_for) ]' || exit 1
"$work/todo" -n --arg port "$TPORT" > /dev/null 2>&1 &
todo=$!
sleep 0.3
check "jqpress to-do app" "$("$work/todo_client" -nc --arg port "$TPORT")" '[200,"hello, jq!",201,"1",201,true,204,["one"],404,500,"a b"]'
kill $todo 2>/dev/null

check "connection refused" "$($JQC -O0 'try socket_connect("127.0.0.1"; 1) catch .' --run -n)" '"socket_connect: Connection refused"'
check "bad fd" "$($JQC -O0 'try socket_send(-1; "x") catch .' --run -n)" '"socket file descriptor must be a non-negative integer"'
exit $fail
