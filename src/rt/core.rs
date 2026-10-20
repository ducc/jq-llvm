//! Memory management, strings, arrays, objects, errors, string builders.

use super::*;
use crate::ir::{Func, Module, V};

pub fn emit(m: &mut Module) {
    // Heap layouts (byte offsets):
    //   string: rc@0 len@8 cap@16 hash@24 data@32 (cap+1 bytes, NUL terminated)
    //   array:  rc@0 len@8 cap@16 items@24 (ptr to %V[cap])
    //   object: rc@0 len@8 cap@16 entries@24 index@32 icap@40
    //           entry (32 bytes): keyptr@0 hash@8 val@16
    //           index: i32[icap], entry index + 1, 0 = empty. Only when len > 8.
    //   error:  rc@0 msg@8 (%V)
    //   bypass: rc@0 id@8 inner@16 (%V)
    malloc(m);
    freelists(m);
    retain_release(m);
    lit_refcount(m);
    release_cold(m);
    free_fn(m);
    strings(m);
    arrays(m);
    objects(m);
    errors(m);
    sb(m);
}

fn malloc(m: &mut Module) {
    let oom = m.cstr("jq: error: cannot allocate memory\n");
    let mut f = Func::new("internal", "ptr", "jv_malloc", &[("i64", "n")], "");
    let p = f.call("ptr", "malloc", &[("i64", "%n")]);
    let isnull = f.icmp("eq", "ptr", &p, "null");
    f.if_then(&isnull, |f| {
        let e = f.load("ptr", "@stderr");
        f.call("i64", "fwrite", &[("ptr", &oom), ("i64", "1"), ("i64", "34"), ("ptr", &e)]);
        f.call("void", "abort", &[]);
        f.unreachable();
    });
    f.ret("ptr", &p);
    m.add(f);

    let mut f = Func::new("internal", "ptr", "jv_realloc", &[("ptr", "p"), ("i64", "n")], "");
    let q = f.call("ptr", "realloc", &[("ptr", "%p"), ("i64", "%n")]);
    let isnull = f.icmp("eq", "ptr", &q, "null");
    f.if_then(&isnull, |f| {
        f.call("void", "abort", &[]);
        f.unreachable();
    });
    f.ret("ptr", &q);
    m.add(f);
}

fn retain_release(m: &mut Module) {
    // Heap kinds have a plain tag (4..=8). A number whose tag word has bits
    // above the low byte carries a refcounted literal-text object (rc@0).
    let mut f = Func::new("internal", "void", "jv_retain", &[(V, "x")], "alwaysinline");
    let t = f.rawtag("%x");
    let d = f.sub(&t, "4");
    let c = f.icmp("ult", "i64", &d, "5");
    f.if_else(
        &c,
        |f| {
            let p = f.ptr_of("%x");
            let rc = f.load("i64", &p);
            let n = f.add(&rc, "1");
            f.store("i64", &n, &p);
        },
        |f| {
            let lit = f.icmp("ugt", "i64", &t, "255");
            f.if_then(&lit, |f| f.call("void", "jv_lit_retain", &[("i64", &t)]));
        },
    );
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "jv_release", &[(V, "x")], "alwaysinline");
    let t = f.rawtag("%x");
    let d = f.sub(&t, "4");
    let c = f.icmp("ult", "i64", &d, "5");
    f.if_else(
        &c,
        |f| {
            let p = f.ptr_of("%x");
            let rc = f.load("i64", &p);
            let n = f.sub(&rc, "1");
            let z = f.eq(&n, "0");
            f.if_else(
                &z,
                |f| {
                    f.call("void", "jv_free", &[(V, "%x")]);
                },
                |f| f.store("i64", &n, &p),
            );
        },
        |f| {
            let lit = f.icmp("ugt", "i64", &t, "255");
            f.if_then(&lit, |f| f.call("void", "jv_lit_release", &[("i64", &t)]));
        },
    );
    f.ret_void();
    m.add(f);

    // copy: retain and return (convenience)
    let mut f = Func::new("internal", V, "jv_copy", &[(V, "x")], "alwaysinline");
    f.call("void", "jv_retain", &[(V, "%x")]);
    f.ret(V, "%x");
    m.add(f);
}

fn free_fn(m: &mut Module) {
    let mut f = Func::new("internal", "void", "jv_free", &[(V, "x")], "noinline");
    let t = f.tag("%x");
    let p = f.ptr_of("%x");
    let (ls, la, lo, le, lb, ld) = (
        f.label("str"),
        f.label("arr"),
        f.label("obj"),
        f.label("err"),
        f.label("byp"),
        f.label("done"),
    );
    f.switch(
        &t,
        &ld,
        &[(4, ls.clone()), (5, la.clone()), (6, lo.clone()), (7, le.clone()), (8, lb.clone())],
    );
    f.block(&ls);
    let scap = f.ld_at("i64", &p, 16);
    let small = f.eq(&scap, "31");
    f.if_else(&small, |f| f.call("void", "jv_fl_free", &[("ptr", "@g_fl64"), ("ptr", &p)]), |f| f.call("void", "free", &[("ptr", &p)]));
    f.br(&ld);

    f.block(&la);
    let n = f.ld_at("i64", &p, 8);
    let items = f.ld_at("ptr", &p, 24);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.idx(&items, i, 16);
        let v = f.load(V, &e);
        f.call("void", "jv_release", &[(V, &v)]);
    });
    f.call("void", "free", &[("ptr", &items)]);
    f.call("void", "jv_fl_free", &[("ptr", "@g_fl32"), ("ptr", &p)]);
    f.br(&ld);

    f.block(&lo);
    let n = f.ld_at("i64", &p, 8);
    let ents = f.ld_at("ptr", &p, 24);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.idx(&ents, i, 32);
        let k = f.load("ptr", &e);
        let kv = f.mkptr("4", &k);
        f.call("void", "jv_release", &[(V, &kv)]);
        let v = f.ld_at(V, &e, 16);
        f.call("void", "jv_release", &[(V, &v)]);
    });
    f.call("void", "free", &[("ptr", &ents)]);
    let idx = f.ld_at("ptr", &p, 32);
    f.call("void", "free", &[("ptr", &idx)]);
    f.call("void", "jv_fl_free", &[("ptr", "@g_fl48"), ("ptr", &p)]);
    f.br(&ld);

    f.block(&le);
    let v = f.ld_at(V, &p, 8);
    f.call("void", "jv_release", &[(V, &v)]);
    f.call("void", "jv_fl_free", &[("ptr", "@g_fl32"), ("ptr", &p)]);
    f.br(&ld);

    f.block(&lb);
    let v = f.ld_at(V, &p, 16);
    f.call("void", "jv_release", &[(V, &v)]);
    f.call("void", "jv_fl_free", &[("ptr", "@g_fl32"), ("ptr", &p)]);
    f.br(&ld);

    f.block(&ld);
    f.ret_void();
    m.add(f);
}

