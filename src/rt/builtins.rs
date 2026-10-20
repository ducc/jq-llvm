//! Native builtins (runtime side). Every native takes (input, args...) as
//! borrowed values and returns an owned value or an error.

use super::ops::{ret_err1, ret_errc};
use super::*;
use crate::ir::{fconst, Func, Module, V};

/// (jq name, arity, runtime function taking (input, args...))
pub const NATIVES: &[(&str, usize, &str)] = &[
    ("has", 1, "jvb_has"),
    ("contains", 1, "jvb_contains"),
    ("tonumber", 0, "jvb_tonumber"),
    ("utf8bytelength", 0, "jvb_utf8bytelength"),
    ("ascii_downcase", 0, "jvb_ascii_downcase"),
    ("ascii_upcase", 0, "jvb_ascii_upcase"),
    ("explode", 0, "jvb_explode"),
    ("implode", 0, "jvb_implode"),
    ("ltrimstr", 1, "jvb_ltrimstr"),
    ("rtrimstr", 1, "jvb_rtrimstr"),
    ("startswith", 1, "jvb_startswith"),
    ("endswith", 1, "jvb_endswith"),
    ("trim", 0, "jvb_trim"),
    ("ltrim", 0, "jvb_ltrim"),
    ("rtrim", 0, "jvb_rtrim"),
    ("split", 1, "jvb_split"),
    ("join", 1, "jvb_join"),
    ("fromjson", 0, "jvb_fromjson"),
    ("setpath", 2, "jvb_setpath"),
    ("delpaths", 1, "jvb_delpaths"),
    ("_sort_by_impl", 1, "jvb_sort_by_impl"),
    ("_group_by_impl", 1, "jvb_group_by_impl"),
    ("_unique_by_impl", 1, "jvb_unique_by_impl"),
    ("_min_by_impl", 1, "jvb_min_by_impl"),
    ("_max_by_impl", 1, "jvb_max_by_impl"),
    ("_strindices", 1, "jvb_strindices"),
    ("infinite", 0, "jvb_infinite"),
    ("nan", 0, "jvb_nan"),
    ("isinfinite", 0, "jvb_isinfinite"),
    ("isnan", 0, "jvb_isnan"),
    ("isnormal", 0, "jvb_isnormal"),
    ("builtins", 0, "jvb_builtins"),
    ("input_filename", 0, "jvb_input_filename"),
    ("input_line_number", 0, "jvb_input_line_number"),
    ("now", 0, "jvb_now"),
    ("halt", 0, "jvb_halt"),
    ("halt_error", 1, "jvb_halt_error"),
    ("debug", 0, "jvb_debug"),
    ("stderr", 0, "jvb_stderr"),
    ("format", 1, "jvb_format"),
    ("pow", 2, "jvb_pow"),
    ("atan2", 2, "jvb_atan2"),
    ("fmin", 2, "jvb_fmin"),
    ("fmax", 2, "jvb_fmax"),
    ("fmod", 2, "jvb_fmod"),
    ("ldexp", 2, "jvb_ldexp"),
    ("scalb", 2, "jvb_scalb"),
    ("scalbln", 2, "jvb_scalbln"),
    ("nextafter", 2, "jvb_nextafter"),
    ("nexttoward", 2, "jvb_nextafter"),
    ("copysign", 2, "jvb_copysign"),
    ("drem", 2, "jvb_drem"),
    ("hypot", 2, "jvb_hypot"),
    ("fdim", 2, "jvb_fdim"),
    ("remainder", 2, "jvb_drem"),
    ("frexp", 0, "jvb_frexp"),
    ("modf", 0, "jvb_modf"),
    ("tojson", 0, "jv_tojson"),
    ("gmtime", 0, "jvb_gmtime"),
    ("localtime", 0, "jvb_localtime"),
    ("mktime", 0, "jvb_mktime"),
    ("strftime", 1, "jvb_strftime"),
    ("strflocaltime", 1, "jvb_strflocaltime"),
    ("strptime", 1, "jvb_strptime"),
    ("toboolean", 0, "jvb_toboolean"),
    ("bsearch", 1, "jvb_bsearch"),
    ("min", 0, "jvb_min"),
    ("max", 0, "jvb_max"),
    ("unique", 0, "jvb_unique"),
    ("add", 0, "jvb_add"),
    ("reverse", 0, "jvb_reverse"),
    ("_re_cache_get", 2, "jvb_re_cache_get"),
    ("_re_cache_put", 3, "jvb_re_cache_put"),
    ("_re_run", 3, "jvb_re_run"),
    ("ltrimstr", 1, "jvb_ltrimstr"),
    ("tostring", 0, "jv_tostring"),
    ("getpath", 1, "jvb_getpath"),
    ("have_decnum", 0, "jvb_true"),
    ("have_literal_numbers", 0, "jvb_true"),
];

pub fn emit(m: &mut Module) {
    for (n, d) in [
        ("atan2", "declare double @atan2(double, double)"),
        ("fmin", "declare double @fmin(double, double)"),
        ("fmax", "declare double @fmax(double, double)"),
        ("ldexp", "declare double @ldexp(double, i32)"),
        ("scalb", "declare double @scalb(double, double)"),
        ("scalbln", "declare double @scalbln(double, i64)"),
        ("nextafter", "declare double @nextafter(double, double)"),
        ("copysign", "declare double @copysign(double, double)"),
        ("drem", "declare double @remainder(double, double)"),
        ("hypot", "declare double @hypot(double, double)"),
        ("fdim", "declare double @fdim(double, double)"),
        ("frexp", "declare double @frexp(double, ptr)"),
        ("modf", "declare double @modf(double, ptr)"),
        ("time", "declare i64 @time(ptr)"),
        ("clock_gettime", "declare i32 @clock_gettime(i32, ptr)"),
        ("gmtime_r", "declare ptr @gmtime_r(ptr, ptr)"),
        ("localtime_r", "declare ptr @localtime_r(ptr, ptr)"),
        ("timegm", "declare i64 @timegm(ptr)"),
        ("strftime", "declare i64 @strftime(ptr, i64, ptr, ptr)"),
        ("strptime", "declare ptr @strptime(ptr, ptr, ptr)"),
        ("fputs", "declare i32 @fputs(ptr, ptr)"),
    ] {
        m.declare(n, d);
    }
    for f in [
        "floor", "sqrt", "ceil", "round", "trunc", "fabs", "exp", "exp2", "exp10", "log", "log2", "log10", "log1p", "expm1",
        "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "cbrt",
        "significand", "logb", "gamma", "lgamma", "tgamma", "rint", "nearbyint", "j0", "j1", "y0", "y1",
    ] {
        m.declare(f, &format!("declare double @{}(double)", f));
    }
    m.declare("lgamma_r", "declare double @lgamma_r(double, ptr)");
    m.global("@g_label = internal global i64 0".into());
    misc(m);
    path_errors(m);
    paths(m);
    strings(m);
    formats(m);
    math(m);
    collections(m);
    time(m);
    crate::rt::regex::emit(m);
}

fn type_err_unless(m: &mut Module, f: &mut Func, v: &str, tag: i64, msg: &str) {
    let ok = f.tag_is(v, tag);
    let bad = f.not1(&ok);
    let v = v.to_string();
    f.if_then(&bad, |f| ret_err1(m, f, &v, msg));
}

