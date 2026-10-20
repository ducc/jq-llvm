# Regular-expression compiler, written in jq and compiled by jqc into the
# runtime. Produces a flat instruction array executed by the native VM
# (see src/rt/regex.rs for the instruction set).
def _re_ranges_complement:
  sort | reduce .[] as $r ({out: [], next: 0};
    if $r[0] > .next then .out += [[.next, $r[0] - 1]] else . end
    | .next = ([.next, $r[1] + 1] | max))
  | if .next <= 1114111 then .out + [[.next, 1114111]] else .out end;
def _re_class_esc($c):
  if   $c == 100 then [[48,57]]
  elif $c == 68  then [[48,57]] | _re_ranges_complement
  elif $c == 119 then [[48,57],[65,90],[95,95],[97,122]]
  elif $c == 87  then [[48,57],[65,90],[95,95],[97,122]] | _re_ranges_complement
  elif $c == 115 then [[9,13],[32,32]]
  elif $c == 83  then [[9,13],[32,32]] | _re_ranges_complement
  elif $c == 104 then [[48,57],[65,70],[97,102]]
  elif $c == 72  then [[48,57],[65,70],[97,102]] | _re_ranges_complement
  else null end;
def _re_simple_esc($c):
  if $c == 110 then 10 elif $c == 116 then 9 elif $c == 114 then 13 elif $c == 102 then 12
  elif $c == 118 then 11 elif $c == 101 then 27 elif $c == 97 then 7 elif $c == 48 then 0
  else $c end;
def _re_hexval($cs): reduce $cs[] as $c (0; . * 16 + (if $c >= 97 then $c - 87 elif $c >= 65 then $c - 55 else $c - 48 end));
def _re_posix($name):
  if $name == "alpha" then [[65,90],[97,122]] elif $name == "digit" then [[48,57]]
  elif $name == "alnum" then [[48,57],[65,90],[97,122]] elif $name == "upper" then [[65,90]]
  elif $name == "lower" then [[97,122]] elif $name == "space" then [[9,13],[32,32]]
  elif $name == "punct" then [[33,47],[58,64],[91,96],[123,126]] elif $name == "xdigit" then [[48,57],[65,70],[97,102]]
  elif $name == "word" then [[48,57],[65,90],[95,95],[97,122]] elif $name == "blank" then [[9,9],[32,32]]
  elif $name == "cntrl" then [[0,31],[127,127]] elif $name == "print" then [[32,126]]
  elif $name == "graph" then [[33,126]] else error("invalid POSIX bracket type") end;