fn strings(m: &mut Module) {
    // jv_str_alloc(cap) -> empty string with capacity
    let mut f = Func::new("internal", V, "jv_str_alloc", &[("i64", "cap")], "");
    // short strings (cap <= 31) come from the 64-byte free list
    let small = f.icmp("sle", "i64", "%cap", "31");
    let sz = f.add("%cap", "33");
    let p = f.if_val(&small, "ptr", |f| f.call("ptr", "jv_fl_alloc", &[("ptr", "@g_fl64"), ("i64", "64")]), |f| f.call("ptr", "jv_malloc", &[("i64", &sz)]));
    let cap = f.select(&small, "i64", "31", "%cap");
    f.st_at("i64", "1", &p, 0);
    f.st_at("i64", "0", &p, 8);
    f.st_at("i64", &cap, &p, 16);
    f.st_at("i64", "0", &p, 24);
    f.st_at("i8", "0", &p, 32);
    let v = f.mkptr("4", &p);
    f.ret(V, &v);
    m.add(f);

    // jv_str(data, len) -> new string
    let mut f = Func::new("internal", V, "jv_str", &[("ptr", "d"), ("i64", "n")], "");
    let v = f.call(V, "jv_str_alloc", &[("i64", "%n")]);
    let p = f.ptr_of(&v);
    let dst = f.off(&p, "32");
    f.call("ptr", "memcpy", &[("ptr", &dst), ("ptr", "%d"), ("i64", "%n")]);
    f.st_at("i64", "%n", &p, 8);
    let end = f.off(&dst, "%n");
    f.store("i8", "0", &end);
    f.ret(V, &v);
    m.add(f);

    // jv_cstr(cstr)
    let mut f = Func::new("internal", V, "jv_cstr", &[("ptr", "s")], "");
    let n = f.call("i64", "strlen", &[("ptr", "%s")]);
    let v = f.call(V, "jv_str", &[("ptr", "%s"), ("i64", &n)]);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", "ptr", "jv_str_ptr", &[(V, "s")], "alwaysinline");
    let p = f.ptr_of("%s");
    let d = f.off(&p, "32");
    f.ret("ptr", &d);
    m.add(f);

    let mut f = Func::new("internal", "i64", "jv_str_len", &[(V, "s")], "alwaysinline");
    let p = f.ptr_of("%s");
    let n = f.ld_at("i64", &p, 8);
    f.ret("i64", &n);
    m.add(f);

    // hash (cached). FNV-1a over 8-byte chunks + tail.
    let mut f = Func::new("internal", "i64", "jv_str_hash", &[(V, "s")], "");
    let p = f.ptr_of("%s");
    let h = f.ld_at("i64", &p, 24);
    let z = f.eq(&h, "0");
    f.if_then(&z, |f| {
        let n = f.ld_at("i64", &p, 8);
        let d = f.off(&p, "32");
        let h3 = f.call("i64", "jv_hash_bytes", &[("ptr", &d), ("i64", &n)]);
        f.st_at("i64", &h3, &p, 24);
    });
    let h = f.ld_at("i64", &p, 24);
    f.ret("i64", &h);
    m.add(f);

    let mut f = Func::new("internal", "i64", "jv_hash_bytes", &[("ptr", "d"), ("i64", "n")], "");
    {
        let (n, d) = ("%n".to_string(), "%d".to_string());
        let hv = f.var_init("i64", "-3750763034362895579");
        let n8 = f.bin("udiv", "i64", &n, "8");
        f.for_range("0", &n8, |f, i, _, _| {
            let q = f.idx(&d, i, 8);
            let w = f.ins(format!("load i64, ptr {}, align 1", q));
            let h0 = f.load("i64", &hv);
            let x = f.bin("xor", "i64", &h0, &w);
            let y = f.bin("mul", "i64", &x, "1099511628211");
            let r = f.bin("lshr", "i64", &y, "29");
            let y2 = f.bin("xor", "i64", &y, &r);
            f.store("i64", &y2, &hv);
        });
        let start = f.bin("mul", "i64", &n8, "8");
        f.for_range(&start, &n, |f, i, _, _| {
            let q = f.off(&d, i);
            let b = f.load("i8", &q);
            let w = f.cast("zext", "i8", &b, "i64");
            let h0 = f.load("i64", &hv);
            let x = f.bin("xor", "i64", &h0, &w);
            let y = f.bin("mul", "i64", &x, "1099511628211");
            f.store("i64", &y, &hv);
        });
        let h1 = f.load("i64", &hv);
        let r = f.bin("lshr", "i64", &h1, "32");
        let h2 = f.bin("xor", "i64", &h1, &r);
        let h3 = f.bin("or", "i64", &h2, "1");
        f.ret("i64", &h3);
    }
    m.add(f);

    // equality
    let mut f = Func::new("internal", "i1", "jv_str_eq", &[(V, "a"), (V, "b")], "");
    let pa = f.ptr_of("%a");
    let pb = f.ptr_of("%b");
    let same = f.icmp("eq", "ptr", &pa, &pb);
    f.if_then(&same, |f| f.ret("i1", "true"));
    let la = f.ld_at("i64", &pa, 8);
    let lb = f.ld_at("i64", &pb, 8);
    let ne = f.ne(&la, &lb);
    f.if_then(&ne, |f| f.ret("i1", "false"));
    let ha = f.ld_at("i64", &pa, 24);
    let hb = f.ld_at("i64", &pb, 24);
    let both = {
        let x = f.ne(&ha, "0");
        let y = f.ne(&hb, "0");
        f.and1(&x, &y)
    };
    let hne = f.ne(&ha, &hb);
    let differ = f.and1(&both, &hne);
    f.if_then(&differ, |f| f.ret("i1", "false"));
    let da = f.off(&pa, "32");
    let db = f.off(&pb, "32");
    let c = f.call("i32", "memcmp", &[("ptr", &da), ("ptr", &db), ("i64", &la)]);
    let r = f.icmp("eq", "i32", &c, "0");
    f.ret("i1", &r);
    m.add(f);

    // compare: i32 <0, 0, >0
    let mut f = Func::new("internal", "i32", "jv_str_cmp", &[(V, "a"), (V, "b")], "");
    let pa = f.ptr_of("%a");
    let pb = f.ptr_of("%b");
    let la = f.ld_at("i64", &pa, 8);
    let lb = f.ld_at("i64", &pb, 8);
    let lt = f.lt(&la, &lb);
    let mn = f.select(&lt, "i64", &la, &lb);
    let da = f.off(&pa, "32");
    let db = f.off(&pb, "32");
    let c = f.call("i32", "memcmp", &[("ptr", &da), ("ptr", &db), ("i64", &mn)]);
    let nz = f.icmp("ne", "i32", &c, "0");
    f.if_then(&nz, |f| f.ret("i32", &c));
    let gt = f.icmp("sgt", "i64", &la, &lb);
    let r1 = f.select(&gt, "i32", "1", "0");
    let r = f.select(&lt, "i32", "-1", &r1);
    f.ret("i32", &r);
    m.add(f);

    // jv_str_reserve(s owned, extra) -> owned unique string with room for extra bytes
    let mut f = Func::new("internal", V, "jv_str_reserve", &[(V, "s"), ("i64", "extra")], "");
    let p = f.ptr_of("%s");
    let rc = f.ld_at("i64", &p, 0);
    let len = f.ld_at("i64", &p, 8);
    let cap = f.ld_at("i64", &p, 16);
    let need = f.add(&len, "%extra");
    let uniq = f.eq(&rc, "1");
    f.if_then(&uniq, |f| {
        f.st_at("i64", "0", &p, 24);
        let small = f.icmp("sle", "i64", &need, &cap);
        f.if_then(&small, |f| f.ret(V, "%s"));
        let c2 = f.bin("mul", "i64", &cap, "2");
        let big = f.icmp("sgt", "i64", &c2, &need);
        let nc = f.select(&big, "i64", &c2, &need);
        let sz = f.add(&nc, "33");
        let fromfl = f.eq(&cap, "31");
        let q = f.if_val(&fromfl, "ptr", |f| {
            let q = f.call("ptr", "jv_malloc", &[("i64", &sz)]);
            let used = f.add(&len, "33");
            f.call("ptr", "memcpy", &[("ptr", &q), ("ptr", &p), ("i64", &used)]);
            f.call("void", "jv_fl_free", &[("ptr", "@g_fl64"), ("ptr", &p)]);
            q
        }, |f| f.call("ptr", "jv_realloc", &[("ptr", &p), ("i64", &sz)]));
        f.st_at("i64", &nc, &q, 16);
        let v = f.mkptr("4", &q);
        f.ret(V, &v);
    });
    let c2 = f.bin("mul", "i64", &len, "2");
    let big = f.icmp("sgt", "i64", &c2, &need);
    let nc = f.select(&big, "i64", &c2, &need);
    let v = f.call(V, "jv_str_alloc", &[("i64", &nc)]);
    let q = f.ptr_of(&v);
    let dst = f.off(&q, "32");
    let src = f.off(&p, "32");
    f.call("ptr", "memcpy", &[("ptr", &dst), ("ptr", &src), ("i64", &len)]);
    f.st_at("i64", &len, &q, 8);
    f.call("void", "jv_release", &[(V, "%s")]);
    f.ret(V, &v);
    m.add(f);

    // jv_str_append(s owned, data, n) -> owned
    let mut f = Func::new("internal", V, "jv_str_append", &[(V, "s"), ("ptr", "d"), ("i64", "n")], "");
    let v = f.call(V, "jv_str_reserve", &[(V, "%s"), ("i64", "%n")]);
    let p = f.ptr_of(&v);
    let len = f.ld_at("i64", &p, 8);
    let base = f.off(&p, "32");
    let dst = f.off(&base, &len);
    f.call("ptr", "memmove", &[("ptr", &dst), ("ptr", "%d"), ("i64", "%n")]);
    let nl = f.add(&len, "%n");
    f.st_at("i64", &nl, &p, 8);
    let e = f.off(&base, &nl);
    f.store("i8", "0", &e);
    f.ret(V, &v);
    m.add(f);

    // jv_str_append_v(s owned, t borrowed)
    let mut f = Func::new("internal", V, "jv_str_append_v", &[(V, "s"), (V, "t")], "");
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%t")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%t")]);
    // if appending to itself, copy first
    let r = f.call(V, "jv_str_append", &[(V, "%s"), ("ptr", &d), ("i64", &n)]);
    f.ret(V, &r);
    m.add(f);

    // jv_str_concat(a, b) -> new
    let mut f = Func::new("internal", V, "jv_str_concat", &[(V, "a"), (V, "b")], "");
    let la = f.call("i64", "jv_str_len", &[(V, "%a")]);
    let lb = f.call("i64", "jv_str_len", &[(V, "%b")]);
    let n = f.add(&la, &lb);
    let v = f.call(V, "jv_str_alloc", &[("i64", &n)]);
    let p = f.ptr_of(&v);
    let dst = f.off(&p, "32");
    let da = f.call("ptr", "jv_str_ptr", &[(V, "%a")]);
    let db = f.call("ptr", "jv_str_ptr", &[(V, "%b")]);
    f.call("ptr", "memcpy", &[("ptr", &dst), ("ptr", &da), ("i64", &la)]);
    let dst2 = f.off(&dst, &la);
    f.call("ptr", "memcpy", &[("ptr", &dst2), ("ptr", &db), ("i64", &lb)]);
    f.st_at("i64", &n, &p, 8);
    let e = f.off(&dst, &n);
    f.store("i8", "0", &e);
    f.ret(V, &v);
    m.add(f);
}