fn misc(m: &mut Module) {
    let mut f = Func::new("internal", "i64", "jv_new_label", &[], "");
    let x = f.load("i64", "@g_label");
    let y = f.add(&x, "1");
    f.store("i64", &y, "@g_label");
    f.ret("i64", &y);
    m.add(f);

    let mut f = Func::new("internal", "void", "jv_immortal", &[(V, "x")], "");
    let t = f.tag("%x");
    let d = f.sub(&t, "4");
    let heap = f.icmp("ult", "i64", &d, "5");
    f.if_then(&heap, |f| {
        let p = f.ptr_of("%x");
        f.store("i64", "1152921504606846976", &p);
    });
    f.ret_void();
    m.add(f);

    let pre = m.cstr("Invalid path expression with result ");
    let mut f = Func::new("internal", V, "jv_path_err", &[(V, "x")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &pre)]);
    f.call("void", "sb_dump_trunc", &[("ptr", &sb), (V, "%x")]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    let p1 = m.cstr("Cannot use ");
    let p2 = m.cstr(" as object key");
    let mut f = Func::new("internal", V, "jv_objkey_err", &[(V, "x")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
    f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%x")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p2)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    let s_start = m.cstr("start");
    let s_end = m.cstr("end");
    let mut f = Func::new("internal", V, "jv_slice_key", &[(V, "a"), (V, "b")], "");
    let o = f.call(V, "jv_obj", &[]);
    let ks = f.call(V, "jv_cstr", &[("ptr", &s_start)]);
    let ke = f.call(V, "jv_cstr", &[("ptr", &s_end)]);
    let o = f.call(V, "jv_obj_set", &[(V, &o), (V, &ks), (V, "%a")]);
    let o = f.call(V, "jv_obj_set", &[(V, &o), (V, &ke), (V, "%b")]);
    f.call("void", "jv_release", &[(V, &ks)]);
    f.call("void", "jv_release", &[(V, &ke)]);
    f.ret(V, &o);
    m.add(f);

    // $ENV
    let mut f = Func::new("internal", V, "jv_env", &[], "");
    let cur = f.load(V, "@g_env");
    let have = f.tag_is(&cur, T_OBJ);
    f.if_then(&have, |f| {
        f.call("void", "jv_retain", &[(V, &cur)]);
        f.ret(V, &cur);
    });
    let o = f.call(V, "jv_obj", &[]);
    let ov = f.var_init(V, &o);
    let env = f.load("ptr", "@environ");
    let iv = f.var_init("i64", "0");
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            let p = f.idx(&env, &i, 8);
            let s = f.load("ptr", &p);
            f.icmp("ne", "ptr", &s, "null")
        },
        |f, _, _| {
            let i = f.load("i64", &iv);
            let p = f.idx(&env, &i, 8);
            let s = f.load("ptr", &p);
            let n = f.call("i64", "strlen", &[("ptr", &s)]);
            let eq = f.call("ptr", "memchr", &[("ptr", &s), ("i32", "61"), ("i64", &n)]);
            let has = f.icmp("ne", "ptr", &eq, "null");
            f.if_then(&has, |f| {
                let si = f.cast("ptrtoint", "ptr", &s, "i64");
                let ei = f.cast("ptrtoint", "ptr", &eq, "i64");
                let kl = f.sub(&ei, &si);
                let k = f.call(V, "jv_str", &[("ptr", &s), ("i64", &kl)]);
                let vp = f.off(&eq, "1");
                let v = f.call(V, "jv_cstr", &[("ptr", &vp)]);
                let o = f.load(V, &ov);
                let o2 = f.call(V, "jv_obj_set", &[(V, &o), (V, &k), (V, &v)]);
                f.store(V, &o2, &ov);
                f.call("void", "jv_release", &[(V, &k)]);
                f.call("void", "jv_release", &[(V, &v)]);
            });
            let i1 = f.add(&i, "1");
            f.store("i64", &i1, &iv);
        },
    );
    let o = f.load(V, &ov);
    f.store(V, &o, "@g_env");
    f.call("void", "jv_retain", &[(V, &o)]);
    f.ret(V, &o);
    m.add(f);

    let s_pos = m.cstr("positional");
    let s_named = m.cstr("named");
    let mut f = Func::new("internal", V, "jv_args", &[], "");
    let o = f.call(V, "jv_obj", &[]);
    let kp = f.call(V, "jv_cstr", &[("ptr", &s_pos)]);
    let kn = f.call(V, "jv_cstr", &[("ptr", &s_named)]);
    let p = f.load(V, "@g_positional");
    let n = f.load(V, "@g_named");
    let o = f.call(V, "jv_obj_set", &[(V, &o), (V, &kp), (V, &p)]);
    let o = f.call(V, "jv_obj_set", &[(V, &o), (V, &kn), (V, &n)]);
    f.call("void", "jv_release", &[(V, &kp)]);
    f.call("void", "jv_release", &[(V, &kn)]);
    f.ret(V, &o);
    m.add(f);

    let undef = m.cstr(" is not defined");
    let mut f = Func::new("internal", V, "jv_named_arg", &[(V, "name")], "");
    let n = f.load(V, "@g_named");
    let v = f.call(V, "jv_obj_get", &[(V, &n), (V, "%name")]);
    let miss = f.tag_is(&v, T_EOF);
    f.if_then(&miss, |f| {
        let sb = f.array_var("i8", 24);
        f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
        f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "36")]);
        f.call("void", "sb_putv", &[("ptr", &sb), (V, "%name")]);
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &undef)]);
        let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
        let e = f.call(V, "jv_err_take", &[(V, &s)]);
        f.ret(V, &e);
    });
    f.call("void", "jv_retain", &[(V, &v)]);
    f.ret(V, &v);
    m.add(f);

    let nomore = m.cstr("break");
    let mut f = Func::new("internal", V, "jv_input", &[], "");
    let v = f.call(V, "jv_next_input", &[]);
    let eof = f.tag_is(&v, T_EOF);
    f.if_then(&eof, |f| {
        let e = f.call(V, "jv_err_c", &[("ptr", &nomore)]);
        f.ret(V, &e);
    });
    f.ret(V, &v);
    m.add(f);

    // trivial natives
    for (name, val) in [("jvb_null", NULL.to_string()), ("jvb_false", FALSE.to_string()), ("jvb_true", TRUE.to_string()), ("jvb_zero", numconst(0.0))] {
        let mut f = Func::new("internal", V, name, &[(V, "x")], "");
        f.ret(V, &val);
        m.add(f);
    }
    let mut f = Func::new("internal", V, "jvb_infinite", &[(V, "x")], "");
    let v = f.mknum(&fconst(f64::INFINITY));
    f.ret(V, &v);
    m.add(f);
    let mut f = Func::new("internal", V, "jvb_nan", &[(V, "x")], "");
    let v = f.mknum(&fconst(f64::NAN));
    f.ret(V, &v);
    m.add(f);

    for (name, kind) in [("jvb_isinfinite", 0), ("jvb_isnan", 1), ("jvb_isnormal", 2)] {
        let mut f = Func::new("internal", V, name, &[(V, "x")], "");
        type_err_unless(m, &mut f, "%x", T_NUM, "number required");
        let d = f.num_of("%x");
        let a = f.call("double", "llvm.fabs.f64", &[("double", &d)]);
        let c = match kind {
            0 => f.fcmp("oeq", &a, &fconst(f64::INFINITY)),
            1 => f.fcmp("uno", &d, &d),
            _ => {
                let x = f.fcmp("oge", &a, &fconst(f64::MIN_POSITIVE));
                let y = f.fcmp("olt", &a, &fconst(f64::INFINITY));
                f.and1(&x, &y)
            }
        };
        let r = f.select(&c, V, TRUE, FALSE);
        f.ret(V, &r);
        m.add(f);
    }

    // halt / halt_error / debug / stderr
    let mut f = Func::new("internal", V, "jvb_halt", &[(V, "x")], "");
    f.call("void", "jq_flush", &[]);
    f.call("void", "exit", &[("i32", "0")]);
    f.unreachable();
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_halt_error", &[(V, "x"), (V, "code")], "");
    type_err_unless(m, &mut f, "%code", T_NUM, "halt_error/1: number required");
    f.call("void", "jq_flush", &[]);
    let e = f.load("ptr", "@stderr");
    let iss = f.tag_is("%x", T_STR);
    f.if_else(
        &iss,
        |f| {
            let d = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
            let n = f.call("i64", "jv_str_len", &[(V, "%x")]);
            f.call("i64", "fwrite", &[("ptr", &d), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
        },
        |f| {
            let s = f.call(V, "jv_tojson", &[(V, "%x")]);
            let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
            let n = f.call("i64", "jv_str_len", &[(V, &s)]);
            f.call("i64", "fwrite", &[("ptr", &d), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
            let nl = m.cstr("\n");
            f.call("i32", "fputs", &[("ptr", &nl), ("ptr", &e)]);
        },
    );
    let c = f.num_of("%code");
    let ci = f.cast("fptosi", "double", &c, "i32");
    f.call("void", "exit", &[("i32", &ci)]);
    f.unreachable();
    m.add(f);

    let dbg = m.cstr("[\"DEBUG:\",");
    let close = m.cstr("]\n");
    let mut f = Func::new("internal", V, "jvb_debug", &[(V, "x")], "");
    let s = f.call(V, "jv_tojson", &[(V, "%x")]);
    let e = f.load("ptr", "@stderr");
    f.call("i32", "fputs", &[("ptr", &dbg), ("ptr", &e)]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("i64", "fwrite", &[("ptr", &d), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
    f.call("i32", "fputs", &[("ptr", &close), ("ptr", &e)]);
    f.call("void", "jv_release", &[(V, &s)]);
    f.call("void", "jv_retain", &[(V, "%x")]);
    f.ret(V, "%x");
    m.add(f);

    // jq 1.8: strings are written raw, other values as compact JSON
    let mut f = Func::new("internal", V, "jvb_stderr", &[(V, "x")], "");
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let e = f.load("ptr", "@stderr");
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("i64", "fwrite", &[("ptr", &d), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
    f.call("void", "jv_release", &[(V, &s)]);
    f.call("void", "jv_retain", &[(V, "%x")]);
    f.ret(V, "%x");
    m.add(f);

    // builtins list
    let names: Vec<String> = BUILTIN_NAMES.split_whitespace().map(|s| format!("\"{}\"", s)).collect();
    let json = format!("[{}]", names.join(","));
    let js = m.cstr(&json);
    let mut f = Func::new("internal", V, "jvb_builtins", &[(V, "x")], "");
    let s = f.call(V, "jv_cstr", &[("ptr", &js)]);
    let v = f.call(V, "jv_fromjson", &[(V, &s)]);
    f.call("void", "jv_release", &[(V, &s)]);
    f.ret(V, &v);
    m.add(f);

    // fromjson with type check
    let mut f = Func::new("internal", V, "jvb_fromjson", &[(V, "x")], "");
    type_err_unless(m, &mut f, "%x", T_STR, "only strings can be parsed");
    let v = f.call(V, "jv_fromjson", &[(V, "%x")]);
    let bad = f.tag_is(&v, T_ERR);
    f.if_then(&bad, |f| {
        // jq: "<msg> (while parsing '<text>')"
        let sb = f.array_var("i8", 24);
        f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
        let msg = f.call(V, "jv_err_msg", &[(V, &v)]);
        f.call("void", "sb_putv", &[("ptr", &sb), (V, &msg)]);
        let w = m.cstr(" (while parsing '");
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &w)]);
        f.call("void", "sb_putv", &[("ptr", &sb), (V, "%x")]);
        let q = m.cstr("')");
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &q)]);
        f.call("void", "jv_release", &[(V, &v)]);
        let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
        let e = f.call(V, "jv_err_take", &[(V, &s)]);
        f.ret(V, &e);
    });
    f.ret(V, &v);
    m.add(f);

    // tonumber
    let mut f = Func::new("internal", V, "jvb_tonumber", &[(V, "x")], "");
    let isn = f.tag_is("%x", T_NUM);
    f.if_then(&isn, |f| f.ret(V, "%x"));
    type_err_unless(m, &mut f, "%x", T_STR, "cannot be parsed as a number");
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%x")]);
    let endp = f.var("ptr");
    let r = f.call("double", "strtod", &[("ptr", &d), ("ptr", &endp)]);
    let e = f.load("ptr", &endp);
    let full = f.off(&d, &n);
    let ok1 = f.icmp("eq", "ptr", &e, &full);
    let nz = f.ne(&n, "0");
    let b0 = f.load("i8", &d);
    let sp = f.icmp("ule", "i8", &b0, "32");
    let nsp = f.not1(&sp);
    let ok2 = f.and1(&ok1, &nz);
    let ok = f.and1(&ok2, &nsp);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| {
        let sb = f.array_var("i8", 24);
        f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
        let p = m.cstr("Cannot parse '");
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p)]);
        f.call("void", "sb_putv", &[("ptr", &sb), (V, "%x")]);
        let q = m.cstr("' as JSON");
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &q)]);
        let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
        let e = f.call(V, "jv_err_take", &[(V, &s)]);
        f.ret(V, &e);
    });
    let v = f.mknum(&r);
    f.ret(V, &v);
    m.add(f);

    let s_true = m.cstr("true");
    let s_false = m.cstr("false");
    let mut f = Func::new("internal", V, "jvb_toboolean", &[(V, "x")], "");
    let t = f.tag("%x");
    let isb = {
        let a = f.eq(&t, "1");
        let b = f.eq(&t, "2");
        f.or1(&a, &b)
    };
    f.if_then(&isb, |f| f.ret(V, "%x"));
    let iss = f.eq(&t, "4");
    f.if_then(&iss, |f| {
        let d = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
        let c = f.call("i32", "strcmp", &[("ptr", &d), ("ptr", &s_true)]);
        let z = f.icmp("eq", "i32", &c, "0");
        f.if_then(&z, |f| f.ret(V, TRUE));
        let c = f.call("i32", "strcmp", &[("ptr", &d), ("ptr", &s_false)]);
        let z = f.icmp("eq", "i32", &c, "0");
        f.if_then(&z, |f| f.ret(V, FALSE));
    });
    ret_err1(m, &mut f, "%x", "cannot be parsed as a boolean");
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_abs", &[(V, "x")], "");
    type_err_unless(m, &mut f, "%x", T_NUM, "has no absolute value");
    let d = f.num_of("%x");
    let neg = f.fcmp("olt", &d, "0.0");
    f.if_then(&neg, |f| {
        let n = f.ins(format!("fneg double {}", d));
        let v = f.mknum(&n);
        f.ret(V, &v);
    });
    f.ret(V, "%x");
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_toarray", &[(V, "x")], "");
    f.call("void", "jv_retain", &[(V, "%x")]);
    let isa = f.tag_is("%x", T_ARR);
    f.if_then(&isa, |f| f.ret(V, "%x"));
    let a = f.call(V, "jv_arr", &[("i64", "1")]);
    let a = f.call(V, "jv_arr_push", &[(V, &a), (V, "%x")]);
    f.call("void", "jv_release", &[(V, "%x")]);
    f.ret(V, &a);
    m.add(f);
}

