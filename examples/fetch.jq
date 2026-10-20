# Fetch a URL given with --arg url and print status, content type and body size.
#
#   jq-llvm -L examples -f examples/fetch.jq -o fetch
#   ./fetch -n --arg url http://example.com/
include "http";

http_get($url)
| {status, reason, type: .headers["content-type"], bytes: (.body | utf8bytelength)}
