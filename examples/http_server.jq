# A tiny HTTP server in plain jq (jq-llvm socket builtins). It answers every
# request with a JSON description of the request it received.
#
#   jq-llvm -L examples -f examples/http_server.jq -o server
#   ./server -n --arg port 8080
include "http";

# Read one request: head up to the blank line, then Content-Length bytes of body.
def read_request($c):
  def head($acc):
    if ($acc | contains("\r\n\r\n")) then $acc
    else socket_recv($c; 65536) as $d
         | if $d == "" then $acc else head($acc + $d) end
    end;
  head("") as $raw
  | ($raw | index("\r\n\r\n") // error("bad request")) as $i
  | ($raw[:$i] | split("\r\n")) as $lines
  | ($lines[0] | split(" ")) as $p
  | if ($p | length) == 3 and ($p[2] | startswith("HTTP/")) then . else error("bad request line") end
  | {method: $p[0], target: $p[1], version: ($p[2] | ltrimstr("HTTP/"))} as $rl
  | (reduce ($lines[1:][] | index(":") as $c | select($c != null) | {k: .[:$c], v: (.[$c + 1:] | trim)}) as $h
       ({}; .[$h.k | ascii_downcase] = $h.v)) as $headers
  | (($headers["content-length"] // "0") | tonumber) as $need
  | def body($b):
      if ($b | utf8bytelength) >= $need then $b
      else socket_recv($c; 65536) as $d
           | if $d == "" then $b else body($b + $d) end
      end;
    body($raw[$i + 4:]) as $body
  | ($rl.target | split("?")) as $t
  | { method: $rl.method,
      path: $t[0],
      query: ($t[1] // "" | split("&") | map(select(. != "") | split("=") | {key: .[0], value: (.[1] // "")}) | from_entries),
      headers: $headers,
      body: $body };

def respond($c; $status; $value):
  ($value | tojson) as $b
  | socket_send($c; "HTTP/1.0 \($status) OK\r\nContent-Type: application/json\r\nContent-Length: \($b | utf8bytelength)\r\nConnection: close\r\n\r\n\($b)")
  | socket_close($c);

socket_listen("127.0.0.1"; $ARGS.named.port // "8080") as $l
| repeat(
    socket_accept($l) as $c
    | (try read_request($c) catch {error: .}) as $req
    | respond($c; 200; $req)
    | "\($req.method) \($req.path)"
  )