fn paths(m: &mut Module) {
    // getpath
    let mut f = Func::new("internal", V, "jv_getpath", &[(V, "t"), (V, "p")], "");
    let isarr = f.tag_is("%p", T_ARR);
    let notarr = f.not1(&isarr);
    f.if_then(&notarr, |f| ret_errc(m, f, "Path must be specified as an array"));
    f.call("void", "jv_retain", &[(V, "%t")]);
    let cur = f.var_init(V, "%t");
    let n = f.call("i64", "jv_arr_len", &[(V, "%p")]);
    f.for_range("0", &n, |f, i, _, _| {
        let c = f.load(V, &cur);
        let isnull = f.tag_is(&c, T_NULL);
        f.if_then(&isnull, |f| f.ret(V, NULL));
        let k = f.call(V, "jv_arr_get", &[(V, "%p"), ("i64", i)]);
        let nx = f.call(V, "jv_index", &[(V, &c), (V, &k)]);
        f.call("void", "jv_release", &[(V, &c)]);
        let iserr = f.tag_is(&nx, T_ERR);
        f.if_then(&iserr, |f| f.ret(V, &nx));
        f.store(V, &nx, &cur);
    });
    let c = f.load(V, &cur);
    f.ret(V, &c);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_getpath", &[(V, "t"), (V, "p")], "");
    let r = f.call(V, "jv_getpath", &[(V, "%t"), (V, "%p")]);
    f.ret(V, &r);
    m.add(f);

    // setpath_at(t owned, path, i, x) -> owned
    let neg = m.cstr("Out of bounds negative array index");
    let slicearr = m.cstr("A slice of an array can only be assigned another array");
    let mut f = Func::new("internal", V, "jv_setpath_at", &[(V, "t"), (V, "p"), ("i64", "i"), (V, "x")], "");
    let n = f.call("i64", "jv_arr_len", &[(V, "%p")]);
    let end = f.eq("%i", &n);
    f.if_then(&end, |f| {
        f.call("void", "jv_release", &[(V, "%t")]);
        f.call("void", "jv_retain", &[(V, "%x")]);
        f.ret(V, "%x");
    });
    let key = f.call(V, "jv_arr_get", &[(V, "%p"), ("i64", "%i")]);
    let i1 = f.add("%i", "1");
    let tv = f.var_init(V, "%t");
    let kt = f.tag(&key);
    let tnull = f.tag_is("%t", T_NULL);
    f.if_then(&tnull, |f| {
        let ks = f.eq(&kt, "4");
        f.if_then(&ks, |f| {
            let o = f.call(V, "jv_obj", &[]);
            f.store(V, &o, &tv);
        });
        let kn = f.eq(&kt, "3");
        let ko = f.eq(&kt, "6");
        let ka = f.or1(&kn, &ko);
        f.if_then(&ka, |f| {
            let a = f.call(V, "jv_arr", &[("i64", "4")]);
            f.store(V, &a, &tv);
        });
    });
    let t = f.load(V, &tv);
    let tt = f.tag(&t);
    let tobj = f.eq(&tt, "6");
    let ks = f.eq(&kt, "4");
    let os = f.and1(&tobj, &ks);
    f.if_then(&os, |f| {
        let o = f.call(V, "jv_obj_unique", &[(V, &t)]);
        let idx = f.call("i64", "jv_obj_find", &[(V, &o), (V, &key)]);
        let found = f.icmp("sge", "i64", &idx, "0");
        let child = f.if_val(
            &found,
            V,
            |f| {
                let p = f.ptr_of(&o);
                let ents = f.ld_at("ptr", &p, 24);
                let e = f.idx(&ents, &idx, 32);
                let vp = f.off(&e, "16");
                let c = f.load(V, &vp);
                f.store(V, NULL, &vp);
                c
            },
            |_| NULL.to_string(),
        );
        let nc = f.call(V, "jv_setpath_at", &[(V, &child), (V, "%p"), ("i64", &i1), (V, "%x")]);
        let err = f.tag_is(&nc, T_ERR);
        f.if_then(&err, |f| {
            f.call("void", "jv_release", &[(V, &o)]);
            f.ret(V, &nc);
        });
        let o2 = f.call(V, "jv_obj_set", &[(V, &o), (V, &key), (V, &nc)]);
        f.call("void", "jv_release", &[(V, &nc)]);
        f.ret(V, &o2);
    });
    let tarr = f.eq(&tt, "5");
    let kn = f.eq(&kt, "3");
    let an = f.and1(&tarr, &kn);
    f.if_then(&an, |f| {
        let kd = f.num_of(&key);
        let knan = f.fcmp("uno", &kd, &kd);
        f.if_then(&knan, |f| {
            f.call("void", "jv_release", &[(V, &t)]);
            ret_errc(m, f, "Cannot set array element at NaN index");
        });
        let huge = f.fcmp("oge", &kd, "536870912.0");
        f.if_then(&huge, |f| {
            f.call("void", "jv_release", &[(V, &t)]);
            ret_errc(m, f, "Array index too large");
        });
        let a = f.call(V, "jv_arr_unique", &[(V, &t)]);
        let len = f.call("i64", "jv_arr_len", &[(V, &a)]);
        let kf = f.call("double", "floor", &[("double", &kd)]);
        let i0 = f.call("i64", "jv_dtoi", &[("double", &kf)]);
        let isneg = f.lt(&i0, "0");
        let ia = f.add(&i0, &len);
        let idx = f.select(&isneg, "i64", &ia, &i0);
        let stillneg = f.lt(&idx, "0");
        f.if_then(&stillneg, |f| {
            f.call("void", "jv_release", &[(V, &a)]);
            let e = f.call(V, "jv_err_c", &[("ptr", &neg)]);
            f.ret(V, &e);
        });
        let inside = f.lt(&idx, &len);
        let child = f.if_val(
            &inside,
            V,
            |f| {
                let p = f.ptr_of(&a);
                let items = f.ld_at("ptr", &p, 24);
                let e = f.idx(&items, &idx, 16);
                let c = f.load(V, &e);
                f.store(V, NULL, &e);
                c
            },
            |_| NULL.to_string(),
        );
        let nc = f.call(V, "jv_setpath_at", &[(V, &child), (V, "%p"), ("i64", &i1), (V, "%x")]);
        let err = f.tag_is(&nc, T_ERR);
        f.if_then(&err, |f| {
            f.call("void", "jv_release", &[(V, &a)]);
            f.ret(V, &nc);
        });
        let a2 = f.call(V, "jv_arr_set", &[(V, &a), ("i64", &idx), (V, &nc)]);
        f.call("void", "jv_release", &[(V, &nc)]);
        f.ret(V, &a2);
    });
    let ko = f.eq(&kt, "6");
    let ao = f.and1(&tarr, &ko);
    f.if_then(&ao, |f| {
        // slice assignment
        let sub = f.call(V, "jv_index", &[(V, &t), (V, &key)]);
        let err = f.tag_is(&sub, T_ERR);
        f.if_then(&err, |f| {
            f.call("void", "jv_release", &[(V, &t)]);
            f.ret(V, &sub);
        });
        let nc = f.call(V, "jv_setpath_at", &[(V, &sub), (V, "%p"), ("i64", &i1), (V, "%x")]);
        let err = f.tag_is(&nc, T_ERR);
        f.if_then(&err, |f| {
            f.call("void", "jv_release", &[(V, &t)]);
            f.ret(V, &nc);
        });
        let isa = f.tag_is(&nc, T_ARR);
        let nota = f.not1(&isa);
        f.if_then(&nota, |f| {
            f.call("void", "jv_release", &[(V, &t)]);
            f.call("void", "jv_release", &[(V, &nc)]);
            let e = f.call(V, "jv_err_c", &[("ptr", &slicearr)]);
            f.ret(V, &e);
        });
        let len = f.call("i64", "jv_arr_len", &[(V, &t)]);
        let ps = f.var("i64");
        let pe = f.var("i64");
        let s_start = m.cstr("start");
        let s_end = m.cstr("end");
        let ksv = f.call(V, "jv_cstr", &[("ptr", &s_start)]);
        let kev = f.call(V, "jv_cstr", &[("ptr", &s_end)]);
        let a = f.call(V, "jv_obj_get", &[(V, &key), (V, &ksv)]);
        let b = f.call(V, "jv_obj_get", &[(V, &key), (V, &kev)]);
        f.call("void", "jv_release", &[(V, &ksv)]);
        f.call("void", "jv_release", &[(V, &kev)]);
        let am = f.tag_is(&a, T_EOF);
        let bm = f.tag_is(&b, T_EOF);
        let a2 = f.select(&am, V, NULL, &a);
        let b2 = f.select(&bm, V, NULL, &b);
        f.call("i1", "jv_slice_bounds", &[(V, &a2), (V, &b2), ("i64", &len), ("ptr", &ps), ("ptr", &pe)]);
        let s = f.load("i64", &ps);
        let e = f.load("i64", &pe);
        let left = f.call(V, "jv_arr_sub", &[(V, &t), ("i64", "0"), ("i64", &s)]);
        let right = f.call(V, "jv_arr_sub", &[(V, &t), ("i64", &e), ("i64", &len)]);
        let r1 = f.call(V, "jv_add_c", &[(V, &left), (V, &nc)]);
        let r2 = f.call(V, "jv_add_c", &[(V, &r1), (V, &right)]);
        f.call("void", "jv_release", &[(V, &right)]);
        f.call("void", "jv_release", &[(V, &nc)]);
        f.call("void", "jv_release", &[(V, &t)]);
        f.ret(V, &r2);
    });
    let tstr = f.eq(&tt, "4");
    let sslice = f.and1(&tstr, &ko);
    f.if_then(&sslice, |f| {
        f.call("void", "jv_release", &[(V, &t)]);
        ret_errc(m, f, "Cannot update string slices");
    });
    let karr = f.eq(&kt, "5");
    let aa = f.and1(&tarr, &karr);
    f.if_then(&aa, |f| {
        f.call("void", "jv_release", &[(V, &t)]);
        ret_errc(m, f, "Cannot update field at array index of array");
    });
    let e = f.call(V, "jv_index_err", &[(V, &t), (V, &key)]);
    f.call("void", "jv_release", &[(V, &t)]);
    f.ret(V, &e);
    m.add(f);

    let mut f = Func::new("internal", V, "jv_setpath_c", &[(V, "t"), (V, "p"), (V, "x")], "");
    let isarr = f.tag_is("%p", T_ARR);
    let notarr = f.not1(&isarr);
    f.if_then(&notarr, |f| {
        f.call("void", "jv_release", &[(V, "%t")]);
        ret_errc(m, f, "Path must be specified as an array");
    });
    let r = f.call(V, "jv_setpath_at", &[(V, "%t"), (V, "%p"), ("i64", "0"), (V, "%x")]);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_setpath", &[(V, "t"), (V, "p"), (V, "x")], "");
    f.call("void", "jv_retain", &[(V, "%t")]);
    let r = f.call(V, "jv_setpath_c", &[(V, "%t"), (V, "%p"), (V, "%x")]);
    f.ret(V, &r);
    m.add(f);

    // delpath_at(t owned, path, i) -> owned
    let mut f = Func::new("internal", V, "jv_delpath_at", &[(V, "t"), (V, "p"), ("i64", "i")], "");
    let n = f.call("i64", "jv_arr_len", &[(V, "%p")]);
    let tnull = f.tag_is("%t", T_NULL);
    f.if_then(&tnull, |f| f.ret(V, "%t"));
    let key = f.call(V, "jv_arr_get", &[(V, "%p"), ("i64", "%i")]);
    let kt = f.tag(&key);
    let tt = f.tag("%t");
    let n1 = f.sub(&n, "1");
    let last = f.eq("%i", &n1);
    let i1 = f.add("%i", "1");
    let tobj = f.eq(&tt, "6");
    let tarr = f.eq(&tt, "5");
    let ks = f.eq(&kt, "4");
    let kn = f.eq(&kt, "3");
    let ko = f.eq(&kt, "6");
    let os = f.and1(&tobj, &ks);
    f.if_then(&os, |f| {
        f.if_then(&last, |f| {
            let r = f.call(V, "jv_obj_del", &[(V, "%t"), (V, &key)]);
            f.ret(V, &r);
        });
        let idx = f.call("i64", "jv_obj_find", &[(V, "%t"), (V, &key)]);
        let miss = f.lt(&idx, "0");
        f.if_then(&miss, |f| f.ret(V, "%t"));
        let o = f.call(V, "jv_obj_unique", &[(V, "%t")]);
        let p = f.ptr_of(&o);
        let ents = f.ld_at("ptr", &p, 24);
        let e = f.idx(&ents, &idx, 32);
        let vp = f.off(&e, "16");
        let c = f.load(V, &vp);
        f.store(V, NULL, &vp);
        let nc = f.call(V, "jv_delpath_at", &[(V, &c), (V, "%p"), ("i64", &i1)]);
        let err = f.tag_is(&nc, T_ERR);
        f.if_then(&err, |f| {
            f.call("void", "jv_release", &[(V, &o)]);
            f.ret(V, &nc);
        });
        f.store(V, &nc, &vp);
        f.ret(V, &o);
    });
    let an = f.and1(&tarr, &kn);
    f.if_then(&an, |f| {
        let len = f.call("i64", "jv_arr_len", &[(V, "%t")]);
        let kd = f.num_of(&key);
        let kf = f.call("double", "floor", &[("double", &kd)]);
        let i0 = f.call("i64", "jv_dtoi", &[("double", &kf)]);
        let isneg = f.lt(&i0, "0");
        let ia = f.add(&i0, &len);
        let idx = f.select(&isneg, "i64", &ia, &i0);
        let lo = f.lt(&idx, "0");
        f.if_then(&lo, |f| {
            f.call("void", "jv_release", &[(V, "%t")]);
            ret_errc(m, f, "Out of bounds negative array index");
        });
        let hi = f.icmp("sge", "i64", &idx, &len);
        f.if_then(&hi, |f| f.ret(V, "%t"));
        let a = f.call(V, "jv_arr_unique", &[(V, "%t")]);
        let p = f.ptr_of(&a);
        let items = f.ld_at("ptr", &p, 24);
        let e = f.idx(&items, &idx, 16);
        f.if_then(&last, |f| {
            let old = f.load(V, &e);
            f.call("void", "jv_release", &[(V, &old)]);
            let ix1 = f.add(&idx, "1");
            let src = f.idx(&items, &ix1, 16);
            let rest = f.sub(&len, &ix1);
            let bytes = f.bin("mul", "i64", &rest, "16");
            f.call("ptr", "memmove", &[("ptr", &e), ("ptr", &src), ("i64", &bytes)]);
            let nl = f.sub(&len, "1");
            f.st_at("i64", &nl, &p, 8);
            f.ret(V, &a);
        });
        let c = f.load(V, &e);
        f.store(V, NULL, &e);
        let nc = f.call(V, "jv_delpath_at", &[(V, &c), (V, "%p"), ("i64", &i1)]);
        let err = f.tag_is(&nc, T_ERR);
        f.if_then(&err, |f| {
            f.call("void", "jv_release", &[(V, &a)]);
            f.ret(V, &nc);
        });
        f.store(V, &nc, &e);
        f.ret(V, &a);
    });
    let ao = f.and1(&tarr, &ko);
    let lao = f.and1(&ao, &last);
    f.if_then(&lao, |f| {
        let len = f.call("i64", "jv_arr_len", &[(V, "%t")]);
        let ps = f.var("i64");
        let pe = f.var("i64");
        let s_start = m.cstr("start");
        let s_end = m.cstr("end");
        let ksv = f.call(V, "jv_cstr", &[("ptr", &s_start)]);
        let kev = f.call(V, "jv_cstr", &[("ptr", &s_end)]);
        let a = f.call(V, "jv_obj_get", &[(V, &key), (V, &ksv)]);
        let b = f.call(V, "jv_obj_get", &[(V, &key), (V, &kev)]);
        f.call("void", "jv_release", &[(V, &ksv)]);
        f.call("void", "jv_release", &[(V, &kev)]);
        let am = f.tag_is(&a, T_EOF);
        let bm = f.tag_is(&b, T_EOF);
        let a2 = f.select(&am, V, NULL, &a);
        let b2 = f.select(&bm, V, NULL, &b);
        f.call("i1", "jv_slice_bounds", &[(V, &a2), (V, &b2), ("i64", &len), ("ptr", &ps), ("ptr", &pe)]);
        let s = f.load("i64", &ps);
        let e = f.load("i64", &pe);
        let left = f.call(V, "jv_arr_sub", &[(V, "%t"), ("i64", "0"), ("i64", &s)]);
        let right = f.call(V, "jv_arr_sub", &[(V, "%t"), ("i64", &e), ("i64", &len)]);
        let r = f.call(V, "jv_add_c", &[(V, &left), (V, &right)]);
        f.call("void", "jv_release", &[(V, &right)]);
        f.call("void", "jv_release", &[(V, "%t")]);
        f.ret(V, &r);
    });
    let pre = m.cstr("Cannot delete field at index of ");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &pre)]);
    let kn2 = f.call("ptr", "jv_kind_name", &[("i64", &tt)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &kn2)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.call("void", "jv_release", &[(V, "%t")]);
    f.ret(V, &e);
    m.add(f);

    // delpaths: a port of jq's jv_delpaths / delpaths_sorted / jv_dels.
    // jv_set1(t owned, key, v) -> owned or error
    let mut f = Func::new("internal", V, "jv_set1", &[(V, "t"), (V, "k"), (V, "x")], "");
    let p = f.call(V, "jv_arr", &[("i64", "1")]);
    let p = f.call(V, "jv_arr_push", &[(V, &p), (V, "%k")]);
    let r = f.call(V, "jv_setpath_at", &[(V, "%t"), (V, &p), ("i64", "0"), (V, "%x")]);
    f.call("void", "jv_release", &[(V, &p)]);
    f.ret(V, &r);
    m.add(f);

    // jv_dels(t owned, keys borrowed)
    let mut f = Func::new("internal", V, "jv_dels", &[(V, "t"), (V, "keys")], "");
    let nk = f.call("i64", "jv_arr_len", &[(V, "%keys")]);
    let tt = f.tag("%t");
    let tnull = f.eq(&tt, "0");
    let nokeys = f.eq(&nk, "0");
    let nochange = f.or1(&tnull, &nokeys);
    f.if_then(&nochange, |f| f.ret(V, "%t"));
    let isarr = f.eq(&tt, "5");
    f.if_then(&isarr, |f| {
        let len = f.call("i64", "jv_arr_len", &[(V, "%t")]);
        let mark = f.call("ptr", "calloc", &[("i64", &len), ("i64", "1")]);
        let ps = f.var("i64");
        let pe = f.var("i64");
        f.for_range("0", &nk, |f, i, _, _| {
            let k = f.call(V, "jv_arr_get", &[(V, "%keys"), ("i64", i)]);
            let kt = f.tag(&k);
            let isnum = f.eq(&kt, "3");
            let isobj = f.eq(&kt, "6");
            let (ln, lo, lbad, lnext) = (f.label("dn"), f.label("do"), f.label("dbad"), f.label("dnext"));
            f.cbr(&isnum, &ln, &lo);
            f.block(&ln);
            let d = f.num_of(&k);
            let ti = f.call("i64", "jv_dtoi", &[("double", &d)]);
            let neg = f.lt(&ti, "0");
            let ta = f.add(&ti, &len);
            let idx = f.select(&neg, "i64", &ta, &ti);
            let a = f.icmp("sge", "i64", &idx, "0");
            let b = f.lt(&idx, &len);
            let inr = f.and1(&a, &b);
            f.if_then(&inr, |f| {
                let q = f.off(&mark, &idx);
                f.store("i8", "1", &q);
            });
            f.br(&lnext);
            f.block(&lo);
            f.cbr(&isobj, &lnext, &lbad);
            f.block(&lbad);
            f.call("void", "free", &[("ptr", &mark)]);
            f.call("void", "jv_release", &[(V, "%t")]);
            let sb = f.array_var("i8", 24);
            f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
            let p1 = m.cstr("Cannot delete ");
            let p2 = m.cstr(" element of array");
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
            let kn = f.call("ptr", "jv_kind_name", &[("i64", &kt)]);
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &kn)]);
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p2)]);
            let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
            let e = f.call(V, "jv_err_take", &[(V, &s)]);
            f.ret(V, &e);
            f.block(&lnext);
            // slices
            f.if_then(&isobj, |f| {
                let s_start = m.cstr("start");
                let s_end = m.cstr("end");
                let ksv = f.call(V, "jv_cstr", &[("ptr", &s_start)]);
                let kev = f.call(V, "jv_cstr", &[("ptr", &s_end)]);
                let a = f.call(V, "jv_obj_get", &[(V, &k), (V, &ksv)]);
                let b = f.call(V, "jv_obj_get", &[(V, &k), (V, &kev)]);
                f.call("void", "jv_release", &[(V, &ksv)]);
                f.call("void", "jv_release", &[(V, &kev)]);
                let am = f.tag_is(&a, T_EOF);
                let bm = f.tag_is(&b, T_EOF);
                let a2 = f.select(&am, V, NULL, &a);
                let b2 = f.select(&bm, V, NULL, &b);
                let ok = f.call("i1", "jv_slice_bounds", &[(V, &a2), (V, &b2), ("i64", &len), ("ptr", &ps), ("ptr", &pe)]);
                let bad = f.not1(&ok);
                f.if_then(&bad, |f| {
                    f.call("void", "free", &[("ptr", &mark)]);
                    f.call("void", "jv_release", &[(V, "%t")]);
                    ret_errc(m, f, "Start and end indices of an array slice must be numbers");
                });
                let s = f.load("i64", &ps);
                let e = f.load("i64", &pe);
                f.for_range(&s, &e, |f, j, _, _| {
                    let q = f.off(&mark, j);
                    f.store("i8", "1", &q);
                });
            });
        });
        let out = f.call(V, "jv_arr", &[("i64", &len)]);
        let ov = f.var_init(V, &out);
        f.for_range("0", &len, |f, i, _, _| {
            let q = f.off(&mark, i);
            let mk = f.load("i8", &q);
            let keep = f.icmp("eq", "i8", &mk, "0");
            f.if_then(&keep, |f| {
                let e = f.call(V, "jv_arr_get", &[(V, "%t"), ("i64", i)]);
                let o = f.load(V, &ov);
                let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &e)]);
                f.store(V, &o2, &ov);
            });
        });
        f.call("void", "free", &[("ptr", &mark)]);
        f.call("void", "jv_release", &[(V, "%t")]);
        let o = f.load(V, &ov);
        f.ret(V, &o);
    });
    let isobj = f.eq(&tt, "6");
    f.if_then(&isobj, |f| {
        let tv = f.var_init(V, "%t");
        f.for_range("0", &nk, |f, i, _, _| {
            let k = f.call(V, "jv_arr_get", &[(V, "%keys"), ("i64", i)]);
            let iss = f.tag_is(&k, T_STR);
            let nots = f.not1(&iss);
            f.if_then(&nots, |f| {
                let t = f.load(V, &tv);
                f.call("void", "jv_release", &[(V, &t)]);
                let sb = f.array_var("i8", 24);
                f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
                let p1 = m.cstr("Cannot delete field at object index of ");
                f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
                let kt = f.tag(&k);
                let kn = f.call("ptr", "jv_kind_name", &[("i64", &kt)]);
                f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &kn)]);
                let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
                let e = f.call(V, "jv_err_take", &[(V, &s)]);
                f.ret(V, &e);
            });
            let t = f.load(V, &tv);
            let t2 = f.call(V, "jv_obj_del", &[(V, &t), (V, &k)]);
            f.store(V, &t2, &tv);
        });
        let t = f.load(V, &tv);
        f.ret(V, &t);
    });
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let p1 = m.cstr("Cannot delete fields from ");
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
    let kn = f.call("ptr", "jv_kind_name", &[("i64", &tt)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &kn)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.call("void", "jv_release", &[(V, "%t")]);
    f.ret(V, &e);
    m.add(f);

    // delpaths_sorted(t owned, paths, lo, hi, start)
    let mut f = Func::new(
        "internal",
        V,
        "jv_delpaths_sorted",
        &[(V, "t"), (V, "ps"), ("i64", "lo"), ("i64", "hi"), ("i64", "start")],
        "",
    );
    let tv = f.var_init(V, "%t");
    let dk = f.call(V, "jv_arr", &[("i64", "4")]);
    let dkv = f.var_init(V, &dk);
    let iv = f.var_init("i64", "%lo");
    let fail = |f: &mut Func, e: &str| {
        let d = f.load(V, &dkv);
        f.call("void", "jv_release", &[(V, &d)]);
        f.ret(V, e);
    };
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, "%hi")
        },
        |f, _, _| {
            let i = f.load("i64", &iv);
            let pi = f.call(V, "jv_arr_get", &[(V, "%ps"), ("i64", &i)]);
            let key = f.call(V, "jv_arr_get", &[(V, &pi), ("i64", "%start")]);
            let jv = f.var_init("i64", &i);
            f.while_(
                |f| {
                    let j = f.load("i64", &jv);
                    let inb = f.lt(&j, "%hi");
                    f.if_val(
                        &inb,
                        "i1",
                        |f| {
                            let pj = f.call(V, "jv_arr_get", &[(V, "%ps"), ("i64", &j)]);
                            let kj = f.call(V, "jv_arr_get", &[(V, &pj), ("i64", "%start")]);
                            f.call("i1", "jv_eq", &[(V, &key), (V, &kj)])
                        },
                        |_| "false".into(),
                    )
                },
                |f, _, _| {
                    let j = f.load("i64", &jv);
                    let j1 = f.add(&j, "1");
                    f.store("i64", &j1, &jv);
                },
            );
            let j = f.load("i64", &jv);
            let plen = f.call("i64", "jv_arr_len", &[(V, &pi)]);
            let s1 = f.add("%start", "1");
            let whole = f.eq(&plen, &s1);
            f.if_else(
                &whole,
                |f| {
                    let d = f.load(V, &dkv);
                    let d2 = f.call(V, "jv_arr_push", &[(V, &d), (V, &key)]);
                    f.store(V, &d2, &dkv);
                },
                |f| {
                    let t = f.load(V, &tv);
                    let sub = f.call(V, "jv_index", &[(V, &t), (V, &key)]);
                    let err = f.tag_is(&sub, T_ERR);
                    f.if_then(&err, |f| {
                        f.call("void", "jv_release", &[(V, &t)]);
                        fail(f, &sub);
                    });
                    let snull = f.tag_is(&sub, T_NULL);
                    let nn = f.not1(&snull);
                    f.if_then(&nn, |f| {
                        let ns = f.call(V, "jv_delpaths_sorted", &[(V, &sub), (V, "%ps"), ("i64", &i), ("i64", &j), ("i64", &s1)]);
                        let err = f.tag_is(&ns, T_ERR);
                        f.if_then(&err, |f| {
                            f.call("void", "jv_release", &[(V, &t)]);
                            fail(f, &ns);
                        });
                        let t2 = f.call(V, "jv_set1", &[(V, &t), (V, &key), (V, &ns)]);
                        f.call("void", "jv_release", &[(V, &ns)]);
                        let err = f.tag_is(&t2, T_ERR);
                        f.if_then(&err, |f| fail(f, &t2));
                        f.store(V, &t2, &tv);
                    });
                },
            );
            f.store("i64", &j, &iv);
        },
    );
    let t = f.load(V, &tv);
    let d = f.load(V, &dkv);
    let r = f.call(V, "jv_dels", &[(V, &t), (V, &d)]);
    f.call("void", "jv_release", &[(V, &d)]);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jv_delpaths_c", &[(V, "t"), (V, "ps")], "");
    let isarr = f.tag_is("%ps", T_ARR);
    let notarr = f.not1(&isarr);
    f.if_then(&notarr, |f| {
        f.call("void", "jv_release", &[(V, "%t")]);
        ret_errc(m, f, "Paths must be specified as an array");
    });
    let n = f.call("i64", "jv_arr_len", &[(V, "%ps")]);
    let none = f.eq(&n, "0");
    f.if_then(&none, |f| f.ret(V, "%t"));
    f.for_range("0", &n, |f, i, _, _| {
        let p = f.call(V, "jv_arr_get", &[(V, "%ps"), ("i64", i)]);
        let isa = f.tag_is(&p, T_ARR);
        let nota = f.not1(&isa);
        f.if_then(&nota, |f| {
            f.call("void", "jv_release", &[(V, "%t")]);
            ret_errc(m, f, "Path must be specified as an array");
        });
    });
    let sorted = f.call(V, "jv_sort", &[(V, "%ps")]);
    let p0 = f.call(V, "jv_arr_get", &[(V, &sorted), ("i64", "0")]);
    let l0 = f.call("i64", "jv_arr_len", &[(V, &p0)]);
    let empty = f.eq(&l0, "0");
    f.if_then(&empty, |f| {
        f.call("void", "jv_release", &[(V, &sorted)]);
        f.call("void", "jv_release", &[(V, "%t")]);
        f.ret(V, NULL);
    });
    let r = f.call(V, "jv_delpaths_sorted", &[(V, "%t"), (V, &sorted), ("i64", "0"), ("i64", &n), ("i64", "0")]);
    f.call("void", "jv_release", &[(V, &sorted)]);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_delpaths", &[(V, "t"), (V, "ps")], "");
    f.call("void", "jv_retain", &[(V, "%t")]);
    let r = f.call(V, "jv_delpaths_c", &[(V, "%t"), (V, "%ps")]);
    f.ret(V, &r);
    m.add(f);
}