fn arrays(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_arr", &[("i64", "cap")], "");
    let p = f.call("ptr", "jv_fl_alloc", &[("ptr", "@g_fl32"), ("i64", "32")]);
    let small = f.icmp("slt", "i64", "%cap", "4");
    let cap = f.select(&small, "i64", "4", "%cap");
    let bytes = f.bin("mul", "i64", &cap, "16");
    let items = f.call("ptr", "jv_malloc", &[("i64", &bytes)]);
    f.st_at("i64", "1", &p, 0);
    f.st_at("i64", "0", &p, 8);
    f.st_at("i64", &cap, &p, 16);
    f.st_at("ptr", &items, &p, 24);
    let v = f.mkptr("5", &p);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", "i64", "jv_arr_len", &[(V, "a")], "alwaysinline");
    let p = f.ptr_of("%a");
    let n = f.ld_at("i64", &p, 8);
    f.ret("i64", &n);
    m.add(f);

    // borrowed element, unchecked
    let mut f = Func::new("internal", V, "jv_arr_get", &[(V, "a"), ("i64", "i")], "alwaysinline");
    let p = f.ptr_of("%a");
    let items = f.ld_at("ptr", &p, 24);
    let e = f.idx(&items, "%i", 16);
    let v = f.load(V, &e);
    f.ret(V, &v);
    m.add(f);

    // make unique (owned -> owned)
    let mut f = Func::new("internal", V, "jv_arr_unique", &[(V, "a")], "");
    let p = f.ptr_of("%a");
    let rc = f.ld_at("i64", &p, 0);
    let u = f.eq(&rc, "1");
    f.if_then(&u, |f| f.ret(V, "%a"));
    let n = f.ld_at("i64", &p, 8);
    let nv = f.call(V, "jv_arr", &[("i64", &n)]);
    let q = f.ptr_of(&nv);
    let src = f.ld_at("ptr", &p, 24);
    let dst = f.ld_at("ptr", &q, 24);
    f.for_range("0", &n, |f, i, _, _| {
        let s = f.idx(&src, i, 16);
        let v = f.load(V, &s);
        f.call("void", "jv_retain", &[(V, &v)]);
        let d = f.idx(&dst, i, 16);
        f.store(V, &v, &d);
    });
    f.st_at("i64", &n, &q, 8);
    f.call("void", "jv_release", &[(V, "%a")]);
    f.ret(V, &nv);
    m.add(f);

    // grow capacity of a unique array to at least n
    let mut f = Func::new("internal", "void", "jv_arr_grow", &[("ptr", "p"), ("i64", "need")], "");
    let cap = f.ld_at("i64", "%p", 16);
    let ok = f.icmp("sge", "i64", &cap, "%need");
    f.if_then(&ok, |f| f.ret_void());
    let c2 = f.bin("mul", "i64", &cap, "2");
    let big = f.icmp("sgt", "i64", &c2, "%need");
    let nc = f.select(&big, "i64", &c2, "%need");
    let items = f.ld_at("ptr", "%p", 24);
    let bytes = f.bin("mul", "i64", &nc, "16");
    let ni = f.call("ptr", "jv_realloc", &[("ptr", &items), ("i64", &bytes)]);
    f.st_at("ptr", &ni, "%p", 24);
    f.st_at("i64", &nc, "%p", 16);
    f.ret_void();
    m.add(f);

    // push (consumes array, retains x)
    let mut f = Func::new("internal", V, "jv_arr_push", &[(V, "a"), (V, "x")], "");
    let a = f.call(V, "jv_arr_unique", &[(V, "%a")]);
    let p = f.ptr_of(&a);
    let n = f.ld_at("i64", &p, 8);
    let n1 = f.add(&n, "1");
    f.call("void", "jv_arr_grow", &[("ptr", &p), ("i64", &n1)]);
    let items = f.ld_at("ptr", &p, 24);
    let e = f.idx(&items, &n, 16);
    f.call("void", "jv_retain", &[(V, "%x")]);
    f.store(V, "%x", &e);
    f.st_at("i64", &n1, &p, 8);
    f.ret(V, &a);
    m.add(f);

    // set index i >= 0 (consumes array, retains x), pads with null
    let mut f = Func::new("internal", V, "jv_arr_set", &[(V, "a"), ("i64", "i"), (V, "x")], "");
    let a = f.call(V, "jv_arr_unique", &[(V, "%a")]);
    let p = f.ptr_of(&a);
    let n = f.ld_at("i64", &p, 8);
    f.call("void", "jv_retain", &[(V, "%x")]);
    let inside = f.lt("%i", &n);
    f.if_else(
        &inside,
        |f| {
            let items = f.ld_at("ptr", &p, 24);
            let e = f.idx(&items, "%i", 16);
            let old = f.load(V, &e);
            f.store(V, "%x", &e);
            f.call("void", "jv_release", &[(V, &old)]);
        },
        |f| {
            let n1 = f.add("%i", "1");
            f.call("void", "jv_arr_grow", &[("ptr", &p), ("i64", &n1)]);
            let items = f.ld_at("ptr", &p, 24);
            f.for_range(&n, "%i", |f, j, _, _| {
                let e = f.idx(&items, j, 16);
                f.store(V, NULL, &e);
            });
            let e = f.idx(&items, "%i", 16);
            f.store(V, "%x", &e);
            f.st_at("i64", &n1, &p, 8);
        },
    );
    f.ret(V, &a);
    m.add(f);

    // slice copy [from, to) clamped (borrowed -> new)
    let mut f = Func::new("internal", V, "jv_arr_sub", &[(V, "a"), ("i64", "from"), ("i64", "to")], "");
    let n = f.sub("%to", "%from");
    let r = f.call(V, "jv_arr", &[("i64", &n)]);
    let rp = f.ptr_of(&r);
    let ri = f.ld_at("ptr", &rp, 24);
    f.for_range("%from", "%to", |f, i, _, _| {
        let v = f.call(V, "jv_arr_get", &[(V, "%a"), ("i64", i)]);
        f.call("void", "jv_retain", &[(V, &v)]);
        let j = f.sub(i, "%from");
        let d = f.idx(&ri, &j, 16);
        f.store(V, &v, &d);
    });
    f.st_at("i64", &n, &rp, 8);
    f.ret(V, &r);
    m.add(f);
}

