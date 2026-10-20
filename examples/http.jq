# http.jq — a small HTTP/1.0 client written in plain jq, on top of jq-llvm's
# raw socket builtins (socket_connect / socket_send / socket_recv / socket_close).
#
#   include "http";
#   http_get("http://example.com/") | .status
#   http({method: "POST", url: "http://localhost:8080/items",
#         headers: {"X-Token": "abc"}, query: {page: 2}, body: {name: "x"}})
#
# A response is {status, reason, version, headers, body}; header names are
# lower-cased. Non-string request bodies are sent as JSON. Plain http only:
# there is no TLS, so https:// URLs are rejected.

# "http://host:port/path?query" -> {scheme, host, port, path}
# (plain string operations: no regexes, so programs stay quick to compile)
def http_parse_url:
  . as $url
  | (index("://") // error("http: invalid URL: \($url)")) as $i
  | (.[:$i] | ascii_downcase) as $scheme
  | if $scheme != "http" then error("http: unsupported scheme \($scheme) (only http:// is supported, no TLS)") else . end
  | .[$i + 3:] as $rest
  | ([$rest | index("/"), index("?"), index("#")] | map(select(. != null)) | min // ($rest | length)) as $j
  | $rest[:$j] as $auth
  | ($rest[$j:] | .[:(index("#") // length)]) as $path
  | (if ($auth | startswith("["))
     then ($auth | index("]")) as $b | {host: $auth[1:$b], port: ($auth[$b + 1:] | ltrimstr(":"))}
     else ($auth | split(":")) as $p | {host: $p[0], port: ($p[1] // "")} end) as $hp
  | if $hp.host == "" then error("http: invalid URL: \($url)") else . end
  | { scheme: $scheme,
      host: $hp.host,
      port: (if $hp.port == "" then 80 else ($hp.port | tonumber) end),
      path: (if $path == "" then "/" elif ($path | startswith("?")) then "/" + $path else $path end) };

# {a: 1, b: "x y"} -> "a=1&b=x%20y"
def http_query_string:
  to_entries | map("\(.key | @uri)=\(.value | tostring | @uri)") | join("&");

# Request object -> {host, port, request} with the raw request text
def http_build_request:
  . as $req
  | ($req.url | http_parse_url) as $u
  | (($req.method // "GET") | ascii_upcase) as $method
  | ($req.body | if . == null or type == "string" then . else tojson end) as $body
  | ($u.path
     + (if ($req.query // {}) == {} then ""
        else (if ($u.path | contains("?")) then "&" else "?" end) + ($req.query | http_query_string) end)
    ) as $target
  | ({ "Host": (if $u.port == 80 then $u.host else "\($u.host):\($u.port)" end),
       "User-Agent": "jq-http/0.1",
       "Accept": "*/*",
       "Connection": "close" }
     + (if $body == null then {} else {"Content-Length": ($body | utf8bytelength | tostring)} end)
     + (if ($req.body | type) | IN("object", "array", "number", "boolean")
        then {"Content-Type": "application/json"} else {} end)
     + ($req.headers // {})
    ) as $headers
  | { host: $u.host,
      port: $u.port,
      request: ("\($method) \($target) HTTP/1.0\r\n"
                + ([$headers | to_entries[] | "\(.key): \(.value)\r\n"] | join(""))
                + "\r\n"
                + ($body // "")) };

# Everything the peer sends until it closes the connection
def socket_read_all($fd):
  def chunks: socket_recv($fd; 65536) | if . == "" then empty else ., chunks end;
  [chunks] | join("");

# Raw response text -> {status, reason, version, headers, body}
def http_parse_response:
  (index("\r\n\r\n") // error("http: malformed response (no header terminator)")) as $i
  | .[:$i] as $head
  | .[$i + 4:] as $body
  | ($head | split("\r\n")) as $lines
  | ($lines[0] | split(" ")) as $p
  | if ($p[0] | startswith("HTTP/")) and ($p | length) >= 2 then . else error("http: malformed status line: \($lines[0])") end
  | {version: ($p[0] | ltrimstr("HTTP/")), status: $p[1], reason: ($p[2:] | join(" "))} as $sl
  | { status: ($sl.status | tonumber),
      reason: $sl.reason,
      version: $sl.version,
      headers: (reduce ($lines[1:][] | select(length > 0) | index(":") as $c | select($c != null)
                        | {k: .[:$c], v: (.[$c + 1:] | trim)}) as $h
                  ({}; ($h.k | ascii_downcase) as $k
                       | .[$k] = (if has($k) then .[$k] + ", " + $h.v else $h.v end))),
      body: $body };

# Perform a request: {method, url, headers, query, body} -> response
def http($req):
  ($req | http_build_request) as $r
  | socket_connect($r.host; $r.port) as $fd
  | socket_send($fd; $r.request)
  | socket_read_all($fd) as $raw
  | socket_close($fd)
  | $raw | http_parse_response;

def http_get($url): http({method: "GET", url: $url});
def http_post($url; $body): http({method: "POST", url: $url, body: $body});
def http_put($url; $body): http({method: "PUT", url: $url, body: $body});
def http_delete($url): http({method: "DELETE", url: $url});

# Response body parsed as JSON
def http_json: .body | fromjson;