fn strings(m: &mut Module) {
    // has
    let mut f = Func::new("internal", V, "jvb_has", &[(V, "t"), (V, "k")], "");
    let tt = f.tag("%t");
    let kt = f.tag("%k");
    let to = f.eq(&tt, "6");
    let ks = f.eq(&kt, "4");
    let os = f.and1(&to, &ks);
    f.if_then(&os, |f| {
        let i = f.call("i64", "jv_obj_find", &[(V, "%t"), (V, "%k")]);
        let has = f.icmp("sge", "i64", &i, "0");
        let r = f.select(&has, V, TRUE, FALSE);
        f.ret(V, &r);
    });
    let ta = f.eq(&tt, "5");
    let kn = f.eq(&kt, "3");
    let an = f.and1(&ta, &kn);
    f.if_then(&an, |f| {
        let d = f.num_of("%k");
        let n = f.call("i64", "jv_arr_len", &[(V, "%t")]);
        let nd = f.cast("sitofp", "i64", &n, "double");
        let a = f.fcmp("oge", &d, "0.0");
        let b = f.fcmp("olt", &d, &nd);
        let c = f.and1(&a, &b);
        let r = f.select(&c, V, TRUE, FALSE);
        f.ret(V, &r);
    });
    let p1 = m.cstr("Cannot check whether ");
    let p2 = m.cstr(" has a ");
    let p3 = m.cstr(" key");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
    let a = f.call("ptr", "jv_kind_name", &[("i64", &tt)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &a)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p2)]);
    let b = f.call("ptr", "jv_kind_name", &[("i64", &kt)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &b)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p3)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    // contains (bool)
    let mut f = Func::new("internal", "i1", "jv_contains", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let ao = f.eq(&ta, "6");
    let bo = f.eq(&tb, "6");
    let oo = f.and1(&ao, &bo);
    f.if_then(&oo, |f| {
        let n = f.call("i64", "jv_obj_len", &[(V, "%b")]);
        f.for_range("0", &n, |f, i, _, _| {
            let k = f.call(V, "jv_obj_key_at", &[(V, "%b"), ("i64", i)]);
            let bv = f.call(V, "jv_obj_val_at", &[(V, "%b"), ("i64", i)]);
            let av = f.call(V, "jv_obj_get", &[(V, "%a"), (V, &k)]);
            let miss = f.tag_is(&av, T_EOF);
            f.if_then(&miss, |f| f.ret("i1", "false"));
            let c = f.call("i1", "jv_contains", &[(V, &av), (V, &bv)]);
            let nc = f.not1(&c);
            f.if_then(&nc, |f| f.ret("i1", "false"));
        });
        f.ret("i1", "true");
    });
    let aa = f.eq(&ta, "5");
    let ba = f.eq(&tb, "5");
    let arr = f.and1(&aa, &ba);
    f.if_then(&arr, |f| {
        let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
        let na = f.call("i64", "jv_arr_len", &[(V, "%a")]);
        f.for_range("0", &nb, |f, i, _, _| {
            let bv = f.call(V, "jv_arr_get", &[(V, "%b"), ("i64", i)]);
            let found = f.var_init("i1", "false");
            f.for_range("0", &na, |f, j, brk, _| {
                let av = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", j)]);
                let c = f.call("i1", "jv_contains", &[(V, &av), (V, &bv)]);
                f.if_then(&c, |f| {
                    f.store("i1", "true", &found);
                    f.br(brk);
                });
            });
            let fd = f.load("i1", &found);
            let nf = f.not1(&fd);
            f.if_then(&nf, |f| f.ret("i1", "false"));
        });
        f.ret("i1", "true");
    });
    let as_ = f.eq(&ta, "4");
    let bs = f.eq(&tb, "4");
    let ss = f.and1(&as_, &bs);
    f.if_then(&ss, |f| {
        let r = f.call("i64", "jv_str_find", &[(V, "%a"), (V, "%b"), ("i64", "0")]);
        let c = f.icmp("sge", "i64", &r, "0");
        f.ret("i1", &c);
    });
    let r = f.call("i1", "jv_eq", &[(V, "%a"), (V, "%b")]);
    f.ret("i1", &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_contains", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let same = f.eq(&ta, &tb);
    let ab = {
        let x = f.icmp("ule", "i64", &ta, "2");
        let y = f.icmp("ule", "i64", &tb, "2");
        let xx = f.icmp("uge", "i64", &ta, "1");
        let yy = f.icmp("uge", "i64", &tb, "1");
        let p = f.and1(&x, &y);
        let q = f.and1(&xx, &yy);
        f.and1(&p, &q)
    };
    let ok = f.or1(&same, &ab);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| super::ops::ret_err2(m, f, "%a", "%b", "cannot have their containment checked"));
    let c = f.call("i1", "jv_contains", &[(V, "%a"), (V, "%b")]);
    let r = f.select(&c, V, TRUE, FALSE);
    f.ret(V, &r);
    m.add(f);

    // byte index of needle in haystack starting at byte offset, or -1
    let mut f = Func::new("internal", "i64", "jv_str_find", &[(V, "h"), (V, "n"), ("i64", "from")], "");
    let hd = f.call("ptr", "jv_str_ptr", &[(V, "%h")]);
    let hl = f.call("i64", "jv_str_len", &[(V, "%h")]);
    let nd = f.call("ptr", "jv_str_ptr", &[(V, "%n")]);
    let nl = f.call("i64", "jv_str_len", &[(V, "%n")]);
    let lim0 = f.sub(&hl, &nl);
    let lim = f.add(&lim0, "1");
    f.for_range("%from", &lim, |f, i, _, _| {
        let p = f.off(&hd, i);
        let c = f.call("i32", "memcmp", &[("ptr", &p), ("ptr", &nd), ("i64", &nl)]);
        let z = f.icmp("eq", "i32", &c, "0");
        f.if_then(&z, |f| f.ret("i64", i));
    });
    f.ret("i64", "-1");
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_utf8bytelength", &[(V, "x")], "");
    type_err_unless(m, &mut f, "%x", T_STR, "only strings have UTF-8 byte length");
    let n = f.call("i64", "jv_str_len", &[(V, "%x")]);
    let d = f.cast("sitofp", "i64", &n, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    for (name, lo, hi, delta) in [("jvb_ascii_downcase", 65, 90, 32), ("jvb_ascii_upcase", 97, 122, -32)] {
        let mut f = Func::new("internal", V, name, &[(V, "x")], "");
        type_err_unless(m, &mut f, "%x", T_STR, "cannot be case-converted, as it is not a string");
        let d = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
        let n = f.call("i64", "jv_str_len", &[(V, "%x")]);
        let r = f.call(V, "jv_str", &[("ptr", &d), ("i64", &n)]);
        let rd = f.call("ptr", "jv_str_ptr", &[(V, &r)]);
        f.for_range("0", &n, |f, i, _, _| {
            let p = f.off(&rd, i);
            let b = f.load("i8", &p);
            let a = f.icmp("uge", "i8", &b, &lo.to_string());
            let c = f.icmp("ule", "i8", &b, &hi.to_string());
            let inr = f.and1(&a, &c);
            let b2 = f.bin("add", "i8", &b, &delta.to_string());
            let nb = f.select(&inr, "i8", &b2, &b);
            f.store("i8", &nb, &p);
        });
        f.ret(V, &r);
        m.add(f);
    }

    let mut f = Func::new("internal", V, "jvb_explode", &[(V, "x")], "");
    type_err_unless(m, &mut f, "%x", T_STR, "cannot be exploded, as it is not a string");
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%x")]);
    let a = f.call(V, "jv_arr", &[("i64", &n)]);
    let av = f.var_init(V, &a);
    let iv = f.var_init("i64", "0");
    let lv = f.var("i64");
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, &n)
        },
        |f, _, _| {
            let i = f.load("i64", &iv);
            let cp = f.call("i64", "utf8_decode", &[("ptr", &d), ("i64", &n), ("i64", &i), ("ptr", &lv)]);
            let cd = f.cast("sitofp", "i64", &cp, "double");
            let c = f.mknum(&cd);
            let a = f.load(V, &av);
            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &c)]);
            f.store(V, &a2, &av);
            let l = f.load("i64", &lv);
            let i2 = f.add(&i, &l);
            f.store("i64", &i2, &iv);
        },
    );
    let a = f.load(V, &av);
    f.ret(V, &a);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_implode", &[(V, "x")], "");
    let isarr = f.tag_is("%x", T_ARR);
    let nota = f.not1(&isarr);
    f.if_then(&nota, |f| ret_errc(m, f, "implode input must be an array"));
    let n = f.call("i64", "jv_arr_len", &[(V, "%x")]);
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "16")]);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.call(V, "jv_arr_get", &[(V, "%x"), ("i64", i)]);
        let isn = f.tag_is(&e, T_NUM);
        let ed = f.num_of(&e);
        let enan = f.fcmp("uno", &ed, &ed);
        let notnan = f.not1(&enan);
        let isn = f.and1(&isn, &notnan);
        let notn = f.not1(&isn);
        f.if_then(&notn, |f| {
            let d = f.ld_at("ptr", &sb, 0);
            f.call("void", "free", &[("ptr", &d)]);
            ret_err1(m, f, &e, "can't be imploded, unicode codepoint needs to be numeric");
        });
        let d = f.num_of(&e);
        let c = f.call("i64", "jv_dtoi", &[("double", &d)]);
        let bad1 = f.lt(&c, "0");
        let bad2 = f.icmp("sgt", "i64", &c, "1114111");
        let s1 = f.icmp("sge", "i64", &c, "55296");
        let s2 = f.icmp("slt", "i64", &c, "57344");
        let sur = f.and1(&s1, &s2);
        let b = f.or1(&bad1, &bad2);
        let bad = f.or1(&b, &sur);
        let cp = f.select(&bad, "i64", "65533", &c);
        f.call("void", "sb_utf8", &[("ptr", &sb), ("i64", &cp)]);
    });
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &s);
    m.add(f);

    // ltrimstr / rtrimstr / startswith / endswith / trimstr
    for (name, left, is_test) in [
        ("jvb_ltrimstr", true, false),
        ("jvb_rtrimstr", false, false),
        ("jvb_startswith", true, true),
        ("jvb_endswith", false, true),
    ] {
        let mut f = Func::new("internal", V, name, &[(V, "s"), (V, "p")], "");
        let a = f.tag_is("%s", T_STR);
        let b = f.tag_is("%p", T_STR);
        let ok = f.and1(&a, &b);
        let bad = f.not1(&ok);
        f.if_then(&bad, |f| {
            let msg = if left { "startswith() requires string inputs" } else { "endswith() requires string inputs" };
            ret_errc(m, f, msg);
        });
        let sd = f.call("ptr", "jv_str_ptr", &[(V, "%s")]);
        let sl = f.call("i64", "jv_str_len", &[(V, "%s")]);
        let pd = f.call("ptr", "jv_str_ptr", &[(V, "%p")]);
        let pl = f.call("i64", "jv_str_len", &[(V, "%p")]);
        let fits = f.icmp("sge", "i64", &sl, &pl);
        let start = if left { "0".to_string() } else { f.sub(&sl, &pl) };
        let matched = f.if_val(
            &fits,
            "i1",
            |f| {
                let q = f.off(&sd, &start);
                let c = f.call("i32", "memcmp", &[("ptr", &q), ("ptr", &pd), ("i64", &pl)]);
                f.icmp("eq", "i32", &c, "0")
            },
            |_| "false".into(),
        );
        if is_test {
            let r = f.select(&matched, V, TRUE, FALSE);
            f.ret(V, &r);
        } else {
            let nm = f.not1(&matched);
            f.if_then(&nm, |f| {
                f.call("void", "jv_retain", &[(V, "%s")]);
                f.ret(V, "%s");
            });
            let rest = f.sub(&sl, &pl);
            let from = if left { f.off(&sd, &pl) } else { sd.clone() };
            let r = f.call(V, "jv_str", &[("ptr", &from), ("i64", &rest)]);
            f.ret(V, &r);
        }
        m.add(f);
    }
    let mut f = Func::new("internal", V, "jvb_trimstr", &[(V, "s"), (V, "p")], "");
    let a = f.call(V, "jvb_ltrimstr", &[(V, "%s"), (V, "%p")]);
    let b = f.call(V, "jvb_rtrimstr", &[(V, &a), (V, "%p")]);
    f.call("void", "jv_release", &[(V, &a)]);
    f.ret(V, &b);
    m.add(f);

    // Unicode whitespace as used by jq's trim family
    let mut f = Func::new("internal", "i1", "jv_isspace", &[("i64", "c")], "");
    let a = f.icmp("sge", "i64", "%c", "9");
    let b = f.icmp("sle", "i64", "%c", "13");
    let ctl = f.and1(&a, &b);
    let mut acc = ctl;
    for x in [32i64, 0x85, 0xA0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000] {
        let e = f.eq("%c", &x.to_string());
        acc = f.or1(&acc, &e);
    }
    let a = f.icmp("sge", "i64", "%c", "8192");
    let b = f.icmp("sle", "i64", "%c", "8202");
    let r = f.and1(&a, &b);
    let acc = f.or1(&acc, &r);
    f.ret("i1", &acc);
    m.add(f);

    for (name, l, r) in [("jvb_trim", true, true), ("jvb_ltrim", true, false), ("jvb_rtrim", false, true)] {
        let mut f = Func::new("internal", V, name, &[(V, "s")], "");
        let iss = f.tag_is("%s", T_STR);
        let nots = f.not1(&iss);
        f.if_then(&nots, |f| ret_errc(m, f, "trim input must be a string"));
        let d = f.call("ptr", "jv_str_ptr", &[(V, "%s")]);
        let n = f.call("i64", "jv_str_len", &[(V, "%s")]);
        let st = f.var_init("i64", "0");
        let en = f.var_init("i64", &n);
        let lenv = f.var("i64");
        if l {
            f.while_(
                |f| {
                    let s = f.load("i64", &st);
                    let e = f.load("i64", &en);
                    f.lt(&s, &e)
                },
                |f, brk, _| {
                    let s = f.load("i64", &st);
                    let c = f.call("i64", "utf8_decode", &[("ptr", &d), ("i64", &n), ("i64", &s), ("ptr", &lenv)]);
                    let sp = f.call("i1", "jv_isspace", &[("i64", &c)]);
                    let nsp = f.not1(&sp);
                    f.if_then(&nsp, |f| f.br(brk));
                    let l = f.load("i64", &lenv);
                    let s1 = f.add(&s, &l);
                    f.store("i64", &s1, &st);
                },
            );
        }
        if r {
            f.while_(
                |f| {
                    let s = f.load("i64", &st);
                    let e = f.load("i64", &en);
                    f.lt(&s, &e)
                },
                |f, brk, _| {
                    // step back to the start of the last code point
                    let e = f.load("i64", &en);
                    let pv = f.var_init("i64", &e);
                    f.while_(
                        |f| {
                            let p = f.load("i64", &pv);
                            let p1 = f.sub(&p, "1");
                            let q = f.off(&d, &p1);
                            let b = f.load("i8", &q);
                            let top = f.bin("and", "i8", &b, "-64");
                            let cont = f.icmp("eq", "i8", &top, "-128");
                            let s = f.load("i64", &st);
                            let room = f.icmp("sgt", "i64", &p1, &s);
                            f.and1(&cont, &room)
                        },
                        |f, _, _| {
                            let p = f.load("i64", &pv);
                            let p1 = f.sub(&p, "1");
                            f.store("i64", &p1, &pv);
                        },
                    );
                    let p = f.load("i64", &pv);
                    let start = f.sub(&p, "1");
                    let c = f.call("i64", "utf8_decode", &[("ptr", &d), ("i64", &n), ("i64", &start), ("ptr", &lenv)]);
                    let sp = f.call("i1", "jv_isspace", &[("i64", &c)]);
                    let nsp = f.not1(&sp);
                    f.if_then(&nsp, |f| f.br(brk));
                    f.store("i64", &start, &en);
                },
            );
        }
        let s = f.load("i64", &st);
        let e = f.load("i64", &en);
        let p = f.off(&d, &s);
        let len = f.sub(&e, &s);
        let v = f.call(V, "jv_str", &[("ptr", &p), ("i64", &len)]);
        f.ret(V, &v);
        m.add(f);
    }

    let mut f = Func::new("internal", V, "jvb_split", &[(V, "s"), (V, "sep")], "");
    let a = f.tag_is("%s", T_STR);
    let b = f.tag_is("%sep", T_STR);
    let ok = f.and1(&a, &b);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| ret_errc(m, f, "split input and separator must be strings"));
    let r = f.call(V, "jv_split", &[(V, "%s"), (V, "%sep")]);
    f.ret(V, &r);
    m.add(f);

    // join: jq's `reduce .[] as $i (null; (if .==null then "" else .+$x end) +
    //        ($i | if type=="boolean" or type=="number" then tostring else .//"" end)) // ""`
    let mut f = Func::new("internal", V, "jvb_join", &[(V, "a"), (V, "sep")], "");
    let isarr = f.tag_is("%a", T_ARR);
    let isobj = f.tag_is("%a", T_OBJ);
    let ok = f.or1(&isarr, &isobj);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| {
        let e = f.call(V, "jv_iter_err", &[(V, "%a")]);
        f.ret(V, &e);
    });
    let n = f.if_val(&isarr, "i64", |f| f.call("i64", "jv_arr_len", &[(V, "%a")]), |f| f.call("i64", "jv_obj_len", &[(V, "%a")]));
    let accv = f.var_init(V, NULL);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.if_val(
            &isarr,
            V,
            |f| f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]),
            |f| f.call(V, "jv_obj_val_at", &[(V, "%a"), ("i64", i)]),
        );
        let acc = f.load(V, &accv);
        let first = f.tag_is(&acc, T_NULL);
        let acc2 = f.if_val(
            &first,
            V,
            |f| f.call(V, "jv_str_alloc", &[("i64", "16")]),
            |f| f.call(V, "jv_join_add", &[(V, &acc), (V, "%sep")]),
        );
        let err = f.tag_is(&acc2, T_ERR);
        f.if_then(&err, |f| f.ret(V, &acc2));
        let t = f.tag(&e);
        let isnum = f.eq(&t, "3");
        let isbool = {
            let a = f.eq(&t, "1");
            let b = f.eq(&t, "2");
            f.or1(&a, &b)
        };
        let conv = f.or1(&isnum, &isbool);
        let piece = f.if_val(&conv, V, |f| f.call(V, "jv_tojson", &[(V, &e)]), |f| {
            let isnull = f.tag_is(&e, T_NULL);
            let es = f.call(V, "jv_copy", &[(V, &e)]);
            let empty = f.call(V, "jv_str_alloc", &[("i64", "0")]);
            let r = f.select(&isnull, V, &empty, &es);
            let other = f.select(&isnull, V, &es, &empty);
            f.call("void", "jv_release", &[(V, &other)]);
            r
        });
        let acc3 = f.call(V, "jv_join_add", &[(V, &acc2), (V, &piece)]);
        f.call("void", "jv_release", &[(V, &piece)]);
        let err = f.tag_is(&acc3, T_ERR);
        f.if_then(&err, |f| f.ret(V, &acc3));
        f.store(V, &acc3, &accv);
    });
    let acc = f.load(V, &accv);
    let isnull = f.tag_is(&acc, T_NULL);
    f.if_then(&isnull, |f| {
        let s = f.call(V, "jv_str_alloc", &[("i64", "0")]);
        f.ret(V, &s);
    });
    f.ret(V, &acc);
    m.add(f);

    // acc (owned string) + x: append strings in place, defer to jv_add otherwise
    let mut f = Func::new("internal", V, "jv_join_add", &[(V, "acc"), (V, "x")], "");
    let iss = f.tag_is("%x", T_STR);
    f.if_then(&iss, |f| {
        let r = f.call(V, "jv_str_append_v", &[(V, "%acc"), (V, "%x")]);
        f.ret(V, &r);
    });
    let r = f.call(V, "jv_add", &[(V, "%acc"), (V, "%x")]);
    f.call("void", "jv_release", &[(V, "%acc")]);
    f.ret(V, &r);
    m.add(f);

    // _strindices: byte-offset based in jq 1.7 (code point offsets in 1.8)
    let mut f = Func::new("internal", V, "jvb_strindices", &[(V, "s"), (V, "n")], "");
    let a = f.tag_is("%s", T_STR);
    let b = f.tag_is("%n", T_STR);
    let ok = f.and1(&a, &b);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| ret_errc(m, f, "Cannot determine string indices of non-strings"));
    let r = f.call(V, "jv_arr", &[("i64", "4")]);
    let rv = f.var_init(V, &r);
    let nl = f.call("i64", "jv_str_len", &[(V, "%n")]);
    let empty = f.eq(&nl, "0");
    f.if_then(&empty, |f| f.ret(V, &r));
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%s")]);
    let sl = f.call("i64", "jv_str_len", &[(V, "%s")]);
    let from = f.var_init("i64", "0");
    f.while_(
        |_| "true".into(),
        |f, brk, _| {
            let st = f.load("i64", &from);
            let i = f.call("i64", "jv_str_find", &[(V, "%s"), (V, "%n"), ("i64", &st)]);
            let none = f.lt(&i, "0");
            f.if_then(&none, |f| f.br(brk));
            let cp = f.call("i64", "utf8_count", &[("ptr", &d), ("i64", &i)]);
            let cd = f.cast("sitofp", "i64", &cp, "double");
            let x = f.mknum(&cd);
            let a = f.load(V, &rv);
            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &x)]);
            f.store(V, &a2, &rv);
            let i1 = f.add(&i, "1");
            f.store("i64", &i1, &from);
            let _ = &sl;
        },
    );
    let r = f.load(V, &rv);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_ascii", &[(V, "x")], "");
    let d = f.num_of("%x");
    let c = f.cast("fptosi", "double", &d, "i8");
    let buf = f.array_var("i8", 1);
    f.store("i8", &c, &buf);
    let s = f.call(V, "jv_str", &[("ptr", &buf), ("i64", "1")]);
    f.ret(V, &s);
    m.add(f);
}