fn objects(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_obj", &[], "");
    let p = f.call("ptr", "jv_fl_alloc", &[("ptr", "@g_fl48"), ("i64", "48")]);
    f.st_at("i64", "1", &p, 0);
    f.st_at("i64", "0", &p, 8);
    f.st_at("i64", "0", &p, 16);
    f.st_at("ptr", "null", &p, 24);
    f.st_at("ptr", "null", &p, 32);
    f.st_at("i64", "0", &p, 40);
    let v = f.mkptr("6", &p);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", "i64", "jv_obj_len", &[(V, "o")], "alwaysinline");
    let p = f.ptr_of("%o");
    let n = f.ld_at("i64", &p, 8);
    f.ret("i64", &n);
    m.add(f);

    let mut f = Func::new("internal", V, "jv_obj_key_at", &[(V, "o"), ("i64", "i")], "alwaysinline");
    let p = f.ptr_of("%o");
    let ents = f.ld_at("ptr", &p, 24);
    let e = f.idx(&ents, "%i", 32);
    let k = f.load("ptr", &e);
    let v = f.mkptr("4", &k);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", V, "jv_obj_val_at", &[(V, "o"), ("i64", "i")], "alwaysinline");
    let p = f.ptr_of("%o");
    let ents = f.ld_at("ptr", &p, 24);
    let e = f.idx(&ents, "%i", 32);
    let v = f.ld_at(V, &e, 16);
    f.ret(V, &v);
    m.add(f);

    // find: index or -1
    let mut f = Func::new("internal", "i64", "jv_obj_find", &[(V, "o"), (V, "k")], "");
    let p = f.ptr_of("%o");
    let h = f.call("i64", "jv_str_hash", &[(V, "%k")]);
    let n = f.ld_at("i64", &p, 8);
    let ents = f.ld_at("ptr", &p, 24);
    let index = f.ld_at("ptr", &p, 32);
    let noidx = f.icmp("eq", "ptr", &index, "null");
    f.if_then(&noidx, |f| {
        f.for_range("0", &n, |f, i, _, _| {
            let e = f.idx(&ents, i, 32);
            let eh = f.ld_at("i64", &e, 8);
            let same = f.eq(&eh, &h);
            f.if_then(&same, |f| {
                let kp = f.load("ptr", &e);
                let kv = f.mkptr("4", &kp);
                let eq = f.call("i1", "jv_str_eq", &[(V, &kv), (V, "%k")]);
                f.if_then(&eq, |f| f.ret("i64", i));
            });
        });
        f.ret("i64", "-1");
    });
    let icap = f.ld_at("i64", &p, 40);
    let mask = f.sub(&icap, "1");
    let j0 = f.bin("and", "i64", &h, &mask);
    let jv = f.var_init("i64", &j0);
    f.while_(
        |_| "true".to_string(),
        |f, _, _| {
            let j = f.load("i64", &jv);
            let sp = f.idx(&index, &j, 4);
            let s = f.load("i32", &sp);
            let s64 = f.cast("sext", "i32", &s, "i64");
            let empty = f.eq(&s64, "0");
            f.if_then(&empty, |f| f.ret("i64", "-1"));
            let ei = f.sub(&s64, "1");
            let e = f.idx(&ents, &ei, 32);
            let eh = f.ld_at("i64", &e, 8);
            let same = f.eq(&eh, &h);
            f.if_then(&same, |f| {
                let kp = f.load("ptr", &e);
                let kv = f.mkptr("4", &kp);
                let eq = f.call("i1", "jv_str_eq", &[(V, &kv), (V, "%k")]);
                f.if_then(&eq, |f| f.ret("i64", &ei));
            });
            let j1 = f.add(&j, "1");
            let j2 = f.bin("and", "i64", &j1, &mask);
            f.store("i64", &j2, &jv);
        },
    );
    f.ret("i64", "-1");
    m.add(f);

    // borrowed lookup: returns value or a tag-10 sentinel when missing
    let mut f = Func::new("internal", V, "jv_obj_get", &[(V, "o"), (V, "k")], "");
    let i = f.call("i64", "jv_obj_find", &[(V, "%o"), (V, "%k")]);
    let miss = f.lt(&i, "0");
    f.if_then(&miss, |f| f.ret(V, "{ i64 10, i64 0 }"));
    let v = f.call(V, "jv_obj_val_at", &[(V, "%o"), ("i64", &i)]);
    f.ret(V, &v);
    m.add(f);

    // rebuild index for p (len > 8)
    let mut f = Func::new("internal", "void", "jv_obj_reindex", &[("ptr", "p")], "");
    let old = f.ld_at("ptr", "%p", 32);
    f.call("void", "free", &[("ptr", &old)]);
    let n = f.ld_at("i64", "%p", 8);
    let small = f.icmp("sle", "i64", &n, "8");
    f.if_then(&small, |f| {
        f.st_at("ptr", "null", "%p", 32);
        f.st_at("i64", "0", "%p", 40);
        f.ret_void();
    });
    let icv = f.var_init("i64", "16");
    let n4 = f.bin("mul", "i64", &n, "4");
    f.while_(
        |f| {
            let c = f.load("i64", &icv);
            f.icmp("slt", "i64", &c, &n4)
        },
        |f, _, _| {
            let c = f.load("i64", &icv);
            let c2 = f.bin("mul", "i64", &c, "2");
            f.store("i64", &c2, &icv);
        },
    );
    let icap = f.load("i64", &icv);
    let idx = f.call("ptr", "calloc", &[("i64", &icap), ("i64", "4")]);
    f.st_at("ptr", &idx, "%p", 32);
    f.st_at("i64", &icap, "%p", 40);
    let mask = f.sub(&icap, "1");
    let ents = f.ld_at("ptr", "%p", 24);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.idx(&ents, i, 32);
        let h = f.ld_at("i64", &e, 8);
        let j0 = f.bin("and", "i64", &h, &mask);
        let jv = f.var_init("i64", &j0);
        f.while_(
            |f| {
                let j = f.load("i64", &jv);
                let sp = f.idx(&idx, &j, 4);
                let s = f.load("i32", &sp);
                f.icmp("ne", "i32", &s, "0")
            },
            |f, _, _| {
                let j = f.load("i64", &jv);
                let j1 = f.add(&j, "1");
                let j2 = f.bin("and", "i64", &j1, &mask);
                f.store("i64", &j2, &jv);
            },
        );
        let j = f.load("i64", &jv);
        let sp = f.idx(&idx, &j, 4);
        let i1 = f.add(i, "1");
        let s = f.cast("trunc", "i64", &i1, "i32");
        f.store("i32", &s, &sp);
    });
    f.ret_void();
    m.add(f);

    // make unique (owned -> owned)
    let mut f = Func::new("internal", V, "jv_obj_unique", &[(V, "o")], "");
    let p = f.ptr_of("%o");
    let rc = f.ld_at("i64", &p, 0);
    let u = f.eq(&rc, "1");
    f.if_then(&u, |f| f.ret(V, "%o"));
    let n = f.ld_at("i64", &p, 8);
    let q = f.call("ptr", "jv_fl_alloc", &[("ptr", "@g_fl48"), ("i64", "48")]);
    let cap = n.clone();
    let bytes = {
        let c = f.icmp("slt", "i64", &cap, "4");
        let c4 = f.select(&c, "i64", "4", &cap);
        f.st_at("i64", &c4, &q, 16);
        f.bin("mul", "i64", &c4, "32")
    };
    let ne = f.call("ptr", "jv_malloc", &[("i64", &bytes)]);
    let oe = f.ld_at("ptr", &p, 24);
    let used = f.bin("mul", "i64", &n, "32");
    f.call("ptr", "memcpy", &[("ptr", &ne), ("ptr", &oe), ("i64", &used)]);
    f.for_range("0", &n, |f, i, _, _| {
        let e = f.idx(&ne, i, 32);
        let k = f.load("ptr", &e);
        let kv = f.mkptr("4", &k);
        f.call("void", "jv_retain", &[(V, &kv)]);
        let v = f.ld_at(V, &e, 16);
        f.call("void", "jv_retain", &[(V, &v)]);
    });
    f.st_at("i64", "1", &q, 0);
    f.st_at("i64", &n, &q, 8);
    f.st_at("ptr", &ne, &q, 24);
    f.st_at("ptr", "null", &q, 32);
    f.st_at("i64", "0", &q, 40);
    f.call("void", "jv_obj_reindex", &[("ptr", &q)]);
    f.call("void", "jv_release", &[(V, "%o")]);
    let r = f.mkptr("6", &q);
    f.ret(V, &r);
    m.add(f);

    // set (consumes object; retains key and value)
    let mut f = Func::new("internal", V, "jv_obj_set", &[(V, "o"), (V, "k"), (V, "x")], "");
    let o = f.call(V, "jv_obj_unique", &[(V, "%o")]);
    let p = f.ptr_of(&o);
    f.call("void", "jv_retain", &[(V, "%x")]);
    let i = f.call("i64", "jv_obj_find", &[(V, &o), (V, "%k")]);
    let found = f.icmp("sge", "i64", &i, "0");
    f.if_then(&found, |f| {
        let ents = f.ld_at("ptr", &p, 24);
        let e = f.idx(&ents, &i, 32);
        let vp = f.off(&e, "16");
        let old = f.load(V, &vp);
        f.store(V, "%x", &vp);
        f.call("void", "jv_release", &[(V, &old)]);
        f.ret(V, &o);
    });
    let n = f.ld_at("i64", &p, 8);
    let cap = f.ld_at("i64", &p, 16);
    let full = f.icmp("sge", "i64", &n, &cap);
    f.if_then(&full, |f| {
        let c2 = f.bin("mul", "i64", &cap, "2");
        let s = f.icmp("slt", "i64", &c2, "4");
        let nc = f.select(&s, "i64", "4", &c2);
        let ents = f.ld_at("ptr", &p, 24);
        let bytes = f.bin("mul", "i64", &nc, "32");
        let ne = f.call("ptr", "jv_realloc", &[("ptr", &ents), ("i64", &bytes)]);
        f.st_at("ptr", &ne, &p, 24);
        f.st_at("i64", &nc, &p, 16);
    });
    let ents = f.ld_at("ptr", &p, 24);
    let e = f.idx(&ents, &n, 32);
    f.call("void", "jv_retain", &[(V, "%k")]);
    let kp = f.ptr_of("%k");
    f.store("ptr", &kp, &e);
    let h = f.call("i64", "jv_str_hash", &[(V, "%k")]);
    f.st_at("i64", &h, &e, 8);
    f.st_at(V, "%x", &e, 16);
    let n1 = f.add(&n, "1");
    f.st_at("i64", &n1, &p, 8);
    // index maintenance
    let index = f.ld_at("ptr", &p, 32);
    let icap = f.ld_at("i64", &p, 40);
    let n2 = f.bin("mul", "i64", &n1, "2");
    let need = f.icmp("sgt", "i64", &n2, &icap);
    let big = f.icmp("sgt", "i64", &n1, "8");
    let rebuild = f.and1(&need, &big);
    f.if_else(
        &rebuild,
        |f| {
            f.call("void", "jv_obj_reindex", &[("ptr", &p)]);
        },
        |f| {
            let has = f.icmp("ne", "ptr", &index, "null");
            f.if_then(&has, |f| {
                let mask = f.sub(&icap, "1");
                let j0 = f.bin("and", "i64", &h, &mask);
                let jv = f.var_init("i64", &j0);
                f.while_(
                    |f| {
                        let j = f.load("i64", &jv);
                        let sp = f.idx(&index, &j, 4);
                        let s = f.load("i32", &sp);
                        f.icmp("ne", "i32", &s, "0")
                    },
                    |f, _, _| {
                        let j = f.load("i64", &jv);
                        let j1 = f.add(&j, "1");
                        let j2 = f.bin("and", "i64", &j1, &mask);
                        f.store("i64", &j2, &jv);
                    },
                );
                let j = f.load("i64", &jv);
                let sp = f.idx(&index, &j, 4);
                let s = f.cast("trunc", "i64", &n1, "i32");
                f.store("i32", &s, &sp);
            });
        },
    );
    f.ret(V, &o);
    m.add(f);

    // delete key (consumes object)
    let mut f = Func::new("internal", V, "jv_obj_del", &[(V, "o"), (V, "k")], "");
    let i0 = f.call("i64", "jv_obj_find", &[(V, "%o"), (V, "%k")]);
    let miss = f.lt(&i0, "0");
    f.if_then(&miss, |f| f.ret(V, "%o"));
    let o = f.call(V, "jv_obj_unique", &[(V, "%o")]);
    let p = f.ptr_of(&o);
    let ents = f.ld_at("ptr", &p, 24);
    let e = f.idx(&ents, &i0, 32);
    let kp = f.load("ptr", &e);
    let kv = f.mkptr("4", &kp);
    f.call("void", "jv_release", &[(V, &kv)]);
    let v = f.ld_at(V, &e, 16);
    f.call("void", "jv_release", &[(V, &v)]);
    let n = f.ld_at("i64", &p, 8);
    let i1 = f.add(&i0, "1");
    let rest = f.sub(&n, &i1);
    let bytes = f.bin("mul", "i64", &rest, "32");
    let src = f.idx(&ents, &i1, 32);
    f.call("ptr", "memmove", &[("ptr", &e), ("ptr", &src), ("i64", &bytes)]);
    let n1 = f.sub(&n, "1");
    f.st_at("i64", &n1, &p, 8);
    f.call("void", "jv_obj_reindex", &[("ptr", &p)]);
    f.ret(V, &o);
    m.add(f);
}

