//! Comparison, sorting, arithmetic, indexing.

use super::*;
use crate::ir::{fconst, Func, Module, V};

pub fn emit(m: &mut Module) {
    errors(m);
    eq(m);
    cmp(m);
    msort(m);
    keys(m);
    arith(m);
    index(m);
    misc(m);
}

/// Emit `ret` of an error built as "<desc a> and <desc b> <suffix>".
pub fn ret_err2(m: &mut Module, f: &mut Func, a: &str, b: &str, suffix: &str) {
    let s = m.cstr(suffix);
    let e = f.call(V, "jv_err2", &[(V, a), (V, b), ("ptr", &s)]);
    f.ret(V, &e);
}

/// Emit `ret` of an error built as "<desc a> <suffix>".
pub fn ret_err1(m: &mut Module, f: &mut Func, a: &str, suffix: &str) {
    let s = m.cstr(suffix);
    let e = f.call(V, "jv_err1", &[(V, a), ("ptr", &s)]);
    f.ret(V, &e);
}

pub fn ret_errc(m: &mut Module, f: &mut Func, msg: &str) {
    let s = m.cstr(msg);
    let e = f.call(V, "jv_err_c", &[("ptr", &s)]);
    f.ret(V, &e);
}

fn errors(m: &mut Module) {
    let and = m.cstr(" and ");
    let mut f = Func::new("internal", V, "jv_err2", &[(V, "a"), (V, "b"), ("ptr", "s")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%a")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &and)]);
    f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%b")]);
    f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "32")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", "%s")]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    let mut f = Func::new("internal", V, "jv_err1", &[(V, "a"), ("ptr", "s")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%a")]);
    f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "32")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", "%s")]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    // "Cannot index <t> with <k>" / with string "k"
    let p1 = m.cstr("Cannot index ");
    let p2 = m.cstr(" with ");
    let p3 = m.cstr("string \"");
    let mut f = Func::new("internal", V, "jv_index_err", &[(V, "t"), (V, "k")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p1)]);
    let tt = f.tag("%t");
    let tn = f.call("ptr", "jv_kind_name", &[("i64", &tt)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &tn)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p2)]);
    let ks = f.tag_is("%k", T_STR);
    f.if_else(
        &ks,
        |f| {
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &p3)]);
            f.call("void", "sb_putv", &[("ptr", &sb), (V, "%k")]);
            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "34")]);
        },
        |f| {
            let kt = f.tag("%k");
            let kn = f.call("ptr", "jv_kind_name", &[("i64", &kt)]);
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &kn)]);
        },
    );
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    let it = m.cstr("Cannot iterate over ");
    let mut f = Func::new("internal", V, "jv_iter_err", &[(V, "t")], "noinline");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &it)]);
    f.call("void", "sb_typedesc", &[("ptr", &sb), (V, "%t")]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);
}

fn eq(m: &mut Module) {
    let mut f = Func::new("internal", "i1", "jv_eq", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let ne = f.ne(&ta, &tb);
    f.if_then(&ne, |f| f.ret("i1", "false"));
    let (ln, ls, la, lo, lt) = (f.label("num"), f.label("str"), f.label("arr"), f.label("obj"), f.label("triv"));
    f.switch(&ta, &lt, &[(3, ln.clone()), (4, ls.clone()), (5, la.clone()), (6, lo.clone())]);
    f.block(&lt);
    f.ret("i1", "true");
    f.block(&ln);
    let lit = f.call("i32", "jv_numlit_cmp2", &[(V, "%a"), (V, "%b")]);
    let haslit = f.icmp("ne", "i32", &lit, "2");
    f.if_then(&haslit, |f| {
        let z = f.icmp("eq", "i32", &lit, "0");
        f.ret("i1", &z);
    });
    let x = f.num_of("%a");
    let y = f.num_of("%b");
    let r = f.fcmp("oeq", &x, &y);
    f.ret("i1", &r);
    f.block(&ls);
    let r = f.call("i1", "jv_str_eq", &[(V, "%a"), (V, "%b")]);
    f.ret("i1", &r);
    f.block(&la);
    let na = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
    let d = f.ne(&na, &nb);
    f.if_then(&d, |f| f.ret("i1", "false"));
    f.for_range("0", &na, |f, i, _, _| {
        let x = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]);
        let y = f.call(V, "jv_arr_get", &[(V, "%b"), ("i64", i)]);
        let e = f.call("i1", "jv_eq", &[(V, &x), (V, &y)]);
        let n = f.not1(&e);
        f.if_then(&n, |f| f.ret("i1", "false"));
    });
    f.ret("i1", "true");
    f.block(&lo);
    let na = f.call("i64", "jv_obj_len", &[(V, "%a")]);
    let nb = f.call("i64", "jv_obj_len", &[(V, "%b")]);
    let d = f.ne(&na, &nb);
    f.if_then(&d, |f| f.ret("i1", "false"));
    f.for_range("0", &na, |f, i, _, _| {
        let k = f.call(V, "jv_obj_key_at", &[(V, "%a"), ("i64", i)]);
        let x = f.call(V, "jv_obj_val_at", &[(V, "%a"), ("i64", i)]);
        let y = f.call(V, "jv_obj_get", &[(V, "%b"), (V, &k)]);
        let e = f.call("i1", "jv_eq", &[(V, &x), (V, &y)]);
        let n = f.not1(&e);
        f.if_then(&n, |f| f.ret("i1", "false"));
    });
    f.ret("i1", "true");
    m.add(f);
}