# Parser. State `.`: {i, ng, names, ic}. Each production outputs [node, state].
def _re_parse($p; $x):
  def at: $p[.i];
  def at1: $p[.i + 1];
  def skipx: if $x then
      def s: (at) as $c | if $c == 32 or $c == 9 or $c == 10 or $c == 13 then .i += 1 | s
        elif $c == 35 then (def c: if at == null or at == 10 then . else .i += 1 | c end; c) | s
        else . end;
      s
    else . end;
  def digits: def d($acc): (at) as $c | if $c != null and $c >= 48 and $c <= 57 then .i += 1 | d($acc * 10 + $c - 48) else [$acc, .] end; d(0);
  def P($m):
    if $m == "alt" then
      P("seq") as [$n, $s]
      | if ($s | at) == 124 then ($s | .i += 1 | P("alt")) as [$n2, $s2] | [{t: "alt", a: $n, b: $n2}, $s2]
        else [$n, $s] end
    elif $m == "seq" then
      def loop($acc): skipx | (at) as $c
        | if $c == null or $c == 124 or $c == 41 then [{t: "cat", l: $acc}, .]
          else P("quant") as [$n, $s] | $s | loop($acc + [$n]) end;
      loop([])
    elif $m == "quant" then
      P("atom") as [$a, $s]
      | $s | skipx
      | def lazy($node): if at == 63 then .i += 1 | [($node | .greedy = false), .]
          elif at == 43 then .i += 1 | [$node, .] else [$node, .] end;
        def more($node): (at) as $c
          | if $c == 42 then .i += 1 | lazy({t: "rep", min: 0, max: -1, greedy: true, e: $node}) as [$n, $s] | $s | more($n)
            elif $c == 43 then .i += 1 | lazy({t: "rep", min: 1, max: -1, greedy: true, e: $node}) as [$n, $s] | $s | more($n)
            elif $c == 63 then .i += 1 | lazy({t: "rep", min: 0, max: 1, greedy: true, e: $node}) as [$n, $s] | $s | more($n)
            elif $c == 123 and (at1 // 0) >= 48 and (at1 // 0) <= 57 then
              . as $save | .i += 1 | digits as [$lo, $s1]
              | $s1 | if at == 125 then .i += 1 | lazy({t: "rep", min: $lo, max: $lo, greedy: true, e: $node}) as [$n, $s] | $s | more($n)
                elif at == 44 then .i += 1
                  | if at == 125 then .i += 1 | lazy({t: "rep", min: $lo, max: -1, greedy: true, e: $node}) as [$n, $s] | $s | more($n)
                    else digits as [$hi, $s2] | $s2
                      | if at == 125 then .i += 1 | lazy({t: "rep", min: $lo, max: $hi, greedy: true, e: $node}) as [$n, $s] | $s | more($n)
                        else $save | [$node, .] end end
                else $save | [$node, .] end
            else [$node, .] end;
        more($a)
    elif $m == "class" then
      # after '['
      (at == 94) as $neg | {neg: $neg} as $h
      | (if $neg then .i += 1 else . end)
      | def item: (at) as $c
          | if $c == 92 then (at1) as $e | .i += 2
              | (_re_class_esc($e)) as $cls
              | if $cls != null then [$cls, ., null]
                elif $e == 120 then [[], (.i += 2), _re_hexval($p[.i:.i + 2])] | [.[0], .[1], .[2]]
                elif $e == 98 then [[], ., 8]
                else [[], ., _re_simple_esc($e)] end
            elif $c == 91 and at1 == 58 then
              ($p[.i + 2:] | index([58, 93])) as $end
              | [_re_posix($p[.i + 2: .i + 2 + $end] | implode), (.i += $end + 4), null]
            else [[], (.i += 1), $c] end;
        def items($acc; $first):
          (at) as $c
          | if $c == null then error("premature end of char-class")
            elif $c == 93 and ($first | not) then [$acc, (.i += 1)]
            else item as [$cls, $s, $ch]
              | if $ch != null and ($s | at) == 45 and ($s | at1) != 93 and ($s | at1) != null then
                  ($s | .i += 1 | item) as [$cls2, $s2, $ch2]
                  | if $ch2 == null then error("invalid range in char-class") else . end
                  | $s2 | items($acc + [[$ch, $ch2]]; false)
                else $s | items($acc + $cls + (if $ch != null then [[$ch, $ch]] else [] end); false) end
            end;
        items([]; true) as [$r, $s]
        | [{t: "class", neg: $h.neg, r: $r, ic: .ic}, $s]
    elif $m == "atom" then
      (at) as $c
      | if $c == 40 then
          .i += 1
          | if at == 63 then
              (at1) as $k
              | if $k == 58 then .i += 2 | P("alt") as [$n, $s] | $s | if at != 41 then error("end pattern with unmatched parenthesis") else . end | .i += 1 | [{t: "group", idx: null, e: $n}, .]
                elif $k == 61 or $k == 33 then .i += 2 | P("alt") as [$n, $s] | $s | .i += 1 | [{t: "look", neg: ($k == 33), e: $n}, .]
                elif $k == 60 and ($p[.i + 2] != 61 and $p[.i + 2] != 33) then
                  ($p[.i + 2:] | index([62])) as $end
                  | ($p[.i + 2: .i + 2 + $end] | implode) as $name
                  | .i += $end + 3 | .ng += 1 | .ng as $idx | .names += [$name]
                  | P("alt") as [$n, $s] | $s | if at != 41 then error("end pattern with unmatched parenthesis") else . end | .i += 1
                  | [{t: "group", idx: $idx, e: $n}, .]
                else
                  # inline flags (?imsx-imsx) or (?imsx-imsx:...)
                  .i += 1
                  | def flags($on): (at) as $f
                      | if $f == 45 then .i += 1 | flags(false)
                        elif $f == 105 then .ic = $on | .i += 1 | flags($on)
                        elif $f == 109 or $f == 115 or $f == 120 then .i += 1 | flags($on)
                        else . end;
                    .ic as $oldic | flags(true)
                    | if at == 41 then .i += 1 | [{t: "cat", l: []}, .]
                      elif at == 58 then .i += 1 | P("alt") as [$n, $s] | $s | .i += 1 | .ic = $oldic | [{t: "group", idx: null, e: $n}, .]
                      else error("undefined group option") end
                end
            else .ng += 1 | .ng as $idx | .names += [null]
              | P("alt") as [$n, $s] | $s | if at != 41 then error("end pattern with unmatched parenthesis") else . end | .i += 1
              | [{t: "group", idx: $idx, e: $n}, .]
            end
        elif $c == 91 then .i += 1 | P("class")
        elif $c == 46 then .i += 1 | [{t: "any"}, .]
        elif $c == 94 then .i += 1 | [{t: "begin"}, .]
        elif $c == 36 then .i += 1 | [{t: "endz"}, .]
        elif $c == 42 or $c == 43 or $c == 63 then error("target of repeat operator is not specified")
        elif $c == 92 then
          (at1) as $e | .i += 2
          | if $e == null then error("end pattern at escape")
            elif _re_class_esc($e) != null then [{t: "class", neg: false, r: _re_class_esc($e), ic: false}, .]
            elif $e == 98 then [{t: "wordb"}, .]
            elif $e == 66 then [{t: "nwordb"}, .]
            elif $e == 65 then [{t: "begin"}, .]
            elif $e == 122 then [{t: "end"}, .]
            elif $e == 90 then [{t: "endz"}, .]
            elif $e == 71 then [{t: "begin"}, .]
            elif $e >= 49 and $e <= 57 then [{t: "backref", k: ($e - 48)}, .]
            elif $e == 120 then [{t: "char", c: _re_hexval($p[.i:.i + 2]), ic: .ic}, (.i += 2)]
            elif $e == 117 then [{t: "char", c: _re_hexval($p[.i:.i + 4]), ic: .ic}, (.i += 4)]
            else [{t: "char", c: _re_simple_esc($e), ic: .ic}, .] end
        else .i += 1 | [{t: "char", c: $c, ic: .ic}, .] end
    else error("bad parser mode") end;
  {i: 0, ng: 0, names: [], ic: false} | P("alt") as [$n, $s]
  | if ($s | at) != null then error("unmatched close parenthesis") else . end
  | {ast: $n, ng: $s.ng, names: $s.names};

def _re_lower: if . >= 65 and . <= 90 then . + 32 elif . >= 192 and . <= 222 and . != 215 then . + 32 else . end;

# Code generation: node -> instruction array, placed at absolute address $pc.
def _re_code($n; $pc):
  if $n.t == "char" then (if $n.ic then [16, ($n.c | _re_lower)] else [1, $n.c] end)
  elif $n.t == "any" then [2]
  elif $n.t == "class" then [if $n.ic then 17 else 4 end, (if $n.neg then 1 else 0 end), ($n.r | length)] + [$n.r[][]]
  elif $n.t == "cat" then reduce $n.l[] as $e ([]; . + _re_code($e; $pc + length))
  elif $n.t == "alt" then
    _re_code($n.a; $pc + 3) as $a
    | ($pc + 3 + ($a | length) + 2) as $lb
    | _re_code($n.b; $lb) as $b
    | [5, $pc + 3, $lb] + $a + [6, $lb + ($b | length)] + $b
  elif $n.t == "group" then
    if $n.idx == null then _re_code($n.e; $pc)
    else [7, 2 * $n.idx] + _re_code($n.e; $pc + 2) + [7, 2 * $n.idx + 1] end
  elif $n.t == "look" then _re_code($n.e; $pc + 3) as $c | [19, (if $n.neg then 1 else 0 end), ($c | length) + 1] + $c + [0]
  elif $n.t == "backref" then [15, $n.k]
  elif $n.t == "begin" then [12]
  elif $n.t == "end" then [13]
  elif $n.t == "endz" then [14]
  elif $n.t == "wordb" then [10]
  elif $n.t == "nwordb" then [11]
  elif $n.t == "rep" then
    # mandatory copies
    (reduce range(0; $n.min) as $_ ([]; . + _re_code($n.e; $pc + length))) as $req
    | ($pc + ($req | length)) as $p1
    | if $n.max == -1 then
        # L: SPLIT body end ; body: MARK L ; e ; PROGRESS L ; JMP L
        _re_code($n.e; $p1 + 5) as $body
        | ($p1 + 5 + ($body | length) + 4) as $end
        | $req + (if $n.greedy then [5, $p1 + 3, $end] else [5, $end, $p1 + 3] end) + [20, $p1] + $body + [21, $p1, 6, $p1]
      else
        # optional copies: SPLIT next end ; e ; ...
        (reduce range(0; $n.max - $n.min) as $_ ([];
            ($p1 + length) as $at | _re_code($n.e; $at + 3) as $c
            | . + (if $n.greedy then [5, $at + 3, -1] else [5, -1, $at + 3] end) + $c)) as $opt
        | ($p1 + ($opt | length)) as $end
        | $req + [$opt[] | if . == -1 then $end else . end]
      end
  else error("bad regex node \($n.t)") end;

def _re_compile($re; $flags):
  if ($re | type) != "string" then error("\($re) (\($re | type)) cannot be matched, as it is not a string") else . end
  | (($flags // "") | explode) as $f
  | if ($f - [103, 105, 120, 110, 112, 115, 108]) | length > 0 then error("\($flags) is not a valid modifier string") else . end
  | _re_parse($re | explode; ($f | any(. == 120))) as $r
  | (if ($f | any(. == 105)) then $r.ast | walk(if type == "object" and has("ic") then .ic = true else . end) else $r.ast end) as $ast
  | ([7, 0] + _re_code($ast; 2)) as $c
  | {code: ($c + [7, 1, 0]), ngroups: $r.ng, names: ([null] + $r.names)};

def _re_prog($re; $flags): _re_cache_get($re; $flags) // (_re_compile($re; $flags) | _re_cache_put($re; $flags; .));
def _match_impl($re; $mode; $test): _re_prog($re; $mode) as $p | _re_run($p; $mode; $test);