fn formats(m: &mut Module) {
    let names = ["text", "json", "html", "uri", "csv", "tsv", "sh", "base64", "base64d", "base32", "urid"];
    let gs: Vec<String> = names.iter().map(|n| m.cstr(n)).collect();
    let b64 = m.cstr("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/");
    let b32 = m.cstr("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567");
    let hex = m.cstr("0123456789ABCDEF");
    let not_valid = m.cstr(" is not a valid format");

    let mut f = Func::new("internal", V, "jv_format", &[(V, "name"), (V, "x")], "");
    let nd = f.call("ptr", "jv_str_ptr", &[(V, "%name")]);
    let sb = f.array_var("i8", 24);
    let labels: Vec<String> = names.iter().map(|n| f.label(&format!("fmt_{}", n))).collect();
    for (i, g) in gs.iter().enumerate() {
        let c = f.call("i32", "strcmp", &[("ptr", &nd), ("ptr", g)]);
        let z = f.icmp("eq", "i32", &c, "0");
        let next = f.label("fmt_next");
        f.cbr(&z, &labels[i], &next);
        f.block(&next);
    }
    // unknown
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "32")]);
    f.call("void", "sb_putv", &[("ptr", &sb), (V, "%name")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &not_valid)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);

    // text
    f.block(&labels[0]);
    let r = f.call(V, "jv_tostring", &[(V, "%x")]);
    f.ret(V, &r);
    // json
    f.block(&labels[1]);
    let r = f.call(V, "jv_tojson", &[(V, "%x")]);
    f.ret(V, &r);

    // html
    f.block(&labels[2]);
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "32")]);
    let reps: [(u8, &str); 5] = [(b'<', "&lt;"), (b'>', "&gt;"), (b'&', "&amp;"), (b'\'', "&apos;"), (b'"', "&quot;")];
    let repg: Vec<(u8, String)> = reps.iter().map(|(c, r)| (*c, m.cstr(r))).collect();
    f.for_range("0", &n, |f, i, _, cont| {
        let p = f.off(&d, i);
        let b = f.load("i8", &p);
        for (c, g) in &repg {
            let is = f.icmp("eq", "i8", &b, &(*c as i64).to_string());
            f.if_then(&is, |f| {
                f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", g)]);
                f.br(cont);
            });
        }
        f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &b)]);
    });
    f.call("void", "jv_release", &[(V, &s)]);
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);

    // uri
    f.block(&labels[3]);
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "32")]);
    f.for_range("0", &n, |f, i, _, _| {
        let p = f.off(&d, i);
        let b = f.load("i8", &p);
        let isalnum = {
            let a1 = f.icmp("uge", "i8", &b, "48");
            let a2 = f.icmp("ule", "i8", &b, "57");
            let d1 = f.and1(&a1, &a2);
            let lc = f.bin("or", "i8", &b, "32");
            let b1 = f.icmp("uge", "i8", &lc, "97");
            let b2 = f.icmp("ule", "i8", &lc, "122");
            let l1 = f.and1(&b1, &b2);
            f.or1(&d1, &l1)
        };
        let m1 = f.icmp("eq", "i8", &b, "45");
        let m2 = f.icmp("eq", "i8", &b, "95");
        let m3 = f.icmp("eq", "i8", &b, "46");
        let m4 = f.icmp("eq", "i8", &b, "126");
        let x = f.or1(&m1, &m2);
        let y = f.or1(&m3, &m4);
        let z = f.or1(&x, &y);
        let unres = f.or1(&isalnum, &z);
        f.if_else(
            &unres,
            |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &b)]),
            |f| {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "37")]);
                let bi = f.cast("zext", "i8", &b, "i64");
                let hi = f.bin("lshr", "i64", &bi, "4");
                let lo = f.bin("and", "i64", &bi, "15");
                let hp = f.off(&hex, &hi);
                let hc = f.load("i8", &hp);
                let lp = f.off(&hex, &lo);
                let lc = f.load("i8", &lp);
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &hc)]);
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &lc)]);
                String::new()
            },
        );
    });
    f.call("void", "jv_release", &[(V, &s)]);
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);

    // csv / tsv
    for (li, csv) in [(4usize, true), (5usize, false)] {
        f.block(&labels[li]);
        let isa = f.tag_is("%x", T_ARR);
        let nota = f.not1(&isa);
        f.if_then(&nota, |f| {
            ret_err1(m, f, "%x", if csv { "cannot be csv-formatted, only an array can be" } else { "cannot be tsv-formatted, only an array can be" })
        });
        let n = f.call("i64", "jv_arr_len", &[(V, "%x")]);
        f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
        f.for_range("0", &n, |f, i, _, _| {
            let nf = f.ne(i, "0");
            f.if_then(&nf, |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", if csv { "44" } else { "9" })]));
            let e = f.call(V, "jv_arr_get", &[(V, "%x"), ("i64", i)]);
            let t = f.tag(&e);
            let (ln, lb, ls, lnull, lbad, ldone) =
                (f.label("cn"), f.label("cb"), f.label("cs"), f.label("cnull"), f.label("cbad"), f.label("cdone"));
            f.switch(&t, &lbad, &[(0, lnull.clone()), (1, lb.clone()), (2, lb.clone()), (3, ln.clone()), (4, ls.clone())]);
            f.block(&ln);
            let d = f.num_of(&e);
            f.call("void", "jv_dump_num", &[("ptr", &sb), ("double", &d)]);
            f.br(&ldone);
            f.block(&lb);
            f.call("void", "jv_dump", &[("ptr", &sb), (V, &e), ("i64", "0"), ("i64", "0")]);
            f.br(&ldone);
            f.block(&lnull);
            f.br(&ldone);
            f.block(&ls);
            let d = f.call("ptr", "jv_str_ptr", &[(V, &e)]);
            let sl = f.call("i64", "jv_str_len", &[(V, &e)]);
            if csv {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "34")]);
            }
            f.for_range("0", &sl, |f, j, _, cont| {
                let p = f.off(&d, j);
                let b = f.load("i8", &p);
                if csv {
                    let q = f.icmp("eq", "i8", &b, "34");
                    f.if_then(&q, |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "34")]));
                } else {
                    for (c, r) in [(9u8, b't'), (10, b'n'), (13, b'r'), (92, b'\\')] {
                        let is = f.icmp("eq", "i8", &b, &(c as i64).to_string());
                        f.if_then(&is, |f| {
                            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "92")]);
                            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &(r as i64).to_string())]);
                            f.br(cont);
                        });
                    }
                }
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &b)]);
            });
            if csv {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "34")]);
            }
            f.br(&ldone);
            f.block(&lbad);
            let d = f.ld_at("ptr", &sb, 0);
            f.call("void", "free", &[("ptr", &d)]);
            ret_err1(m, f, &e, if csv { "is not valid in a csv row" } else { "is not valid in a tsv row" });
            f.block(&ldone);
        });
        let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
        f.ret(V, &r);
    }

    // sh
    f.block(&labels[6]);
    let isa = f.tag_is("%x", T_ARR);
    let arr = f.if_val(
        &isa,
        V,
        |f| {
            f.call("void", "jv_retain", &[(V, "%x")]);
            "%x".to_string()
        },
        |f| {
            let a = f.call(V, "jv_arr", &[("i64", "1")]);
            f.call(V, "jv_arr_push", &[(V, &a), (V, "%x")])
        },
    );
    let n = f.call("i64", "jv_arr_len", &[(V, &arr)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let esc = m.cstr("'\\''");
    f.for_range("0", &n, |f, i, _, _| {
        let nf = f.ne(i, "0");
        f.if_then(&nf, |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "32")]));
        let e = f.call(V, "jv_arr_get", &[(V, &arr), ("i64", i)]);
        let t = f.tag(&e);
        let iss = f.eq(&t, "4");
        let big = f.icmp("uge", "i64", &t, "5");
        f.if_then(&big, |f| {
            let d = f.ld_at("ptr", &sb, 0);
            f.call("void", "free", &[("ptr", &d)]);
            f.call("void", "jv_release", &[(V, &arr)]);
            ret_err1(m, f, &e, "can not be escaped for shell");
        });
        f.if_else(
            &iss,
            |f| {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "39")]);
                let d = f.call("ptr", "jv_str_ptr", &[(V, &e)]);
                let sl = f.call("i64", "jv_str_len", &[(V, &e)]);
                f.for_range("0", &sl, |f, j, _, _| {
                    let p = f.off(&d, j);
                    let b = f.load("i8", &p);
                    let q = f.icmp("eq", "i8", &b, "39");
                    f.if_else(
                        &q,
                        |f| f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &esc)]),
                        |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &b)]),
                    );
                });
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "39")]);
            },
            |f| f.call("void", "jv_dump", &[("ptr", &sb), (V, &e), ("i64", "0"), ("i64", "0")]),
        );
    });
    f.call("void", "jv_release", &[(V, &arr)]);
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);

    // base64
    f.block(&labels[7]);
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let iv = f.var_init("i64", "0");
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, &n)
        },
        |f, _, _| {
            let i = f.load("i64", &iv);
            let rem = f.sub(&n, &i);
            let byte = |f: &mut Func, k: i64| -> String {
                let j = f.add(&i, &k.to_string());
                let inb = f.lt(&j, &n);
                f.if_val(
                    &inb,
                    "i64",
                    |f| {
                        let p = f.off(&d, &j);
                        let b = f.load("i8", &p);
                        f.cast("zext", "i8", &b, "i64")
                    },
                    |_| "0".into(),
                )
            };
            let b0 = byte(f, 0);
            let b1 = byte(f, 1);
            let b2 = byte(f, 2);
            let x0 = f.bin("shl", "i64", &b0, "16");
            let x1 = f.bin("shl", "i64", &b1, "8");
            let x = f.bin("or", "i64", &x0, &x1);
            let w = f.bin("or", "i64", &x, &b2);
            for (k, sh) in [(0, 18), (1, 12), (2, 6), (3, 0)] {
                let present = f.icmp("sgt", "i64", &rem, &(k - 1).max(0).to_string());
                let present = if k <= 1 { "true".to_string() } else { present };
                let c = f.bin("lshr", "i64", &w, &sh.to_string());
                let c = f.bin("and", "i64", &c, "63");
                let cp = f.off(&b64, &c);
                let ch = f.load("i8", &cp);
                let out = f.select(&present, "i8", &ch, "61");
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &out)]);
            }
            let i3 = f.add(&i, "3");
            f.store("i64", &i3, &iv);
        },
    );
    f.call("void", "jv_release", &[(V, &s)]);
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);

    // base64d
    f.block(&labels[8]);
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let acc = f.var_init("i64", "0");
    let bits = f.var_init("i64", "0");
    let invalid = f.var_init("i1", "false");
    let cntv = f.var_init("i64", "0");
    f.for_range("0", &n, |f, i, brk, cont| {
        let p = f.off(&d, i);
        let b = f.load("i8", &p);
        let bi = f.cast("zext", "i8", &b, "i64");
        let isp = f.eq(&bi, "61");
        f.if_then(&isp, |f| f.br(brk));
        let val = f.var_init("i64", "-1");
        let r = |f: &mut Func, lo: i64, hi: i64, base: i64| {
            let a = f.icmp("sge", "i64", &bi, &lo.to_string());
            let c = f.icmp("sle", "i64", &bi, &hi.to_string());
            let inr = f.and1(&a, &c);
            f.if_then(&inr, |f| {
                let x = f.sub(&bi, &lo.to_string());
                let y = f.add(&x, &base.to_string());
                f.store("i64", &y, &val);
            });
        };
        r(f, 65, 90, 0);
        r(f, 97, 122, 26);
        r(f, 48, 57, 52);
        r(f, 43, 43, 62);
        r(f, 47, 47, 63);
        let v = f.load("i64", &val);
        let bad = f.lt(&v, "0");
        f.if_then(&bad, |f| {
            f.store("i1", "true", &invalid);
            f.br(brk);
        });
        let c0 = f.load("i64", &cntv);
        let c1 = f.add(&c0, "1");
        f.store("i64", &c1, &cntv);
        let a = f.load("i64", &acc);
        let a6 = f.bin("shl", "i64", &a, "6");
        let a2 = f.bin("or", "i64", &a6, &v);
        let a3 = f.bin("and", "i64", &a2, "16777215");
        f.store("i64", &a3, &acc);
        let bt = f.load("i64", &bits);
        let bt2 = f.add(&bt, "6");
        f.store("i64", &bt2, &bits);
        let ge8 = f.icmp("sge", "i64", &bt2, "8");
        let ng = f.not1(&ge8);
        f.if_then(&ng, |f| f.br(cont));
        let bt3 = f.sub(&bt2, "8");
        f.store("i64", &bt3, &bits);
        let ch = f.bin("lshr", "i64", &a3, &bt3);
        let c8 = f.cast("trunc", "i64", &ch, "i8");
        f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &c8)]);
    });
    let cnt = f.load("i64", &cntv);
    let rem = f.bin("and", "i64", &cnt, "3");
    let trailing = f.eq(&rem, "1");
    f.if_then(&trailing, |f| {
        let d = f.ld_at("ptr", &sb, 0);
        f.call("void", "free", &[("ptr", &d)]);
        f.call("void", "jv_release", &[(V, &s)]);
        ret_err1(m, f, "%x", "trailing base64 byte found");
    });
    f.call("void", "jv_release", &[(V, &s)]);
    let inv = f.load("i1", &invalid);
    f.if_then(&inv, |f| {
        let d = f.ld_at("ptr", &sb, 0);
        f.call("void", "free", &[("ptr", &d)]);
        ret_err1(m, f, "%x", "is not valid base64 data");
    });
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);

    // base32
    f.block(&labels[9]);
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let iv = f.var_init("i64", "0");
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, &n)
        },
        |f, _, _| {
            let i = f.load("i64", &iv);
            let rem = f.sub(&n, &i);
            let w = f.var_init("i64", "0");
            for k in 0..5 {
                let j = f.add(&i, &k.to_string());
                let inb = f.lt(&j, &n);
                let b = f.if_val(
                    &inb,
                    "i64",
                    |f| {
                        let p = f.off(&d, &j);
                        let b = f.load("i8", &p);
                        f.cast("zext", "i8", &b, "i64")
                    },
                    |_| "0".into(),
                );
                let x = f.load("i64", &w);
                let x8 = f.bin("shl", "i64", &x, "8");
                let y = f.bin("or", "i64", &x8, &b);
                f.store("i64", &y, &w);
            }
            let wv = f.load("i64", &w);
            // number of output chars for rem bytes: 1->2, 2->4, 3->5, 4->7, 5+->8
            let c1 = f.eq(&rem, "1");
            let c2 = f.eq(&rem, "2");
            let c3 = f.eq(&rem, "3");
            let c4 = f.eq(&rem, "4");
            let s4 = f.select(&c4, "i64", "7", "8");
            let s3 = f.select(&c3, "i64", "5", &s4);
            let s2 = f.select(&c2, "i64", "4", &s3);
            let nch = f.select(&c1, "i64", "2", &s2);
            for k in 0..8 {
                let sh = 35 - 5 * k;
                let c = f.bin("lshr", "i64", &wv, &sh.to_string());
                let c = f.bin("and", "i64", &c, "31");
                let cp = f.off(&b32, &c);
                let ch = f.load("i8", &cp);
                let present = f.icmp("slt", "i64", &k.to_string(), &nch);
                let out = f.select(&present, "i8", &ch, "61");
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &out)]);
            }
            let i5 = f.add(&i, "5");
            f.store("i64", &i5, &iv);
        },
    );
    f.call("void", "jv_release", &[(V, &s)]);
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);

    // urid: percent-decoding
    f.block(&labels[10]);
    let s = f.call(V, "jv_tostring", &[(V, "%x")]);
    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let iv = f.var_init("i64", "0");
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, &n)
        },
        |f, _, cont| {
            let i = f.load("i64", &iv);
            let p = f.off(&d, &i);
            let b = f.load("i8", &p);
            let pct = f.icmp("eq", "i8", &b, "37");
            let npct = f.not1(&pct);
            f.if_then(&npct, |f| {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &b)]);
                let i1 = f.add(&i, "1");
                f.store("i64", &i1, &iv);
                f.br(cont);
            });
            let i3 = f.add(&i, "3");
            let fits = f.icmp("sle", "i64", &i3, &n);
            let hv = f.if_val(
                &fits,
                "i64",
                |f| {
                    let p1 = f.off(&p, "1");
                    let a = f.load("i8", &p1);
                    let p2 = f.off(&p, "2");
                    let c = f.load("i8", &p2);
                    let ha = f.call("i64", "hexval", &[("i8", &a)]);
                    let hc = f.call("i64", "hexval", &[("i8", &c)]);
                    let ba = f_lt0(f, &ha);
                    let bc = f_lt0(f, &hc);
                    let bad = f.or1(&ba, &bc);
                    let hi = f.bin("shl", "i64", &ha, "4");
                    let v = f.bin("or", "i64", &hi, &hc);
                    f.select(&bad, "i64", "-1", &v)
                },
                |_| "-1".into(),
            );
            let bad = f.lt(&hv, "0");
            f.if_then(&bad, |f| {
                let dd = f.ld_at("ptr", &sb, 0);
                f.call("void", "free", &[("ptr", &dd)]);
                f.call("void", "jv_release", &[(V, &s)]);
                ret_err1(m, f, "%x", "is not a valid uri encoding");
            });
            let c8 = f.cast("trunc", "i64", &hv, "i8");
            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &c8)]);
            f.store("i64", &i3, &iv);
        },
    );
    f.call("void", "jv_release", &[(V, &s)]);
    let r = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_format", &[(V, "x"), (V, "name")], "");
    type_err_unless(m, &mut f, "%name", T_STR, "is not a valid format");
    let r = f.call(V, "jv_format", &[(V, "%name"), (V, "%x")]);
    f.ret(V, &r);
    m.add(f);
}