fn cmp(m: &mut Module) {
    let mut f = Func::new("internal", "i32", "jv_cmp", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let ne = f.ne(&ta, &tb);
    f.if_then(&ne, |f| {
        let lt = f.lt(&ta, &tb);
        let r = f.select(&lt, "i32", "-1", "1");
        f.ret("i32", &r);
    });
    let (ln, ls, la, lo, lt) = (f.label("num"), f.label("str"), f.label("arr"), f.label("obj"), f.label("triv"));
    f.switch(&ta, &lt, &[(3, ln.clone()), (4, ls.clone()), (5, la.clone()), (6, lo.clone())]);
    f.block(&lt);
    f.ret("i32", "0");
    f.block(&ln);
    let lit = f.call("i32", "jv_numlit_cmp2", &[(V, "%a"), (V, "%b")]);
    let haslit = f.icmp("ne", "i32", &lit, "2");
    f.if_then(&haslit, |f| f.ret("i32", &lit));
    let x = f.num_of("%a");
    let y = f.num_of("%b");
    let xn = f.fcmp("uno", &x, &x);
    let yn = f.fcmp("uno", &y, &y);
    f.if_then(&xn, |f| {
        let r = f.select(&yn, "i32", "0", "-1");
        f.ret("i32", &r);
    });
    f.if_then(&yn, |f| f.ret("i32", "1"));
    let l = f.fcmp("olt", &x, &y);
    let e = f.fcmp("oeq", &x, &y);
    let r1 = f.select(&e, "i32", "0", "1");
    let r = f.select(&l, "i32", "-1", &r1);
    f.ret("i32", &r);
    f.block(&ls);
    let r = f.call("i32", "jv_str_cmp", &[(V, "%a"), (V, "%b")]);
    f.ret("i32", &r);
    f.block(&la);
    let na = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
    let lt = f.lt(&na, &nb);
    let mn = f.select(&lt, "i64", &na, &nb);
    f.for_range("0", &mn, |f, i, _, _| {
        let x = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]);
        let y = f.call(V, "jv_arr_get", &[(V, "%b"), ("i64", i)]);
        let c = f.call("i32", "jv_cmp", &[(V, &x), (V, &y)]);
        let nz = f.icmp("ne", "i32", &c, "0");
        f.if_then(&nz, |f| f.ret("i32", &c));
    });
    let gt = f.icmp("sgt", "i64", &na, &nb);
    let r1 = f.select(&gt, "i32", "1", "0");
    let r = f.select(&lt, "i32", "-1", &r1);
    f.ret("i32", &r);
    f.block(&lo);
    let ka = f.call(V, "jv_keys", &[(V, "%a")]);
    let kb = f.call(V, "jv_keys", &[(V, "%b")]);
    let c = f.call("i32", "jv_cmp", &[(V, &ka), (V, &kb)]);
    f.call("void", "jv_release", &[(V, &kb)]);
    let nz = f.icmp("ne", "i32", &c, "0");
    f.if_then(&nz, |f| {
        f.call("void", "jv_release", &[(V, &ka)]);
        f.ret("i32", &c);
    });
    let n = f.call("i64", "jv_arr_len", &[(V, &ka)]);
    f.for_range("0", &n, |f, i, _, _| {
        let k = f.call(V, "jv_arr_get", &[(V, &ka), ("i64", i)]);
        let x = f.call(V, "jv_obj_get", &[(V, "%a"), (V, &k)]);
        let y = f.call(V, "jv_obj_get", &[(V, "%b"), (V, &k)]);
        let c = f.call("i32", "jv_cmp", &[(V, &x), (V, &y)]);
        let nz = f.icmp("ne", "i32", &c, "0");
        f.if_then(&nz, |f| {
            f.call("void", "jv_release", &[(V, &ka)]);
            f.ret("i32", &c);
        });
    });
    f.call("void", "jv_release", &[(V, &ka)]);
    f.ret("i32", "0");
    m.add(f);
}

