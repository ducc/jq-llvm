# jqpress — an Express-style web framework in plain jq, on top of jq-llvm's
# socket builtins.
#
# An app is a filter from a request to responses. Routes are alternatives
# joined with `,` and the first one that matches wins; middleware are filters
# wrapping filters:
#
#   include "jqpress";
#   def app:
#       get("/"; text("hello from jq"))
#     , get("/users/:id"; json({id: .params.id, q: .query}))
#     , post("/echo"; json(.json))
#     ;
#   serve(8080; cors(app))
#
# Request:  {method, path, query, params, headers, cookies, body, json, state}
# Response: {status, headers, body} — or return a string (text/plain) or any
#           other value (sent as JSON).
#
# Stateful apps use serve_state($port; $init; app): the request carries
# `.state`, and a response may set `.state` to replace it for later requests.

# ---------------------------------------------------------------- responses

def text($s): {status: 200, headers: {"Content-Type": "text/plain; charset=utf-8"}, body: $s};
def html($s): {status: 200, headers: {"Content-Type": "text/html; charset=utf-8"}, body: $s};
def json($v): {status: 200, headers: {"Content-Type": "application/json"}, body: ($v | tojson)};
def status($code): .status = $code;
def header($name; $value): .headers[$name] = $value;
def redirect($url): {status: 302, headers: {"Location": $url}, body: ""};
def set_state($s): .state = $s;
def set_cookie($name; $value):
  .headers["Set-Cookie"] = ([.headers["Set-Cookie"] // empty, "\($name)=\($value | @uri); Path=/"] | join("\r\nSet-Cookie: "));

def not_found: json({error: "not found"}) | status(404);
def bad_request($msg): json({error: $msg}) | status(400);

# ------------------------------------------------------------------ routing

# Match `.path` against "/users/:id/files/*": emits the params object or nothing
def path_params($pattern):
  (.path | ltrimstr("/") | split("/")) as $segs
  | ($pattern | ltrimstr("/") | split("/")) as $pat
  | ($pat | length) as $n
  | if ($n > 0 and $pat[$n - 1] == "*") then ($segs | length) >= $n - 1
    else ($segs | length) == $n end
  | select(.)
  | reduce range(0; $n) as $i ({};
      if . == null then null
      elif $pat[$i] == "*" then .["*"] = ($segs[$i:] | join("/"))
      elif ($pat[$i] | startswith(":")) then .[$pat[$i][1:]] = $segs[$i]
      elif $pat[$i] == $segs[$i] then .
      else null end)
  | select(. != null);

def route($method; $pattern; handler):
  select($method == "*" or .method == $method)
  | . as $req
  | path_params($pattern) as $params
  | $req | .params = $params | handler;

def get($p; h): route("GET"; $p; h);
def post($p; h): route("POST"; $p; h);
def put($p; h): route("PUT"; $p; h);
def patch($p; h): route("PATCH"; $p; h);
def delete($p; h): route("DELETE"; $p; h);

# Mount a sub-app under a path prefix: mount("/api"; api)
def mount($prefix; app):
  select(.path == $prefix or (.path | startswith($prefix + "/")))
  | .path |= (ltrimstr($prefix) | if . == "" then "/" else . end)
  | app;

# --------------------------------------------------------------- middleware

def cors(app):
  if .method == "OPTIONS"
  then {status: 204, headers: {"Access-Control-Allow-Methods": "GET, POST, PUT, PATCH, DELETE, OPTIONS",
                               "Access-Control-Allow-Headers": "Content-Type, Authorization"}, body: ""}
  else app end
  | .headers["Access-Control-Allow-Origin"] = "*";

# Require "Authorization: Bearer <token>"
def bearer_auth($token; app):
  if .headers.authorization == "Bearer \($token)" then app
  else json({error: "unauthorized"}) | status(401) | header("WWW-Authenticate"; "Bearer") end;

# Answer with a JSON 500 when the app raises an error
def catch_errors(app): try app catch (json({error: (if type == "string" then . else tojson end)}) | status(500));

# -------------------------------------------------------------------- server

def _jqpress_decode: split("+") | join(" ") | (try @urid catch .);

def parse_query:
  split("&") | map(select(. != "")
                   | (index("=")) as $i
                   | if $i == null then {key: _jqpress_decode, value: ""}
                     else {key: (.[:$i] | _jqpress_decode), value: (.[$i + 1:] | _jqpress_decode)} end)
  | from_entries;

def parse_cookies:
  split(";") | map(trim | select(. != "") | (index("=")) as $i | select($i != null)
                   | {key: .[:$i], value: (.[$i + 1:] | _jqpress_decode)})
  | from_entries;

# Read one HTTP request from a connection
def read_request($c):
  def head($acc):
    if ($acc | contains("\r\n\r\n")) or ($acc | utf8bytelength) > 1048576 then $acc
    else socket_recv($c; 65536) as $d
         | if $d == "" then $acc else head($acc + $d) end
    end;
  head("") as $raw
  | ($raw | index("\r\n\r\n") // error("incomplete request head")) as $i
  | ($raw[:$i] | split("\r\n")) as $lines
  | ($lines[0] | split(" ")) as $p
  | if ($p | length) == 3 and ($p[2] | startswith("HTTP/")) then . else error("bad request line") end
  | (reduce ($lines[1:][] | index(":") as $k | select($k != null) | {k: .[:$k], v: (.[$k + 1:] | trim)}) as $h
       ({}; .[$h.k | ascii_downcase] = $h.v)) as $headers
  | (($headers["content-length"] // "0") | tonumber) as $need
  | def body($b):
      if ($b | utf8bytelength) >= $need then $b
      else socket_recv($c; 65536) as $d
           | if $d == "" then $b else body($b + $d) end
      end;
    body($raw[$i + 4:]) as $body
  | ($p[1] | index("?")) as $q
  | { method: $p[0],
      path: (if $q == null then $p[1] else $p[1][:$q] end | _jqpress_decode),
      query: (if $q == null then {} else $p[1][$q + 1:] | parse_query end),
      params: {},
      headers: $headers,
      cookies: ($headers.cookie // "" | parse_cookies),
      body: $body,
      json: (if ($headers["content-type"] // "" | startswith("application/json"))
             then (try ($body | fromjson) catch null) else null end) };

def _jqpress_reason:
  {"200": "OK", "201": "Created", "202": "Accepted", "204": "No Content", "301": "Moved Permanently",
   "302": "Found", "304": "Not Modified", "400": "Bad Request", "401": "Unauthorized", "403": "Forbidden",
   "404": "Not Found", "405": "Method Not Allowed", "409": "Conflict", "422": "Unprocessable Entity",
   "500": "Internal Server Error"}[tostring] // "Unknown";

# Turn whatever the app produced into a response object
def _jqpress_response:
  if type == "object" and has("status") and (has("body") or has("headers")) then .
  elif type == "string" then text(.)
  else json(.) end
  | .body |= (. // "" | if type == "string" then . else tojson end)
  | .headers |= (. // {});

def _jqpress_send($c; $res):
  ($res.status // 200) as $st
  | socket_send($c; "HTTP/1.0 \($st) \($st | _jqpress_reason)\r\n"
      + ([$res.headers + {"Content-Length": ($res.body | utf8bytelength | tostring), "Connection": "close"}
          | to_entries[] | "\(.key): \(.value)\r\n"] | join(""))
      + "\r\n" + $res.body);

# Handle one connection: returns {res, req}
def _jqpress_handle($c; $state; app):
  (try read_request($c) catch null) as $req
  | (if $req == null then bad_request("malformed request")
     else first($req | .state = $state | app) // not_found
     end | _jqpress_response) as $res
  | (try _jqpress_send($c; $res | del(.state)) catch null)
  | (try socket_close($c) catch null)
  | {req: $req, res: $res};

def _jqpress_log: "\(.req.method // "-") \(.req.path // "-") \(.res.status)";

# Serve forever with state: requests see `.state`; responses may set `.state`
def serve_state($port; $init; app):
  socket_listen(null; $port) as $l
  | ("jqpress listening on http://localhost:\($port)\n" | stderr | empty),
    foreach (range(0; infinite) | socket_accept($l)) as $c
      ({state: $init};
       .state as $s
       | (try _jqpress_handle($c; $s; catch_errors(app)) catch {req: null, res: {status: 500}}) as $r
       | {state: (if ($r.res | has("state")) then $r.res.state else $s end), log: ($r | _jqpress_log)};
       .log);

# Serve forever; prints one log line per request
def serve($port; app): serve_state($port; null; app);