fn math(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_math1", &[(V, "x"), ("ptr", "fp")], "");
    type_err_unless(m, &mut f, "%x", T_NUM, "number required");
    let d = f.num_of("%x");
    let r = f.call("double", "%fp", &[("double", &d)]);
    let v = f.mknum(&r);
    f.ret(V, &v);
    m.add(f);

    for (name, cf, kind) in [
        ("jvb_pow", "pow", 0),
        ("jvb_atan2", "atan2", 0),
        ("jvb_fmin", "fmin", 0),
        ("jvb_fmax", "fmax", 0),
        ("jvb_fmod", "fmod", 0),
        ("jvb_scalb", "scalb", 0),
        ("jvb_nextafter", "nextafter", 0),
        ("jvb_copysign", "copysign", 0),
        ("jvb_drem", "remainder", 0),
        ("jvb_hypot", "hypot", 0),
        ("jvb_fdim", "fdim", 0),
        ("jvb_ldexp", "ldexp", 1),
        ("jvb_scalbln", "scalbln", 2),
    ] {
        let mut f = Func::new("internal", V, name, &[(V, "x"), (V, "a"), (V, "b")], "");
        let a = f.tag_is("%a", T_NUM);
        let b = f.tag_is("%b", T_NUM);
        let ok = f.and1(&a, &b);
        let bad = f.not1(&ok);
        f.if_then(&bad, |f| ret_err1(m, f, "%a", "number required"));
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let r = match kind {
            0 => f.call("double", cf, &[("double", &x), ("double", &y)]),
            1 => {
                let yi = f.cast("fptosi", "double", &y, "i32");
                f.call("double", cf, &[("double", &x), ("i32", &yi)])
            }
            _ => {
                let yi = f.cast("fptosi", "double", &y, "i64");
                f.call("double", cf, &[("double", &x), ("i64", &yi)])
            }
        };
        let v = f.mknum(&r);
        f.ret(V, &v);
        m.add(f);
    }

    let mut f = Func::new("internal", V, "jvb_frexp", &[(V, "x")], "");
    type_err_unless(m, &mut f, "%x", T_NUM, "number required");
    let d = f.num_of("%x");
    let ep = f.var("i32");
    let r = f.call("double", "frexp", &[("double", &d), ("ptr", &ep)]);
    let e = f.load("i32", &ep);
    let ed = f.cast("sitofp", "i32", &e, "double");
    let a = f.call(V, "jv_arr", &[("i64", "2")]);
    let rv = f.mknum(&r);
    let ev = f.mknum(&ed);
    let a = f.call(V, "jv_arr_push", &[(V, &a), (V, &rv)]);
    let a = f.call(V, "jv_arr_push", &[(V, &a), (V, &ev)]);
    f.ret(V, &a);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_modf", &[(V, "x")], "");
    type_err_unless(m, &mut f, "%x", T_NUM, "number required");
    let d = f.num_of("%x");
    let ip = f.var("double");
    let r = f.call("double", "modf", &[("double", &d), ("ptr", &ip)]);
    let i = f.load("double", &ip);
    let a = f.call(V, "jv_arr", &[("i64", "2")]);
    let rv = f.mknum(&r);
    let iv = f.mknum(&i);
    let a = f.call(V, "jv_arr_push", &[(V, &a), (V, &rv)]);
    let a = f.call(V, "jv_arr_push", &[(V, &a), (V, &iv)]);
    f.ret(V, &a);
    m.add(f);
}