fn msort(m: &mut Module) {
    // Stable bottom-up merge sort of n elements of `es` bytes each, keyed by the
    // %V at offset 0 of each element.
    // comparator kinds: 0 generic jv_cmp, 1 plain numbers (unboxed doubles), 2 strings
    let mut f = Func::new("internal", "i32", "jv_cmp_kind", &[(V, "a"), (V, "b"), ("i64", "kind")], "alwaysinline");
    let isnum = f.eq("%kind", "1");
    f.if_then(&isnum, |f| {
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let xn = f.fcmp("uno", &x, &x);
        let yn = f.fcmp("uno", &y, &y);
        let lt = f.fcmp("olt", &x, &y);
        let gt = f.fcmp("ogt", &x, &y);
        let r0 = f.select(&gt, "i32", "1", "0");
        let r1 = f.select(&lt, "i32", "-1", &r0);
        let r2 = f.select(&yn, "i32", "1", &r1);
        let both = f.select(&yn, "i32", "0", "-1");
        let r = f.select(&xn, "i32", &both, &r2);
        f.ret("i32", &r);
    });
    let isstr = f.eq("%kind", "2");
    f.if_then(&isstr, |f| {
        let r = f.call("i32", "jv_str_cmp", &[(V, "%a"), (V, "%b")]);
        f.ret("i32", &r);
    });
    let r = f.call("i32", "jv_cmp", &[(V, "%a"), (V, "%b")]);
    f.ret("i32", &r);
    m.add(f);

    // which comparator fits the keys (the %V at offset 0 of each element)
    let mut f = Func::new("internal", "i64", "jv_sort_kind", &[("ptr", "base"), ("i64", "n"), ("i64", "es")], "");
    let alln = f.var_init("i1", "true");
    let alls = f.var_init("i1", "true");
    f.for_range("0", "%n", |f, i, _, _| {
        let o = f.bin("mul", "i64", i, "%es");
        let p = f.off("%base", &o);
        let v = f.load(V, &p);
        let raw = f.rawtag(&v);
        let isn = f.eq(&raw, "3");
        let iss = f.eq(&raw, "4");
        let a = f.load("i1", &alln);
        let a2 = f.and1(&a, &isn);
        f.store("i1", &a2, &alln);
        let b = f.load("i1", &alls);
        let b2 = f.and1(&b, &iss);
        f.store("i1", &b2, &alls);
    });
    let a = f.load("i1", &alln);
    let b = f.load("i1", &alls);
    let k1 = f.select(&b, "i64", "2", "0");
    let k = f.select(&a, "i64", "1", &k1);
    f.ret("i64", &k);
    m.add(f);

    let mut f = Func::new("internal", "void", "jv_msort", &[("ptr", "base"), ("i64", "n"), ("i64", "es")], "");
    let kind = f.call("i64", "jv_sort_kind", &[("ptr", "%base"), ("i64", "%n"), ("i64", "%es")]);
    let small = f.icmp("slt", "i64", "%n", "2");
    f.if_then(&small, |f| f.ret_void());
    let bytes = f.bin("mul", "i64", "%n", "%es");
    let tmp = f.call("ptr", "jv_malloc", &[("i64", &bytes)]);
    let src = f.var_init("ptr", "%base");
    let dst = f.var_init("ptr", &tmp);
    let wv = f.var_init("i64", "1");
    f.while_(
        |f| {
            let w = f.load("i64", &wv);
            f.lt(&w, "%n")
        },
        |f, _, _| {
            let w = f.load("i64", &wv);
            let s = f.load("ptr", &src);
            let d = f.load("ptr", &dst);
            let lo = f.var_init("i64", "0");
            f.while_(
                |f| {
                    let l = f.load("i64", &lo);
                    f.lt(&l, "%n")
                },
                |f, _, _| {
                    let l = f.load("i64", &lo);
                    let mid0 = f.add(&l, &w);
                    let c = f.lt(&mid0, "%n");
                    let mid = f.select(&c, "i64", &mid0, "%n");
                    let hi0 = f.add(&mid, &w);
                    let c2 = f.lt(&hi0, "%n");
                    let hi = f.select(&c2, "i64", &hi0, "%n");
                    let iv = f.var_init("i64", &l);
                    let jv = f.var_init("i64", &mid);
                    let kv = f.var_init("i64", &l);
                    f.while_(
                        |f| {
                            let k = f.load("i64", &kv);
                            f.lt(&k, &hi)
                        },
                        |f, _, _| {
                            let i = f.load("i64", &iv);
                            let j = f.load("i64", &jv);
                            let k = f.load("i64", &kv);
                            let iok = f.lt(&i, &mid);
                            let jok = f.lt(&j, &hi);
                            // take left if i<mid and (j>=hi or cmp(left,right) <= 0)
                            let takel = f.var_init("i1", &iok);
                            let both = f.and1(&iok, &jok);
                            f.if_then(&both, |f| {
                                let oi = f.bin("mul", "i64", &i, "%es");
                                let pi = f.off(&s, &oi);
                                let oj = f.bin("mul", "i64", &j, "%es");
                                let pj = f.off(&s, &oj);
                                let a = f.load(V, &pi);
                                let b = f.load(V, &pj);
                                let c = f.call("i32", "jv_cmp_kind", &[(V, &a), (V, &b), ("i64", &kind)]);
                                let le = f.icmp("sle", "i32", &c, "0");
                                f.store("i1", &le, &takel);
                            });
                            let tl = f.load("i1", &takel);
                            let ok = f.bin("mul", "i64", &k, "%es");
                            let pk = f.off(&d, &ok);
                            f.if_else(
                                &tl,
                                |f| {
                                    let oi = f.bin("mul", "i64", &i, "%es");
                                    let pi = f.off(&s, &oi);
                                    f.call("ptr", "memcpy", &[("ptr", &pk), ("ptr", &pi), ("i64", "%es")]);
                                    let i1 = f.add(&i, "1");
                                    f.store("i64", &i1, &iv);
                                },
                                |f| {
                                    let oj = f.bin("mul", "i64", &j, "%es");
                                    let pj = f.off(&s, &oj);
                                    f.call("ptr", "memcpy", &[("ptr", &pk), ("ptr", &pj), ("i64", "%es")]);
                                    let j1 = f.add(&j, "1");
                                    f.store("i64", &j1, &jv);
                                },
                            );
                            let k1 = f.add(&k, "1");
                            f.store("i64", &k1, &kv);
                        },
                    );
                    f.store("i64", &hi, &lo);
                },
            );
            f.store("ptr", &d, &src);
            f.store("ptr", &s, &dst);
            let w2 = f.bin("mul", "i64", &w, "2");
            f.store("i64", &w2, &wv);
        },
    );
    let s = f.load("ptr", &src);
    let differs = f.icmp("ne", "ptr", &s, "%base");
    f.if_then(&differs, |f| {
        f.call("ptr", "memcpy", &[("ptr", "%base"), ("ptr", &s), ("i64", &bytes)]);
    });
    f.call("void", "free", &[("ptr", &tmp)]);
    f.ret_void();
    m.add(f);

    // sort array (borrowed) -> new sorted array
    let mut f = Func::new("internal", V, "jv_sort", &[(V, "a")], "");
    let isarr = f.tag_is("%a", T_ARR);
    let notarr = f.not1(&isarr);
    f.if_then(&notarr, |f| ret_err1(m, f, "%a", "cannot be sorted, as it is not an array"));
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let r = f.call(V, "jv_arr_sub", &[(V, "%a"), ("i64", "0"), ("i64", &n)]);
    let p = f.ptr_of(&r);
    let items = f.ld_at("ptr", &p, 24);
    f.call("void", "jv_msort", &[("ptr", &items), ("i64", &n), ("i64", "16")]);
    f.ret(V, &r);
    m.add(f);

    // _sort_by_impl / _group_by_impl / _unique_by_impl helpers:
    // build pair buffer (key, value) sorted stably by key. Returns buffer ptr.
    let mut f = Func::new("internal", "ptr", "jv_sorted_pairs", &[(V, "a"), (V, "keys")], "");
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    // keys from map([f]) are 1-element arrays: compare their elements directly
    let single = f.var_init("i1", "true");
    f.for_range("0", &n, |f, i, brk, _| {
        let k = f.call(V, "jv_arr_get", &[(V, "%keys"), ("i64", i)]);
        let isa = f.tag_is(&k, T_ARR);
        let one = f.if_val(&isa, "i1", |f| {
            let l = f.call("i64", "jv_arr_len", &[(V, &k)]);
            f.eq(&l, "1")
        }, |_| "false".into());
        let no = f.not1(&one);
        f.if_then(&no, |f| {
            f.store("i1", "false", &single);
            f.br(brk);
        });
    });
    let unwrap = f.load("i1", &single);
    let bytes = f.bin("mul", "i64", &n, "32");
    let buf = f.call("ptr", "jv_malloc", &[("i64", &bytes)]);
    f.for_range("0", &n, |f, i, _, _| {
        let k0 = f.call(V, "jv_arr_get", &[(V, "%keys"), ("i64", i)]);
        let k = f.if_val(&unwrap, V, |f| f.call(V, "jv_arr_get", &[(V, &k0), ("i64", "0")]), |_| k0.clone());
        let v = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]);
        let e = f.idx(&buf, i, 32);
        f.store(V, &k, &e);
        f.st_at(V, &v, &e, 16);
    });
    f.call("void", "jv_msort", &[("ptr", &buf), ("i64", &n), ("i64", "32")]);
    f.ret("ptr", &buf);
    m.add(f);

    // mode 0: sort_by, 1: group_by, 2: unique_by, 3: min_by, 4: max_by
    let mut f = Func::new("internal", V, "jv_by_impl", &[(V, "a"), (V, "keys"), ("i64", "mode")], "");
    let isarr = f.tag_is("%a", T_ARR);
    let notarr = f.not1(&isarr);
    f.if_then(&notarr, |f| ret_err1(m, f, "%a", "cannot be sorted, as it is not an array"));
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let isminmax = f.icmp("sge", "i64", "%mode", "3");
    f.if_then(&isminmax, |f| {
        let empty = f.eq(&n, "0");
        f.if_then(&empty, |f| f.ret(V, NULL));
        let best = f.var_init("i64", "0");
        let ismax = f.eq("%mode", "4");
        f.for_range("1", &n, |f, i, _, _| {
            let b = f.load("i64", &best);
            let kb = f.call(V, "jv_arr_get", &[(V, "%keys"), ("i64", &b)]);
            let ki = f.call(V, "jv_arr_get", &[(V, "%keys"), ("i64", i)]);
            let c = f.call("i32", "jv_cmp", &[(V, &ki), (V, &kb)]);
            let lt = f.icmp("slt", "i32", &c, "0");
            let ge = f.icmp("sge", "i32", &c, "0");
            let take = f.select(&ismax, "i1", &ge, &lt);
            f.if_then(&take, |f| f.store("i64", i, &best));
        });
        let b = f.load("i64", &best);
        let v = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", &b)]);
        f.call("void", "jv_retain", &[(V, &v)]);
        f.ret(V, &v);
    });
    let buf = f.call("ptr", "jv_sorted_pairs", &[(V, "%a"), (V, "%keys")]);
    let out = f.var_init(V, NULL);
    let r0 = f.call(V, "jv_arr", &[("i64", &n)]);
    f.store(V, &r0, &out);
    let issort = f.eq("%mode", "0");
    f.if_then(&issort, |f| {
        f.for_range("0", &n, |f, i, _, _| {
            let e = f.idx(&buf, i, 32);
            let v = f.ld_at(V, &e, 16);
            let o = f.load(V, &out);
            let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &v)]);
            f.store(V, &o2, &out);
        });
    });
    let isgroup = f.eq("%mode", "1");
    f.if_then(&isgroup, |f| {
        let grp = f.var_init(V, NULL);
        f.for_range("0", &n, |f, i, _, _| {
            let e = f.idx(&buf, i, 32);
            let k = f.load(V, &e);
            let v = f.ld_at(V, &e, 16);
            let first = f.eq(i, "0");
            let newgrp = f.var_init("i1", &first);
            let notfirst = f.not1(&first);
            f.if_then(&notfirst, |f| {
                let im1 = f.sub(i, "1");
                let pe = f.idx(&buf, &im1, 32);
                let pk = f.load(V, &pe);
                let c = f.call("i1", "jv_eq", &[(V, &k), (V, &pk)]);
                let nc = f.not1(&c);
                f.store("i1", &nc, &newgrp);
            });
            let ng = f.load("i1", &newgrp);
            f.if_then(&ng, |f| {
                let g = f.load(V, &grp);
                let gnull = f.tag_is(&g, T_NULL);
                let gn = f.not1(&gnull);
                f.if_then(&gn, |f| {
                    let o = f.load(V, &out);
                    let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &g)]);
                    f.store(V, &o2, &out);
                    f.call("void", "jv_release", &[(V, &g)]);
                });
                let ng = f.call(V, "jv_arr", &[("i64", "4")]);
                f.store(V, &ng, &grp);
            });
            let g = f.load(V, &grp);
            let g2 = f.call(V, "jv_arr_push", &[(V, &g), (V, &v)]);
            f.store(V, &g2, &grp);
        });
        let g = f.load(V, &grp);
        let gnull = f.tag_is(&g, T_NULL);
        let gn = f.not1(&gnull);
        f.if_then(&gn, |f| {
            let o = f.load(V, &out);
            let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &g)]);
            f.store(V, &o2, &out);
            f.call("void", "jv_release", &[(V, &g)]);
        });
    });
    let isuniq = f.eq("%mode", "2");
    f.if_then(&isuniq, |f| {
        f.for_range("0", &n, |f, i, _, _| {
            let e = f.idx(&buf, i, 32);
            let k = f.load(V, &e);
            let v = f.ld_at(V, &e, 16);
            let first = f.eq(i, "0");
            let take = f.var_init("i1", &first);
            let notfirst = f.not1(&first);
            f.if_then(&notfirst, |f| {
                let im1 = f.sub(i, "1");
                let pe = f.idx(&buf, &im1, 32);
                let pk = f.load(V, &pe);
                let c = f.call("i1", "jv_eq", &[(V, &k), (V, &pk)]);
                let nc = f.not1(&c);
                f.store("i1", &nc, &take);
            });
            let t = f.load("i1", &take);
            f.if_then(&t, |f| {
                let o = f.load(V, &out);
                let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &v)]);
                f.store(V, &o2, &out);
            });
        });
    });
    f.call("void", "free", &[("ptr", &buf)]);
    let o = f.load(V, &out);
    f.ret(V, &o);
    m.add(f);
}