fn errors(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_err", &[(V, "msg")], "");
    let p = f.call("ptr", "jv_fl_alloc", &[("ptr", "@g_fl32"), ("i64", "32")]);
    f.st_at("i64", "1", &p, 0);
    f.call("void", "jv_retain", &[(V, "%msg")]);
    f.st_at(V, "%msg", &p, 8);
    let v = f.mkptr("7", &p);
    f.ret(V, &v);
    m.add(f);

    // error from a C string
    let mut f = Func::new("internal", V, "jv_err_c", &[("ptr", "s")], "noinline");
    let s = f.call(V, "jv_cstr", &[("ptr", "%s")]);
    let e = f.call(V, "jv_err", &[(V, &s)]);
    f.call("void", "jv_release", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    // error consuming an owned message
    let mut f = Func::new("internal", V, "jv_err_take", &[(V, "s")], "noinline");
    let e = f.call(V, "jv_err", &[(V, "%s")]);
    f.call("void", "jv_release", &[(V, "%s")]);
    f.ret(V, &e);
    m.add(f);

    let mut f = Func::new("internal", V, "jv_err_msg", &[(V, "e")], "");
    let p = f.ptr_of("%e");
    let v = f.ld_at(V, &p, 8);
    f.ret(V, &v);
    m.add(f);

    // bypass(id, inner owned)
    let mut f = Func::new("internal", V, "jv_bypass", &[("i64", "id"), (V, "x")], "noinline");
    let p = f.call("ptr", "jv_fl_alloc", &[("ptr", "@g_fl32"), ("i64", "32")]);
    f.st_at("i64", "1", &p, 0);
    f.st_at("i64", "%id", &p, 8);
    f.st_at(V, "%x", &p, 16);
    let v = f.mkptr("8", &p);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", "i64", "jv_bypass_id", &[(V, "b")], "");
    let p = f.ptr_of("%b");
    let v = f.ld_at("i64", &p, 8);
    f.ret("i64", &v);
    m.add(f);

    // unwrap (consumes b, returns owned inner)
    let mut f = Func::new("internal", V, "jv_bypass_unwrap", &[(V, "b")], "noinline");
    let p = f.ptr_of("%b");
    let v = f.ld_at(V, &p, 16);
    f.call("void", "jv_retain", &[(V, &v)]);
    f.call("void", "jv_release", &[(V, "%b")]);
    f.ret(V, &v);
    m.add(f);
}

fn sb(m: &mut Module) {
    // string builder at ptr: data@0 len@8 cap@16
    let mut f = Func::new("internal", "void", "sb_init", &[("ptr", "b"), ("i64", "cap")], "");
    let d = f.call("ptr", "jv_malloc", &[("i64", "%cap")]);
    f.st_at("ptr", &d, "%b", 0);
    f.st_at("i64", "0", "%b", 8);
    f.st_at("i64", "%cap", "%b", 16);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "sb_grow", &[("ptr", "b"), ("i64", "need")], "noinline");
    let cap = f.ld_at("i64", "%b", 16);
    let c2 = f.bin("mul", "i64", &cap, "2");
    let big = f.icmp("sgt", "i64", &c2, "%need");
    let nc = f.select(&big, "i64", &c2, "%need");
    let d = f.ld_at("ptr", "%b", 0);
    let nd = f.call("ptr", "jv_realloc", &[("ptr", &d), ("i64", &nc)]);
    f.st_at("ptr", &nd, "%b", 0);
    f.st_at("i64", &nc, "%b", 16);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "sb_putc", &[("ptr", "b"), ("i8", "c")], "alwaysinline");
    let len = f.ld_at("i64", "%b", 8);
    let cap = f.ld_at("i64", "%b", 16);
    let full = f.icmp("sge", "i64", &len, &cap);
    f.if_then(&full, |f| {
        let n = f.add(&len, "1");
        f.call("void", "sb_grow", &[("ptr", "%b"), ("i64", &n)]);
    });
    let d = f.ld_at("ptr", "%b", 0);
    let q = f.off(&d, &len);
    f.store("i8", "%c", &q);
    let n = f.add(&len, "1");
    f.st_at("i64", &n, "%b", 8);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "sb_write", &[("ptr", "b"), ("ptr", "s"), ("i64", "n")], "");
    let len = f.ld_at("i64", "%b", 8);
    let cap = f.ld_at("i64", "%b", 16);
    let need = f.add(&len, "%n");
    let full = f.icmp("sgt", "i64", &need, &cap);
    f.if_then(&full, |f| {
        f.call("void", "sb_grow", &[("ptr", "%b"), ("i64", &need)]);
    });
    let d = f.ld_at("ptr", "%b", 0);
    let q = f.off(&d, &len);
    f.call("ptr", "memcpy", &[("ptr", &q), ("ptr", "%s"), ("i64", "%n")]);
    f.st_at("i64", &need, "%b", 8);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "sb_puts", &[("ptr", "b"), ("ptr", "s")], "");
    let n = f.call("i64", "strlen", &[("ptr", "%s")]);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", "%s"), ("i64", &n)]);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "sb_putv", &[("ptr", "b"), (V, "s")], "");
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%s")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%s")]);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &d), ("i64", &n)]);
    f.ret_void();
    m.add(f);

    // finish: new string, frees the builder storage
    let mut f = Func::new("internal", V, "sb_finish", &[("ptr", "b")], "");
    let d = f.ld_at("ptr", "%b", 0);
    let n = f.ld_at("i64", "%b", 8);
    let v = f.call(V, "jv_str", &[("ptr", &d), ("i64", &n)]);
    f.call("void", "free", &[("ptr", &d)]);
    f.ret(V, &v);
    m.add(f);

    // append integer
    let mut f = Func::new("internal", "void", "sb_int", &[("ptr", "b"), ("i64", "x")], "");
    // digits are produced right-to-left into a small buffer
    let buf = f.array_var("i8", 24);
    let neg = f.icmp("slt", "i64", "%x", "0");
    let nx = f.sub("0", "%x");
    let mag = f.select(&neg, "i64", &nx, "%x");
    let pv = f.var_init("i64", "24");
    let xv = f.var_init("i64", &mag);
    f.while_(
        |_| "true".into(),
        |f, brk, _| {
            let x = f.load("i64", &xv);
            let q = f.bin("udiv", "i64", &x, "10");
            let q10 = f.bin("mul", "i64", &q, "10");
            let d = f.sub(&x, &q10);
            let c = f.add(&d, "48");
            let c8 = f.cast("trunc", "i64", &c, "i8");
            let p = f.load("i64", &pv);
            let p1 = f.sub(&p, "1");
            f.store("i64", &p1, &pv);
            let at = f.off(&buf, &p1);
            f.store("i8", &c8, &at);
            f.store("i64", &q, &xv);
            let z = f.eq(&q, "0");
            f.if_then(&z, |f| f.br(brk));
        },
    );
    f.if_then(&neg, |f| {
        let p = f.load("i64", &pv);
        let p1 = f.sub(&p, "1");
        f.store("i64", &p1, &pv);
        let at = f.off(&buf, &p1);
        f.store("i8", "45", &at);
    });
    let p = f.load("i64", &pv);
    let start = f.off(&buf, &p);
    let len = f.sub("24", &p);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &start), ("i64", &len)]);
    f.ret_void();
    m.add(f);
}

