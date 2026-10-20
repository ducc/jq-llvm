# Builtins defined in jq, mirroring jq 1.8.1's builtin.jq. Hot or simple
# builtins are native (src/rt/builtins.rs) or special forms in the code
# generator (path, getpath, range, limit, first, error, empty, input, ...).
def halt_error: halt_error(5);
def error(msg): msg | error;
def map(f): [.[] | f];
def select(f): if f then . else empty end;
def sort_by(f): _sort_by_impl(map([f]));
def group_by(f): _group_by_impl(map([f]));
def unique_by(f): _unique_by_impl(map([f]));
def max_by(f): _max_by_impl(map([f]));
def min_by(f): _min_by_impl(map([f]));
def add(f): reduce f as $x (null; . + $x);
def del(f): delpaths([path(f)]);
def abs: if . < 0 then - . else . end;
def map_values(f): .[] |= f;
def recurse(f): def r: ., (f | r); r;
def recurse(f; cond): def r: ., (f | select(cond) | r); r;
def recurse: recurse(.[]?);
def to_entries: [keys_unsorted[] as $k | {key: $k, value: .[$k]}];
def from_entries: map({(.key // .Key // .name // .Name): (if has("value") then .value else .Value end)}) | add | . //= {};
def with_entries(f): to_entries | map(f) | from_entries;
def indices($i): if type == "array" and ($i|type) == "array" then .[$i]
  elif type == "array" then .[[$i]]
  elif type == "string" and ($i|type) == "string" then _strindices($i)
  else .[$i] end;
def index($i): indices($i) | .[0];
def rindex($i): indices($i) | .[-1:][0];
def paths: path(recurse) | select(length > 0);
def paths(node_filter): path(recurse | select(node_filter)) | select(length > 0);
def isfinite: type == "number" and (isinfinite | not);
def arrays: select(type == "array");
def objects: select(type == "object");
def iterables: select(type | . == "array" or . == "object");
def booleans: select(type == "boolean");
def numbers: select(type == "number");
def normals: select(isnormal);
def finites: select(isfinite);
def strings: select(type == "string");
def nulls: select(. == null);
def values: select(. != null);
def scalars: select(type | . != "array" and . != "object");
def _flatten($x): reduce .[] as $i ([]; if $i | type == "array" and $x != 0 then . + ($i | _flatten($x - 1)) else . + [$i] end);
def flatten($x): if $x < 0 then error("flatten depth must not be negative") else _flatten($x) end;
def flatten: _flatten(-1);
def range($x): range(0; $x);
def fromdateiso8601: strptime("%Y-%m-%dT%H:%M:%SZ") | mktime;
def todateiso8601: strftime("%Y-%m-%dT%H:%M:%SZ");
def fromdate: fromdateiso8601;
def todate: todateiso8601;
def trimstr($val): ltrimstr($val) | rtrimstr($val);
def match(re; mode): _match_impl(re; mode; false) | .[];
def match($val): ($val | type) as $vt | if $vt == "string" then match($val; null)
   elif $vt == "array" and ($val | length) > 1 then match($val[0]; $val[1])
   elif $vt == "array" and ($val | length) > 0 then match($val[0]; null)
   else error($vt + " not a string or array") end;
def test(re; mode): _match_impl(re; mode; true);
def test($val): ($val | type) as $vt | if $vt == "string" then test($val; null)
   elif $vt == "array" and ($val | length) > 1 then test($val[0]; $val[1])
   elif $vt == "array" and ($val | length) > 0 then test($val[0]; null)
   else error($vt + " not a string or array") end;
def capture(re; mods): match(re; mods) | reduce (.captures | .[] | select(.name != null) | { (.name): .string }) as $pair ({}; . + $pair);
def capture($val): ($val | type) as $vt | if $vt == "string" then capture($val; null)
   elif $vt == "array" and ($val | length) > 1 then capture($val[0]; $val[1])
   elif $vt == "array" and ($val | length) > 0 then capture($val[0]; null)
   else error($vt + " not a string or array") end;
def scan($re; $flags):
  match($re; "g" + $flags)
    | if (.captures | length > 0)
      then [ .captures | .[] | .string ]
      else .string
      end;
def scan($re): scan($re; null);
def splits($re; $flags):
  .[foreach (match($re; $flags + "g"), null) as {$offset, $length}
      (null; {start: .next, end: $offset, next: ($offset + $length)})];
def splits($re): splits($re; null);
def split($re; $flags): [ splits($re; $flags) ];
def sub($re; s; $flags):
   . as $in
   | (reduce match($re; $flags) as $edit
        ({result: [], previous: 0};
            $in[ .previous: ($edit | .offset) ] as $gap
            | [reduce ( $edit | .captures | .[] | select(.name != null) | { (.name): .string } ) as $pair
                 ({}; . + $pair) | s ] as $inserts
            | reduce range(0; $inserts | length) as $ix (.; .result[$ix] += $gap + $inserts[$ix])
            | .previous = ($edit | .offset + .length ) )
          | .result[] + $in[.previous:] )
      // $in;
def sub($re; s): sub($re; s; "");
def gsub($re; s; flags): sub($re; s; flags + "g");
def gsub($re; s): sub($re; s; "g");
def while(cond; update):
     def _while:
         if cond then ., (update | _while) else empty end;
     _while;
def until(cond; next):
     def _until:
         if cond then . else (next | _until) end;
     _until;
def skip($n; expr):
  if $n > 0 then foreach expr as $item ($n; . - 1; if . < 0 then $item else empty end)
  elif $n == 0 then expr
  else error("skip doesn't support negative count") end;
def isempty(g): first((g | false), true);
def all(generator; condition): isempty(generator | condition and empty);
def any(generator; condition): isempty(generator | condition or empty) | not;
def all(condition): all(.[]; condition);
def any(condition): any(.[]; condition);
def all: all(.[]; .);
def any: any(.[]; .);
def nth($n; g):
  if $n < 0 then error("nth doesn't support negative indices")
  else first(skip($n; g)) end;
def first: .[0];
def last: .[-1];
def nth($n): .[$n];
def combinations:
    if length == 0 then [] else
        .[0][] as $x
          | (.[1:] | combinations) as $y
          | [$x] + $y
    end;
def combinations(n):
    . as $dot
      | [range(n) | $dot]
      | combinations;
def transpose: [range(0; map(length) | max // 0) as $i | [.[][$i]]];
def in(xs): . as $x | xs | has($x);
def inside(xs): . as $x | xs | contains($x);
def repeat(exp):
     def _repeat:
         exp, _repeat;
     _repeat;
def truncate_stream(stream):
  . as $n | null | stream | . as $input | if (.[0] | length) > $n then setpath([0]; $input[0][$n:]) else empty end;
def fromstream(i): {x: null, e: false} as $init
  | foreach i as $i ($init
      ; if .e then $init else . end
      | if $i | length == 2
        then setpath(["e"]; $i[0] | length == 0) | setpath(["x"] + $i[0]; $i[1])
        else setpath(["e"]; $i[0] | length == 1) end
      ; if .e then .x else empty end);
def tostream:
  path(def r: (.[]? | r), .; r) as $p
  | getpath($p)
  | reduce path(.[]?) as $q ([$p, .]; [$p + $q]);
def walk(f):
  def w:
    if type == "object"
    then map_values(w)
    elif type == "array" then map(w)
    else .
    end
    | f;
  w;
def pick(pathexps):
  . as $in
  | reduce path(pathexps) as $a (null;
      setpath($a; $in | getpath($a)) );
def debug(msgs): (msgs | debug | empty), .;
def INDEX(stream; idx_expr):
  reduce stream as $row ({}; .[$row | idx_expr | tostring] = $row);
def INDEX(idx_expr): INDEX(.[]; idx_expr);
def JOIN($idx; idx_expr):
  [.[] | [., $idx[idx_expr]]];
def JOIN($idx; stream; idx_expr):
  stream | [., $idx[idx_expr]];
def JOIN($idx; stream; idx_expr; join_expr):
  stream | [., $idx[idx_expr]] | join_expr;
def IN(s): any(s == .; .);
def IN(src; s): any(src == s; .);
def env: $ENV;