fn keys(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_keys_unsorted", &[(V, "o")], "");
    let isobj = f.tag_is("%o", T_OBJ);
    f.if_then(&isobj, |f| {
        let n = f.call("i64", "jv_obj_len", &[(V, "%o")]);
        let r = f.call(V, "jv_arr", &[("i64", &n)]);
        let rv = f.var_init(V, &r);
        f.for_range("0", &n, |f, i, _, _| {
            let k = f.call(V, "jv_obj_key_at", &[(V, "%o"), ("i64", i)]);
            let a = f.load(V, &rv);
            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &k)]);
            f.store(V, &a2, &rv);
        });
        let a = f.load(V, &rv);
        f.ret(V, &a);
    });
    let isarr = f.tag_is("%o", T_ARR);
    f.if_then(&isarr, |f| {
        let n = f.call("i64", "jv_arr_len", &[(V, "%o")]);
        let r = f.call(V, "jv_arr", &[("i64", &n)]);
        let rv = f.var_init(V, &r);
        f.for_range("0", &n, |f, i, _, _| {
            let d = f.cast("sitofp", "i64", i, "double");
            let x = f.mknum(&d);
            let a = f.load(V, &rv);
            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &x)]);
            f.store(V, &a2, &rv);
        });
        let a = f.load(V, &rv);
        f.ret(V, &a);
    });
    ret_err1(m, &mut f, "%o", "has no keys");
    m.add(f);

    let mut f = Func::new("internal", V, "jv_keys", &[(V, "o")], "");
    let k = f.call(V, "jv_keys_unsorted", &[(V, "%o")]);
    let isobj = f.tag_is("%o", T_OBJ);
    f.if_then(&isobj, |f| {
        let p = f.ptr_of(&k);
        let n = f.ld_at("i64", &p, 8);
        let items = f.ld_at("ptr", &p, 24);
        f.call("void", "jv_msort", &[("ptr", &items), ("i64", &n), ("i64", "16")]);
    });
    f.ret(V, &k);
    m.add(f);
}