/// Free lists for fixed-size blocks (array/error headers: 32 bytes, object
/// headers: 48, short strings: 64). Freed blocks are pushed and reused.
fn freelists(m: &mut Module) {
    for g in ["@g_fl32", "@g_fl48", "@g_fl64"] {
        m.global(format!("{} = internal global ptr null", g));
    }
    let mut f = Func::new("internal", "ptr", "jv_fl_alloc", &[("ptr", "head"), ("i64", "size")], "alwaysinline");
    let h = f.load("ptr", "%head");
    let empty = f.icmp("eq", "ptr", &h, "null");
    f.if_then(&empty, |f| {
        let p = f.call("ptr", "jv_malloc", &[("i64", "%size")]);
        f.ret("ptr", &p);
    });
    let next = f.load("ptr", &h);
    f.store("ptr", &next, "%head");
    f.ret("ptr", &h);
    m.add(f);

    let mut f = Func::new("internal", "void", "jv_fl_free", &[("ptr", "head"), ("ptr", "p")], "alwaysinline");
    let h = f.load("ptr", "%head");
    f.store("ptr", &h, "%p");
    f.store("ptr", "%p", "%head");
    f.ret_void();
    m.add(f);
}

/// Refcounting of preserved number literals (rare: kept out of line).
pub fn lit_refcount(m: &mut Module) {
    let mut f = Func::new("internal", "void", "jv_lit_retain", &[("i64", "t")], "noinline cold");
    let a = f.bin("lshr", "i64", "%t", "8");
    let p = f.cast("inttoptr", "i64", &a, "ptr");
    let rc = f.load("i64", &p);
    let n = f.add(&rc, "1");
    f.store("i64", &n, &p);
    f.ret_void();
    m.add(f);
    let mut f = Func::new("internal", "void", "jv_lit_release", &[("i64", "t")], "noinline cold");
    let a = f.bin("lshr", "i64", "%t", "8");
    let p = f.cast("inttoptr", "i64", &a, "ptr");
    let rc = f.load("i64", &p);
    let n = f.sub(&rc, "1");
    let z = f.eq(&n, "0");
    f.if_else(&z, |f| f.call("void", "free", &[("ptr", &p)]), |f| f.store("i64", &n, &p));
    f.ret_void();
    m.add(f);
}

/// Release for cold paths (error exits): not inlined, keeps generated code small.
pub fn release_cold(m: &mut Module) {
    let mut f = Func::new("internal", "void", "jv_release_cold", &[(V, "x")], "noinline cold");
    f.call("void", "jv_release", &[(V, "%x")]);
    f.ret_void();
    m.add(f);
}