fn collections(m: &mut Module) {
    let mut f = Func::new("internal", V, "jvb_by_impl", &[(V, "a"), (V, "keys"), (V, "mode")], "");
    let md = f.num_of("%mode");
    let mi = f.cast("fptosi", "double", &md, "i64");
    let r = f.call(V, "jv_by_impl", &[(V, "%a"), (V, "%keys"), ("i64", &mi)]);
    f.ret(V, &r);
    m.add(f);

    for (name, mode) in [("jvb_sort_by_impl", 0), ("jvb_group_by_impl", 1), ("jvb_unique_by_impl", 2), ("jvb_min_by_impl", 3), ("jvb_max_by_impl", 4)] {
        let mut f = Func::new("internal", V, name, &[(V, "a"), (V, "k")], "");
        let r = f.call(V, "jv_by_impl", &[(V, "%a"), (V, "%k"), ("i64", &mode.to_string())]);
        f.ret(V, &r);
        m.add(f);
    }
    for (name, mode) in [("jvb_min", 3), ("jvb_max", 4), ("jvb_unique", 2)] {
        let mut f = Func::new("internal", V, name, &[(V, "a")], "");
        let r = f.call(V, "jv_by_impl", &[(V, "%a"), (V, "%a"), ("i64", &mode.to_string())]);
        f.ret(V, &r);
        m.add(f);
    }

    // add: `reduce .[] as $x (null; . + $x)` with in-place accumulation
    let mut f = Func::new("internal", V, "jvb_add", &[(V, "a")], "");
    let isarr = f.tag_is("%a", T_ARR);
    let isobj = f.tag_is("%a", T_OBJ);
    let ok = f.or1(&isarr, &isobj);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| {
        let e = f.call(V, "jv_iter_err", &[(V, "%a")]);
        f.ret(V, &e);
    });
    let n = f.if_val(&isarr, "i64", |f| f.call("i64", "jv_arr_len", &[(V, "%a")]), |f| f.call("i64", "jv_obj_len", &[(V, "%a")]));
    let acc = f.var_init(V, NULL);
    f.for_range("0", &n, |f, i, _, _| {
        let v = f.if_val(
            &isarr,
            V,
            |f| f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]),
            |f| f.call(V, "jv_obj_val_at", &[(V, "%a"), ("i64", i)]),
        );
        let a = f.load(V, &acc);
        let r = f.call(V, "jv_add_c2", &[(V, &a), (V, &v)]);
        let err = f.tag_is(&r, T_ERR);
        f.if_then(&err, |f| f.ret(V, &r));
        f.store(V, &r, &acc);
    });
    let a = f.load(V, &acc);
    f.ret(V, &a);
    m.add(f);

    // consuming add that also handles null accumulators without copying
    let mut f = Func::new("internal", V, "jv_add_c2", &[(V, "a"), (V, "b")], "");
    let an = f.tag_is("%a", T_NULL);
    f.if_then(&an, |f| {
        f.call("void", "jv_retain", &[(V, "%b")]);
        f.ret(V, "%b");
    });
    let ab = f.bits("%a");
    let bb = f.bits("%b");
    let same = f.eq(&ab, &bb);
    f.if_then(&same, |f| {
        let r = f.call(V, "jv_add", &[(V, "%a"), (V, "%b")]);
        f.call("void", "jv_release", &[(V, "%a")]);
        f.ret(V, &r);
    });
    let r = f.call(V, "jv_add_c", &[(V, "%a"), (V, "%b")]);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_reverse", &[(V, "a")], "");
    // jq 1.8: def reverse: [.[length - 1 - range(0;length)]];
    let isarr0 = f.tag_is("%a", T_ARR);
    let notarr0 = f.not1(&isarr0);
    f.if_then(&notarr0, |f| {
        let l = f.call(V, "jv_length", &[(V, "%a")]);
        let lerr = f.tag_is(&l, T_ERR);
        f.if_then(&lerr, |f| f.ret(V, &l));
        let ld = f.num_of(&l);
        let z = f.fcmp("oeq", &ld, "0.0");
        f.if_then(&z, |f| {
            let a = f.call(V, "jv_arr", &[("i64", "0")]);
            f.ret(V, &a);
        });
        let k = numconst(0.0);
        let e = f.call(V, "jv_index_err", &[(V, "%a"), (V, &k)]);
        f.ret(V, &e);
    });
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let r = f.call(V, "jv_arr", &[("i64", &n)]);
    let rv = f.var_init(V, &r);
    f.for_range("0", &n, |f, i, _, _| {
        let n1 = f.sub(&n, "1");
        let j = f.sub(&n1, i);
        let e = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", &j)]);
        let a = f.load(V, &rv);
        let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &e)]);
        f.store(V, &a2, &rv);
    });
    let r = f.load(V, &rv);
    f.ret(V, &r);
    m.add(f);

    // _flatten(depth)
    let mut f = Func::new("internal", V, "jvb_flatten", &[(V, "a"), (V, "depth")], "");
    type_err_unless(m, &mut f, "%a", T_ARR, "cannot be flattened");
    let dd = f.num_of("%depth");
    let r = f.call(V, "jv_arr", &[("i64", "8")]);
    let rv = f.var_init(V, &r);
    f.call("void", "jv_flatten_into", &[("ptr", &rv), (V, "%a"), ("double", &dd)]);
    let r = f.load(V, &rv);
    f.ret(V, &r);
    m.add(f);

    let mut f = Func::new("internal", "void", "jv_flatten_into", &[("ptr", "out"), (V, "a"), ("double", "d")], "");
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]);
        let isa = f.tag_is(&e, T_ARR);
        let pos = f.fcmp("one", "%d", "0.0");
        let rec = f.and1(&isa, &pos);
        f.if_else(
            &rec,
            |f| {
                let d1 = f.bin("fsub", "double", "%d", "1.0");
                f.call("void", "jv_flatten_into", &[("ptr", "%out"), (V, &e), ("double", &d1)]);
            },
            |f| {
                let a = f.load(V, "%out");
                let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &e)]);
                f.store(V, &a2, "%out");
            },
        );
    });
    f.ret_void();
    m.add(f);

    // bsearch: index or (-1 - insertion point)
    let mut f = Func::new("internal", V, "jvb_bsearch", &[(V, "a"), (V, "t")], "");
    type_err_unless(m, &mut f, "%a", T_ARR, "cannot be searched from");
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let lo = f.var_init("i64", "0");
    let hi = f.var_init("i64", &n);
    f.while_(
        |f| {
            let l = f.load("i64", &lo);
            let h = f.load("i64", &hi);
            f.lt(&l, &h)
        },
        |f, _, _| {
            let l = f.load("i64", &lo);
            let h = f.load("i64", &hi);
            let s = f.add(&l, &h);
            let mid = f.bin("sdiv", "i64", &s, "2");
            let e = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", &mid)]);
            let c = f.call("i32", "jv_cmp", &[(V, &e), (V, "%t")]);
            let z = f.icmp("eq", "i32", &c, "0");
            f.if_then(&z, |f| {
                let d = f.cast("sitofp", "i64", &mid, "double");
                let v = f.mknum(&d);
                f.ret(V, &v);
            });
            let lt = f.icmp("slt", "i32", &c, "0");
            f.if_else(
                &lt,
                |f| {
                    let m1 = f.add(&mid, "1");
                    f.store("i64", &m1, &lo);
                },
                |f| f.store("i64", &mid, &hi),
            );
        },
    );
    let l = f.load("i64", &lo);
    let r = f.sub("-1", &l);
    let d = f.cast("sitofp", "i64", &r, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);
}