fn arith(m: &mut Module) {
    // ---- add ----
    let mut f = Func::new("internal", V, "jv_add", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let an = f.eq(&ta, "0");
    f.if_then(&an, |f| {
        f.call("void", "jv_retain", &[(V, "%b")]);
        f.ret(V, "%b");
    });
    let bn = f.eq(&tb, "0");
    f.if_then(&bn, |f| {
        f.call("void", "jv_retain", &[(V, "%a")]);
        f.ret(V, "%a");
    });
    let same = f.eq(&ta, &tb);
    f.if_then(&same, |f| {
        let (ln, ls, la, lo, ld) = (f.label("n"), f.label("s"), f.label("a"), f.label("o"), f.label("d"));
        f.switch(&ta, &ld, &[(3, ln.clone()), (4, ls.clone()), (5, la.clone()), (6, lo.clone())]);
        f.block(&ln);
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let z = f.bin("fadd", "double", &x, &y);
        let r = f.mknum(&z);
        f.ret(V, &r);
        f.block(&ls);
        let r = f.call(V, "jv_str_concat", &[(V, "%a"), (V, "%b")]);
        f.ret(V, &r);
        f.block(&la);
        let na = f.call("i64", "jv_arr_len", &[(V, "%a")]);
        let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
        let n = f.add(&na, &nb);
        let r = f.call(V, "jv_arr", &[("i64", &n)]);
        let rv = f.var_init(V, &r);
        for (src, cnt) in [("%a", &na), ("%b", &nb)] {
            f.for_range("0", cnt, |f, i, _, _| {
                let e = f.call(V, "jv_arr_get", &[(V, src), ("i64", i)]);
                let x = f.load(V, &rv);
                let y = f.call(V, "jv_arr_push", &[(V, &x), (V, &e)]);
                f.store(V, &y, &rv);
            });
        }
        let r = f.load(V, &rv);
        f.ret(V, &r);
        f.block(&lo);
        f.call("void", "jv_retain", &[(V, "%a")]);
        let rv = f.var_init(V, "%a");
        let nb = f.call("i64", "jv_obj_len", &[(V, "%b")]);
        f.for_range("0", &nb, |f, i, _, _| {
            let k = f.call(V, "jv_obj_key_at", &[(V, "%b"), ("i64", i)]);
            let v = f.call(V, "jv_obj_val_at", &[(V, "%b"), ("i64", i)]);
            let x = f.load(V, &rv);
            let y = f.call(V, "jv_obj_set", &[(V, &x), (V, &k), (V, &v)]);
            f.store(V, &y, &rv);
        });
        let r = f.load(V, &rv);
        f.ret(V, &r);
        f.block(&ld);
    });
    ret_err2(m, &mut f, "%a", "%b", "cannot be added");
    m.add(f);

    // consuming add: a is owned (used for reduce accumulators)
    let mut f = Func::new("internal", V, "jv_add_c", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let same = f.eq(&ta, &tb);
    let iss = f.eq(&ta, "4");
    let isa = f.eq(&ta, "5");
    let iso = f.eq(&ta, "6");
    let s1 = f.and1(&same, &iss);
    f.if_then(&s1, |f| {
        let r = f.call(V, "jv_str_append_v", &[(V, "%a"), (V, "%b")]);
        f.ret(V, &r);
    });
    let s2 = f.and1(&same, &isa);
    f.if_then(&s2, |f| {
        let rv = f.var_init(V, "%a");
        let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
        f.for_range("0", &nb, |f, i, _, _| {
            let e = f.call(V, "jv_arr_get", &[(V, "%b"), ("i64", i)]);
            let x = f.load(V, &rv);
            let y = f.call(V, "jv_arr_push", &[(V, &x), (V, &e)]);
            f.store(V, &y, &rv);
        });
        let r = f.load(V, &rv);
        f.ret(V, &r);
    });
    let s3 = f.and1(&same, &iso);
    f.if_then(&s3, |f| {
        let rv = f.var_init(V, "%a");
        let nb = f.call("i64", "jv_obj_len", &[(V, "%b")]);
        f.for_range("0", &nb, |f, i, _, _| {
            let k = f.call(V, "jv_obj_key_at", &[(V, "%b"), ("i64", i)]);
            let v = f.call(V, "jv_obj_val_at", &[(V, "%b"), ("i64", i)]);
            let x = f.load(V, &rv);
            let y = f.call(V, "jv_obj_set", &[(V, &x), (V, &k), (V, &v)]);
            f.store(V, &y, &rv);
        });
        let r = f.load(V, &rv);
        f.ret(V, &r);
    });
    let r = f.call(V, "jv_add", &[(V, "%a"), (V, "%b")]);
    f.call("void", "jv_release", &[(V, "%a")]);
    f.ret(V, &r);
    m.add(f);

    // ---- sub ----
    let mut f = Func::new("internal", V, "jv_sub", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let same = f.eq(&ta, &tb);
    let isn = f.eq(&ta, "3");
    let isa = f.eq(&ta, "5");
    let nn = f.and1(&same, &isn);
    f.if_then(&nn, |f| {
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let z = f.bin("fsub", "double", &x, &y);
        let r = f.mknum(&z);
        f.ret(V, &r);
    });
    let aa = f.and1(&same, &isa);
    f.if_then(&aa, |f| {
        let na = f.call("i64", "jv_arr_len", &[(V, "%a")]);
        let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
        let r = f.call(V, "jv_arr", &[("i64", &na)]);
        let rv = f.var_init(V, &r);
        f.for_range("0", &na, |f, i, _, _| {
            let e = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]);
            let found = f.var_init("i1", "false");
            f.for_range("0", &nb, |f, j, brk, _| {
                let x = f.call(V, "jv_arr_get", &[(V, "%b"), ("i64", j)]);
                let q = f.call("i1", "jv_eq", &[(V, &e), (V, &x)]);
                f.if_then(&q, |f| {
                    f.store("i1", "true", &found);
                    f.br(brk);
                });
            });
            let fd = f.load("i1", &found);
            let nf = f.not1(&fd);
            f.if_then(&nf, |f| {
                let x = f.load(V, &rv);
                let y = f.call(V, "jv_arr_push", &[(V, &x), (V, &e)]);
                f.store(V, &y, &rv);
            });
        });
        let r = f.load(V, &rv);
        f.ret(V, &r);
    });
    ret_err2(m, &mut f, "%a", "%b", "cannot be subtracted");
    m.add(f);

    // ---- mul ----
    let mut f = Func::new("internal", V, "jv_mul", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let an = f.eq(&ta, "3");
    let bn = f.eq(&tb, "3");
    let nn = f.and1(&an, &bn);
    f.if_then(&nn, |f| {
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let z = f.bin("fmul", "double", &x, &y);
        let r = f.mknum(&z);
        f.ret(V, &r);
    });
    let as_ = f.eq(&ta, "4");
    let bs = f.eq(&tb, "4");
    let sn = f.and1(&as_, &bn);
    let ns = f.and1(&an, &bs);
    let rep = f.or1(&sn, &ns);
    f.if_then(&rep, |f| {
        let s = f.select(&sn, V, "%a", "%b");
        let nv = f.select(&sn, V, "%b", "%a");
        let d = f.num_of(&nv);
        // negative or NaN count: null
        let ok = f.fcmp("oge", &d, "0.0");
        let neg = f.not1(&ok);
        f.if_then(&neg, |f| f.ret(V, NULL));
        let len = f.call("i64", "jv_str_len", &[(V, &s)]);
        let empty = f.eq(&len, "0");
        f.if_then(&empty, |f| {
            let e = f.call(V, "jv_str_alloc", &[("i64", "0")]);
            f.ret(V, &e);
        });
        let lend = f.cast("sitofp", "i64", &len, "double");
        let fl = f.call("double", "floor", &[("double", &d)]);
        let totd = f.bin("fmul", "double", &lend, &fl);
        let toolong = f.fcmp("ogt", &totd, "2147483647.0");
        f.if_then(&toolong, |f| ret_errc(m, f, "Repeat string result too long"));
        let cnt = f.cast("fptosi", "double", &fl, "i64");
        let tot = f.bin("mul", "i64", &len, &cnt);
        let r = f.call(V, "jv_str_alloc", &[("i64", &tot)]);
        let rv = f.var_init(V, &r);
        let sp = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
        f.for_range("0", &cnt, |f, _, _, _| {
            let x = f.load(V, &rv);
            let y = f.call(V, "jv_str_append", &[(V, &x), ("ptr", &sp), ("i64", &len)]);
            f.store(V, &y, &rv);
        });
        let r = f.load(V, &rv);
        f.ret(V, &r);
    });
    let ao = f.eq(&ta, "6");
    let bo = f.eq(&tb, "6");
    let oo = f.and1(&ao, &bo);
    f.if_then(&oo, |f| {
        let r = f.call(V, "jv_obj_merge_deep", &[(V, "%a"), (V, "%b")]);
        f.ret(V, &r);
    });
    ret_err2(m, &mut f, "%a", "%b", "cannot be multiplied");
    m.add(f);

    let mut f = Func::new("internal", V, "jv_obj_merge_deep", &[(V, "a"), (V, "b")], "");
    f.call("void", "jv_retain", &[(V, "%a")]);
    let rv = f.var_init(V, "%a");
    let nb = f.call("i64", "jv_obj_len", &[(V, "%b")]);
    f.for_range("0", &nb, |f, i, _, _| {
        let k = f.call(V, "jv_obj_key_at", &[(V, "%b"), ("i64", i)]);
        let v = f.call(V, "jv_obj_val_at", &[(V, "%b"), ("i64", i)]);
        let x = f.load(V, &rv);
        let old = f.call(V, "jv_obj_get", &[(V, &x), (V, &k)]);
        let o1 = f.tag_is(&old, T_OBJ);
        let o2 = f.tag_is(&v, T_OBJ);
        let both = f.and1(&o1, &o2);
        f.if_else(
            &both,
            |f| {
                let mv = f.call(V, "jv_obj_merge_deep", &[(V, &old), (V, &v)]);
                let y = f.call(V, "jv_obj_set", &[(V, &x), (V, &k), (V, &mv)]);
                f.call("void", "jv_release", &[(V, &mv)]);
                f.store(V, &y, &rv);
            },
            |f| {
                let y = f.call(V, "jv_obj_set", &[(V, &x), (V, &k), (V, &v)]);
                f.store(V, &y, &rv);
            },
        );
    });
    let r = f.load(V, &rv);
    f.ret(V, &r);
    m.add(f);

    // ---- div ----
    let mut f = Func::new("internal", V, "jv_div", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let an = f.eq(&ta, "3");
    let bn = f.eq(&tb, "3");
    let nn = f.and1(&an, &bn);
    f.if_then(&nn, |f| {
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let z0 = f.fcmp("oeq", &y, "0.0");
        f.if_then(&z0, |f| ret_err2(m, f, "%a", "%b", "cannot be divided because the divisor is zero"));
        let z = f.bin("fdiv", "double", &x, &y);
        let r = f.mknum(&z);
        f.ret(V, &r);
    });
    let as_ = f.eq(&ta, "4");
    let bs = f.eq(&tb, "4");
    let ss = f.and1(&as_, &bs);
    f.if_then(&ss, |f| {
        let r = f.call(V, "jv_split", &[(V, "%a"), (V, "%b")]);
        f.ret(V, &r);
    });
    ret_err2(m, &mut f, "%a", "%b", "cannot be divided");
    m.add(f);

    // ---- mod ----
    let mut f = Func::new("internal", V, "jv_mod", &[(V, "a"), (V, "b")], "");
    let ta = f.tag("%a");
    let tb = f.tag("%b");
    let an = f.eq(&ta, "3");
    let bn = f.eq(&tb, "3");
    let nn = f.and1(&an, &bn);
    f.if_then(&nn, |f| {
        let x = f.num_of("%a");
        let y = f.num_of("%b");
        let xn = f.fcmp("uno", &x, &y);
        f.if_then(&xn, |f| {
            let r = f.mknum(&fconst(f64::NAN));
            f.ret(V, &r);
        });
        let xi = f.call("i64", "jv_dtoi", &[("double", &x)]);
        let yi = f.call("i64", "jv_dtoi", &[("double", &y)]);
        let z = f.eq(&yi, "0");
        f.if_then(&z, |f| ret_err2(m, f, "%a", "%b", "cannot be divided (remainder) because the divisor is zero"));
        // avoid INT_MIN % -1 trap
        let m1 = f.eq(&yi, "-1");
        let yi2 = f.select(&m1, "i64", "1", &yi);
        let r = f.bin("srem", "i64", &xi, &yi2);
        let d = f.cast("sitofp", "i64", &r, "double");
        let v = f.mknum(&d);
        f.ret(V, &v);
    });
    ret_err2(m, &mut f, "%a", "%b", "cannot be divided");
    m.add(f);

    // saturating double -> i64
    let mut f = Func::new("internal", "i64", "jv_dtoi", &[("double", "d")], "");
    let hi = f.fcmp("oge", "%d", &fconst(9.2233720368547758e18));
    f.if_then(&hi, |f| f.ret("i64", "9223372036854775807"));
    let lo = f.fcmp("ole", "%d", &fconst(-9.2233720368547758e18));
    f.if_then(&lo, |f| f.ret("i64", "-9223372036854775808"));
    let nan = f.fcmp("uno", "%d", "%d");
    f.if_then(&nan, |f| f.ret("i64", "0"));
    let r = f.cast("fptosi", "double", "%d", "i64");
    f.ret("i64", &r);
    m.add(f);

    // ---- negate ----
    let mut f = Func::new("internal", V, "jv_neg", &[(V, "a")], "");
    let isn = f.tag_is("%a", T_NUM);
    f.if_then(&isn, |f| {
        let raw = f.rawtag("%a");
        let lit = f.icmp("ugt", "i64", &raw, "255");
        f.if_then(&lit, |f| {
            let r = f.call(V, "jv_lit_neg", &[(V, "%a")]);
            f.ret(V, &r);
        });
        let x = f.num_of("%a");
        let y = f.ins(format!("fneg double {}", x));
        let r = f.mknum(&y);
        f.ret(V, &r);
    });
    ret_err1(m, &mut f, "%a", "cannot be negated");
    m.add(f);
}

fn index(m: &mut Module) {
    // integer index into array (borrowed array) -> owned value or null
    let mut f = Func::new("internal", V, "jv_arr_index", &[(V, "a"), ("double", "d")], "");
    let isnan = f.fcmp("uno", "%d", "%d");
    f.if_then(&isnan, |f| f.ret(V, NULL));
    let fl = f.call("double", "floor", &[("double", "%d")]);
    let i0 = f.call("i64", "jv_dtoi", &[("double", &fl)]);
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let neg = f.lt(&i0, "0");
    let i1 = f.add(&i0, &n);
    let i = f.select(&neg, "i64", &i1, &i0);
    let lo = f.lt(&i, "0");
    let hi = f.icmp("sge", "i64", &i, &n);
    let out = f.or1(&lo, &hi);
    f.if_then(&out, |f| f.ret(V, NULL));
    let v = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", &i)]);
    f.call("void", "jv_retain", &[(V, &v)]);
    f.ret(V, &v);
    m.add(f);

    let s_start = m.cstr("start");
    let s_end = m.cstr("end");
    let mut f = Func::new("internal", V, "jv_index", &[(V, "t"), (V, "k")], "");
    let tt = f.tag("%t");
    let kt = f.tag("%k");
    let tobj = f.eq(&tt, "6");
    let kstr = f.eq(&kt, "4");
    let os = f.and1(&tobj, &kstr);
    f.if_then(&os, |f| {
        let i = f.call("i64", "jv_obj_find", &[(V, "%t"), (V, "%k")]);
        let miss = f.lt(&i, "0");
        f.if_then(&miss, |f| f.ret(V, NULL));
        let v = f.call(V, "jv_obj_val_at", &[(V, "%t"), ("i64", &i)]);
        f.call("void", "jv_retain", &[(V, &v)]);
        f.ret(V, &v);
    });
    let tarr = f.eq(&tt, "5");
    let knum = f.eq(&kt, "3");
    let an = f.and1(&tarr, &knum);
    f.if_then(&an, |f| {
        let d = f.num_of("%k");
        let r = f.call(V, "jv_arr_index", &[(V, "%t"), ("double", &d)]);
        f.ret(V, &r);
    });
    let tnull = f.eq(&tt, "0");
    let kobj = f.eq(&kt, "6");
    let knull = f.eq(&kt, "0");
    let ok1 = f.or1(&kstr, &knum);
    let ok2 = f.or1(&kobj, &knull);
    let okk = f.or1(&ok1, &ok2);
    let nk = f.and1(&tnull, &okk);
    f.if_then(&nk, |f| f.ret(V, NULL));
    // slice via object {start, end}
    let tstr = f.eq(&tt, "4");
    let sl = f.or1(&tarr, &tstr);
    let slk = f.and1(&sl, &kobj);
    f.if_then(&slk, |f| {
        let ks = f.call(V, "jv_cstr", &[("ptr", &s_start)]);
        let ke = f.call(V, "jv_cstr", &[("ptr", &s_end)]);
        let a = f.call(V, "jv_obj_get", &[(V, "%k"), (V, &ks)]);
        let b = f.call(V, "jv_obj_get", &[(V, "%k"), (V, &ke)]);
        f.call("void", "jv_release", &[(V, &ks)]);
        f.call("void", "jv_release", &[(V, &ke)]);
        let am = f.tag_is(&a, T_EOF);
        let bm = f.tag_is(&b, T_EOF);
        let a2 = f.select(&am, V, NULL, &a);
        let b2 = f.select(&bm, V, NULL, &b);
        let r = f.call(V, "jv_slice", &[(V, "%t"), (V, &a2), (V, &b2)]);
        f.ret(V, &r);
    });
    let karr = f.eq(&kt, "5");
    let aa = f.and1(&tarr, &karr);
    f.if_then(&aa, |f| {
        let r = f.call(V, "jv_indices", &[(V, "%t"), (V, "%k")]);
        f.ret(V, &r);
    });
    let e = f.call(V, "jv_index_err", &[(V, "%t"), (V, "%k")]);
    f.ret(V, &e);
    m.add(f);

    // resolve slice bounds: writes start/end (clamped) for length n
    let mut f = Func::new(
        "internal",
        "i1",
        "jv_slice_bounds",
        &[(V, "from"), (V, "to"), ("i64", "n"), ("ptr", "ps"), ("ptr", "pe")],
        "",
    );
    let nd = f.cast("sitofp", "i64", "%n", "double");
    let fnull = f.tag_is("%from", T_NULL);
    let tnull = f.tag_is("%to", T_NULL);
    let fnum = f.tag_is("%from", T_NUM);
    let tnum = f.tag_is("%to", T_NUM);
    let fok = f.or1(&fnull, &fnum);
    let tok = f.or1(&tnull, &tnum);
    let ok = f.and1(&fok, &tok);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| f.ret("i1", "false"));
    let fv = f.num_of("%from");
    let tv = f.num_of("%to");
    // null or NaN bounds mean "from the start" / "to the end"
    let fnan = f.fcmp("uno", &fv, &fv);
    let tnan = f.fcmp("uno", &tv, &tv);
    let fdef = f.or1(&fnull, &fnan);
    let tdef = f.or1(&tnull, &tnan);
    let s0 = f.select(&fdef, "double", "0.0", &fv);
    let e0 = f.select(&tdef, "double", &nd, &tv);
    let sneg = f.fcmp("olt", &s0, "0.0");
    let s1a = f.bin("fadd", "double", &s0, &nd);
    let s1 = f.select(&sneg, "double", &s1a, &s0);
    let eneg = f.fcmp("olt", &e0, "0.0");
    let e1a = f.bin("fadd", "double", &e0, &nd);
    let e1 = f.select(&eneg, "double", &e1a, &e0);
    let slo = f.fcmp("olt", &s1, "0.0");
    let s2 = f.select(&slo, "double", "0.0", &s1);
    let shi = f.fcmp("ogt", &s2, &nd);
    let s3 = f.select(&shi, "double", &nd, &s2);
    let ehi = f.fcmp("ogt", &e1, &nd);
    let e2 = f.select(&ehi, "double", &nd, &e1);
    let eneg2 = f.fcmp("olt", &e2, "0.0");
    let e3 = f.select(&eneg2, "double", "0.0", &e2);
    let sf = f.call("double", "floor", &[("double", &s3)]);
    let ec = f.call("double", "ceil", &[("double", &e3)]);
    let si = f.cast("fptosi", "double", &sf, "i64");
    let ei = f.cast("fptosi", "double", &ec, "i64");
    let ehi2 = f.icmp("sgt", "i64", &ei, "%n");
    let ei1 = f.select(&ehi2, "i64", "%n", &ei);
    // an end before the start means an empty slice
    let elo = f.icmp("slt", "i64", &ei1, &si);
    let ei2 = f.select(&elo, "i64", &si, &ei1);
    f.store("i64", &si, "%ps");
    f.store("i64", &ei2, "%pe");
    f.ret("i1", "true");
    m.add(f);

    let mut f = Func::new("internal", V, "jv_slice", &[(V, "t"), (V, "from"), (V, "to")], "");
    let tt = f.tag("%t");
    let tnull = f.eq(&tt, "0");
    f.if_then(&tnull, |f| f.ret(V, NULL));
    let ps = f.var("i64");
    let pe = f.var("i64");
    let tarr = f.eq(&tt, "5");
    f.if_then(&tarr, |f| {
        let n = f.call("i64", "jv_arr_len", &[(V, "%t")]);
        let ok = f.call("i1", "jv_slice_bounds", &[(V, "%from"), (V, "%to"), ("i64", &n), ("ptr", &ps), ("ptr", &pe)]);
        let bad = f.not1(&ok);
        f.if_then(&bad, |f| ret_errc(m, f, "Start and end indices of an array slice must be numbers"));
        let s = f.load("i64", &ps);
        let e = f.load("i64", &pe);
        let r = f.call(V, "jv_arr_sub", &[(V, "%t"), ("i64", &s), ("i64", &e)]);
        f.ret(V, &r);
    });
    let tstr = f.eq(&tt, "4");
    f.if_then(&tstr, |f| {
        let d = f.call("ptr", "jv_str_ptr", &[(V, "%t")]);
        let bl = f.call("i64", "jv_str_len", &[(V, "%t")]);
        let n = f.call("i64", "utf8_count", &[("ptr", &d), ("i64", &bl)]);
        let ok = f.call("i1", "jv_slice_bounds", &[(V, "%from"), (V, "%to"), ("i64", &n), ("ptr", &ps), ("ptr", &pe)]);
        let bad = f.not1(&ok);
        f.if_then(&bad, |f| ret_errc(m, f, "Start and end indices of an string slice must be numbers"));
        let s = f.load("i64", &ps);
        let e = f.load("i64", &pe);
        let bs = f.call("i64", "utf8_offset", &[("ptr", &d), ("i64", &bl), ("i64", &s)]);
        let be = f.call("i64", "utf8_offset", &[("ptr", &d), ("i64", &bl), ("i64", &e)]);
        let p = f.off(&d, &bs);
        let len = f.sub(&be, &bs);
        let r = f.call(V, "jv_str", &[("ptr", &p), ("i64", &len)]);
        f.ret(V, &r);
    });
    let ko = f.call(V, "jv_obj", &[]);
    let e = f.call(V, "jv_index_err", &[(V, "%t"), (V, &ko)]);
    f.call("void", "jv_release", &[(V, &ko)]);
    f.ret(V, &e);
    m.add(f);

    // byte offset of the cp-th code point
    let mut f = Func::new("internal", "i64", "utf8_offset", &[("ptr", "d"), ("i64", "n"), ("i64", "cp")], "");
    let cnt = f.var_init("i64", "0");
    f.for_range("0", "%n", |f, i, _, _| {
        let q = f.off("%d", i);
        let b = f.load("i8", &q);
        let c = f.cast("zext", "i8", &b, "i64");
        let top = f.bin("and", "i64", &c, "192");
        let cont = f.eq(&top, "128");
        let notc = f.not1(&cont);
        f.if_then(&notc, |f| {
            let k = f.load("i64", &cnt);
            let hit = f.eq(&k, "%cp");
            f.if_then(&hit, |f| f.ret("i64", i));
            let k1 = f.add(&k, "1");
            f.store("i64", &k1, &cnt);
        });
    });
    f.ret("i64", "%n");
    m.add(f);
}

fn misc(m: &mut Module) {
    // length
    let mut f = Func::new("internal", V, "jv_length", &[(V, "a")], "");
    let t = f.tag("%a");
    let (ln, lnum, ls, la, lo, ld) = (f.label("n"), f.label("num"), f.label("s"), f.label("a"), f.label("o"), f.label("d"));
    f.switch(&t, &ld, &[(0, ln.clone()), (3, lnum.clone()), (4, ls.clone()), (5, la.clone()), (6, lo.clone())]);
    f.block(&ln);
    let z = numconst(0.0);
    f.ret(V, &z);
    f.block(&lnum);
    let d = f.num_of("%a");
    let raw = f.rawtag("%a");
    let lit = f.icmp("ugt", "i64", &raw, "255");
    f.if_then(&lit, |f| {
        let neg = f.fcmp("olt", &d, "0.0");
        f.if_then(&neg, |f| {
            let r = f.call(V, "jv_lit_neg", &[(V, "%a")]);
            f.ret(V, &r);
        });
        f.call("void", "jv_retain", &[(V, "%a")]);
        f.ret(V, "%a");
    });
    let a = f.call("double", "llvm.fabs.f64", &[("double", &d)]);
    let r = f.mknum(&a);
    f.ret(V, &r);
    f.block(&ls);
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%a")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%a")]);
    let c = f.call("i64", "utf8_count", &[("ptr", &d), ("i64", &n)]);
    let cd = f.cast("sitofp", "i64", &c, "double");
    let r = f.mknum(&cd);
    f.ret(V, &r);
    f.block(&la);
    let n = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let cd = f.cast("sitofp", "i64", &n, "double");
    let r = f.mknum(&cd);
    f.ret(V, &r);
    f.block(&lo);
    let n = f.call("i64", "jv_obj_len", &[(V, "%a")]);
    let cd = f.cast("sitofp", "i64", &n, "double");
    let r = f.mknum(&cd);
    f.ret(V, &r);
    f.block(&ld);
    ret_err1(m, &mut f, "%a", "has no length");
    m.add(f);

    // split(str, sep) -> array of strings (literal separator)
    let mut f = Func::new("internal", V, "jv_split", &[(V, "s"), (V, "sep")], "");
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%s")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%s")]);
    let sd = f.call("ptr", "jv_str_ptr", &[(V, "%sep")]);
    let sn = f.call("i64", "jv_str_len", &[(V, "%sep")]);
    let r0 = f.call(V, "jv_arr", &[("i64", "4")]);
    let rv = f.var_init(V, &r0);
    let empty = f.eq(&n, "0");
    f.if_then(&empty, |f| f.ret(V, &r0));
    let sepempty = f.eq(&sn, "0");
    f.if_then(&sepempty, |f| {
        // split into code points
        let iv = f.var_init("i64", "0");
        let lenv = f.var("i64");
        f.while_(
            |f| {
                let i = f.load("i64", &iv);
                f.lt(&i, &n)
            },
            |f, _, _| {
                let i = f.load("i64", &iv);
                f.call("i64", "utf8_decode", &[("ptr", &d), ("i64", &n), ("i64", &i), ("ptr", &lenv)]);
                let l = f.load("i64", &lenv);
                let p = f.off(&d, &i);
                let s = f.call(V, "jv_str", &[("ptr", &p), ("i64", &l)]);
                let a = f.load(V, &rv);
                let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &s)]);
                f.call("void", "jv_release", &[(V, &s)]);
                f.store(V, &a2, &rv);
                let i2 = f.add(&i, &l);
                f.store("i64", &i2, &iv);
            },
        );
        let a = f.load(V, &rv);
        f.ret(V, &a);
    });
    let start = f.var_init("i64", "0");
    let iv = f.var_init("i64", "0");
    let lim = f.sub(&n, &sn);
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.icmp("sle", "i64", &i, &lim)
        },
        |f, _, cont| {
            let i = f.load("i64", &iv);
            let p = f.off(&d, &i);
            let c = f.call("i32", "memcmp", &[("ptr", &p), ("ptr", &sd), ("i64", &sn)]);
            let hit = f.icmp("eq", "i32", &c, "0");
            let nohit = f.not1(&hit);
            f.if_then(&nohit, |f| {
                let i1 = f.add(&i, "1");
                f.store("i64", &i1, &iv);
                f.br(cont);
            });
            let st = f.load("i64", &start);
            let sp = f.off(&d, &st);
            let len = f.sub(&i, &st);
            let s = f.call(V, "jv_str", &[("ptr", &sp), ("i64", &len)]);
            let a = f.load(V, &rv);
            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &s)]);
            f.call("void", "jv_release", &[(V, &s)]);
            f.store(V, &a2, &rv);
            let ni = f.add(&i, &sn);
            f.store("i64", &ni, &iv);
            f.store("i64", &ni, &start);
        },
    );
    let st = f.load("i64", &start);
    let sp = f.off(&d, &st);
    let len = f.sub(&n, &st);
    let s = f.call(V, "jv_str", &[("ptr", &sp), ("i64", &len)]);
    let a = f.load(V, &rv);
    let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &s)]);
    f.call("void", "jv_release", &[(V, &s)]);
    f.ret(V, &a2);
    m.add(f);

    // indices of sub-array b in a (or of string in string handled in builtins)
    let mut f = Func::new("internal", V, "jv_indices", &[(V, "a"), (V, "b")], "");
    let na = f.call("i64", "jv_arr_len", &[(V, "%a")]);
    let nb = f.call("i64", "jv_arr_len", &[(V, "%b")]);
    let r0 = f.call(V, "jv_arr", &[("i64", "4")]);
    let rv = f.var_init(V, &r0);
    let emptyb = f.eq(&nb, "0");
    f.if_then(&emptyb, |f| {
        f.call("void", "jv_release", &[(V, &r0)]);
        f.ret(V, NULL);
    });
    let lim0 = f.sub(&na, &nb);
    let lim = f.add(&lim0, "1");
    f.for_range("0", &lim, |f, i, _, _| {
        let all = f.var_init("i1", "true");
        f.for_range("0", &nb, |f, j, brk, _| {
            let ij = f.add(i, j);
            let x = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", &ij)]);
            let y = f.call(V, "jv_arr_get", &[(V, "%b"), ("i64", j)]);
            let e = f.call("i1", "jv_eq", &[(V, &x), (V, &y)]);
            let ne = f.not1(&e);
            f.if_then(&ne, |f| {
                f.store("i1", "false", &all);
                f.br(brk);
            });
        });
        let a = f.load("i1", &all);
        f.if_then(&a, |f| {
            let d = f.cast("sitofp", "i64", i, "double");
            let x = f.mknum(&d);
            let r = f.load(V, &rv);
            let r2 = f.call(V, "jv_arr_push", &[(V, &r), (V, &x)]);
            f.store(V, &r2, &rv);
        });
    });
    let r = f.load(V, &rv);
    f.ret(V, &r);
    m.add(f);
}