fn time(m: &mut Module) {
    // now
    let mut f = Func::new("internal", V, "jvb_now", &[(V, "x")], "");
    let ts = f.array_var("i64", 2);
    f.call("i32", "clock_gettime", &[("i32", "0"), ("ptr", &ts)]);
    let s = f.load("i64", &ts);
    let nsp = f.off(&ts, "8");
    let ns = f.load("i64", &nsp);
    let sd = f.cast("sitofp", "i64", &s, "double");
    let nd = f.cast("sitofp", "i64", &ns, "double");
    let frac = f.bin("fdiv", "double", &nd, &fconst(1e9));
    let t = f.bin("fadd", "double", &sd, &frac);
    let v = f.mknum(&t);
    f.ret(V, &v);
    m.add(f);

    // struct tm <-> jq broken-down time array
    // glibc struct tm: 9 ints (sec,min,hour,mday,mon,year,wday,yday,isdst) + long gmtoff + ptr zone
    let mut f = Func::new("internal", V, "jv_tm_to_arr", &[("ptr", "tm"), ("double", "fsec")], "");
    let a = f.call(V, "jv_arr", &[("i64", "8")]);
    let av = f.var_init(V, &a);
    // jq order: [sec(with fraction), min, hour, mday, mon, year-1900, wday, yday]... actually
    // jq: [year+1900, month, mday, hours, minutes, seconds(float), wday, yday]
    let fields = [(5, 1900), (4, 0), (3, 0), (2, 0), (1, 0), (0, 0), (6, 0), (7, 0)];
    for (idx, add) in fields {
        let p = f.off("%tm", &(idx * 4).to_string());
        let x = f.load("i32", &p);
        let xd = f.cast("sitofp", "i32", &x, "double");
        let xd = if add != 0 { f.bin("fadd", "double", &xd, &format!("{}.0", add)) } else { xd };
        let xd = if idx == 0 { f.bin("fadd", "double", &xd, "%fsec") } else { xd };
        let v = f.mknum(&xd);
        let a = f.load(V, &av);
        let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &v)]);
        f.store(V, &a2, &av);
    }
    let a = f.load(V, &av);
    f.ret(V, &a);
    m.add(f);

    // array -> struct tm; returns false on bad input
    let mut f = Func::new("internal", "i1", "jv_arr_to_tm", &[(V, "a"), ("ptr", "tm")], "");
    f.call("ptr", "memset", &[("ptr", "%tm"), ("i32", "0"), ("i64", "64")]);
    let isa = f.tag_is("%a", T_ARR);
    let nota = f.not1(&isa);
    f.if_then(&nota, |f| f.ret("i1", "false"));
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let fields = [(5, -1900), (4, 0), (3, 0), (2, 0), (1, 0), (0, 0), (6, 0), (7, 0)];
    for (j, (idx, add)) in fields.iter().enumerate() {
        let inb = f.lt(&j.to_string(), &n);
        f.if_then(&inb, |f| {
            let e = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", &j.to_string())]);
            let isn = f.tag_is(&e, T_NUM);
            let notn = f.not1(&isn);
            f.if_then(&notn, |f| f.ret("i1", "false"));
            let d = f.num_of(&e);
            let fl = f.call("double", "floor", &[("double", &d)]);
            let x = f.cast("fptosi", "double", &fl, "i32");
            let x = if *add != 0 { f.bin("add", "i32", &x, &add.to_string()) } else { x };
            let p = f.off("%tm", &(idx * 4).to_string());
            f.store("i32", &x, &p);
        });
    }
    f.ret("i1", "true");
    m.add(f);

    for (name, func) in [("jvb_gmtime", "gmtime_r"), ("jvb_localtime", "localtime_r")] {
        let mut f = Func::new("internal", V, name, &[(V, "x")], "");
        type_err_unless(m, &mut f, "%x", T_NUM, if func == "gmtime_r" { "gmtime() requires a number" } else { "localtime() requires a number" });
        let d = f.num_of("%x");
        let fl = f.call("double", "floor", &[("double", &d)]);
        let frac = f.bin("fsub", "double", &d, &fl);
        let t = f.cast("fptosi", "double", &fl, "i64");
        let tp = f.var_init("i64", &t);
        let tm = f.array_var("i8", 64);
        f.call("ptr", func, &[("ptr", &tp), ("ptr", &tm)]);
        let a = f.call(V, "jv_tm_to_arr", &[("ptr", &tm), ("double", &frac)]);
        f.ret(V, &a);
        m.add(f);
    }

    let mut f = Func::new("internal", V, "jvb_mktime", &[(V, "x")], "");
    let tm = f.array_var("i8", 64);
    let isa = f.tag_is("%x", T_ARR);
    let nota = f.not1(&isa);
    f.if_then(&nota, |f| ret_errc(m, f, "mktime requires array of 6 numbers"));
    let ok = f.call("i1", "jv_arr_to_tm", &[(V, "%x"), ("ptr", &tm)]);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| ret_errc(m, f, "mktime requires parsed datetime inputs"));
    let t = f.call("i64", "timegm", &[("ptr", &tm)]);
    let d = f.cast("sitofp", "i64", &t, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    for (name, local) in [("jvb_strftime", false), ("jvb_strflocaltime", true)] {
        let mut f = Func::new("internal", V, name, &[(V, "x"), (V, "fmt")], "");
        let isn = f.tag_is("%x", T_NUM);
        let arr = f.if_val(
            &isn,
            V,
            |f| f.call(V, if local { "jvb_localtime" } else { "jvb_gmtime" }, &[(V, "%x")]),
            |f| {
                f.call("void", "jv_retain", &[(V, "%x")]);
                "%x".to_string()
            },
        );
        let fs = f.tag_is("%fmt", T_STR);
        let nfs = f.not1(&fs);
        f.if_then(&nfs, |f| ret_errc(m, f, if local { "strflocaltime/1 requires a string format" } else { "strftime/1 requires a string format" }));
        let tm = f.array_var("i8", 64);
        let ok = f.call("i1", "jv_arr_to_tm", &[(V, &arr), ("ptr", &tm)]);
        f.call("void", "jv_release", &[(V, &arr)]);
        let bad = f.not1(&ok);
        f.if_then(&bad, |f| ret_errc(m, f, if local { "strflocaltime/1 requires parsed datetime inputs" } else { "strftime/1 requires parsed datetime inputs" }));
        let buf = f.array_var("i8", 512);
        let fd = f.call("ptr", "jv_str_ptr", &[(V, "%fmt")]);
        let n = f.call("i64", "strftime", &[("ptr", &buf), ("i64", "512"), ("ptr", &fd), ("ptr", &tm)]);
        let s = f.call(V, "jv_str", &[("ptr", &buf), ("i64", &n)]);
        f.ret(V, &s);
        m.add(f);
    }

    let mut f = Func::new("internal", V, "jvb_strptime", &[(V, "x"), (V, "fmt")], "");
    type_err_unless(m, &mut f, "%x", T_STR, "strptime/1 requires string inputs and arguments");
    type_err_unless(m, &mut f, "%fmt", T_STR, "strptime/1 requires string inputs and arguments");
    let tm = f.array_var("i8", 64);
    f.call("ptr", "memset", &[("ptr", &tm), ("i32", "0"), ("i64", "64")]);
    let xd = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
    let fd = f.call("ptr", "jv_str_ptr", &[(V, "%fmt")]);
    let r = f.call("ptr", "strptime", &[("ptr", &xd), ("ptr", &fd), ("ptr", &tm)]);
    let fail = f.icmp("eq", "ptr", &r, "null");
    let b = f.load("i8", &r);
    let _ = b;
    f.if_then(&fail, |f| {
        let sb = f.array_var("i8", 24);
        f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
        f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%x")]);
        let s1 = m.cstr(" does not match format ");
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &s1)]);
        f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%fmt")]);
        let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
        let e = f.call(V, "jv_err_take", &[(V, &s)]);
        f.ret(V, &e);
    });
    // normalise wday/yday via timegm round-trip
    let t = f.call("i64", "timegm", &[("ptr", &tm)]);
    let tp = f.var_init("i64", &t);
    f.call("ptr", "gmtime_r", &[("ptr", &tp), ("ptr", &tm)]);
    let a = f.call(V, "jv_tm_to_arr", &[("ptr", &tm), ("double", "0.0")]);
    f.ret(V, &a);
    m.add(f);
}

pub const BUILTIN_NAMES: &str = include_str!("builtins.txt");

fn f_lt0(f: &mut Func, x: &str) -> String {
    f.lt(x, "0")
}

/// jq's lazily reported path-expression errors.
pub fn path_errors(m: &mut Module) {
    let p1 = m.cstr("Invalid path expression near attempt to access element ");
    let p2 = m.cstr(" of ");
    let mut f = Func::new("internal", V, "jv_path_access_err", &[(V, "k"), (V, "x")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "96")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
    f.call("void", "sb_dump_truncn", &[("ptr", &sb), (V, "%k"), ("i64", "15")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p2)]);
    f.call("void", "sb_dump_truncn", &[("ptr", &sb), (V, "%x"), ("i64", "30")]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    let p = m.cstr("Invalid path expression near attempt to iterate through ");
    let mut f = Func::new("internal", V, "jv_path_iter_err", &[(V, "x")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "96")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p)]);
    f.call("void", "sb_dump_truncn", &[("ptr", &sb), (V, "%x"), ("i64", "30")]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);
}
