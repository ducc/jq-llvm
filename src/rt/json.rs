//! JSON serialisation and parsing, number formatting, UTF-8 helpers.

use super::*;
use crate::ir::{fconst, Func, Module, V};

pub fn emit(m: &mut Module) {
    m.declare("llvm.fabs.f64", "declare double @llvm.fabs.f64(double)");
    utf8(m);
    type_names(m);
    dump_num(m);
    numlit(m);
    numlit_ops(m);
    numlit_cmp2(m);
    dump_color(m);
    dump_str(m);
    dump(m);
    crate::rt::parse::emit(m);
}

fn utf8(m: &mut Module) {
    // decode one code point at d[i] (n = total length); writes byte length to *len.
    // Invalid sequences decode as U+FFFD with length 1.
    let mut f = Func::new("internal", "i64", "utf8_decode", &[("ptr", "d"), ("i64", "n"), ("i64", "i"), ("ptr", "len")], "");
    let p = f.off("%d", "%i");
    let b0 = f.load("i8", &p);
    let c0 = f.cast("zext", "i8", &b0, "i64");
    let ascii = f.icmp("ult", "i64", &c0, "128");
    f.if_then(&ascii, |f| {
        f.store("i64", "1", "%len");
        f.ret("i64", &c0);
    });
    f.store("i64", "1", "%len");
    // determine sequence length
    let is2 = {
        let x = f.bin("and", "i64", &c0, "224");
        f.eq(&x, "192")
    };
    let is3 = {
        let x = f.bin("and", "i64", &c0, "240");
        f.eq(&x, "224")
    };
    let is4 = {
        let x = f.bin("and", "i64", &c0, "248");
        f.eq(&x, "240")
    };
    let l34 = f.select(&is3, "i64", "3", "4");
    let l = f.select(&is2, "i64", "2", &l34);
    let valid_lead = {
        let a = f.or1(&is2, &is3);
        f.or1(&a, &is4)
    };
    let bad = f.not1(&valid_lead);
    f.if_then(&bad, |f| f.ret("i64", "65533"));
    let end = f.add("%i", &l);
    let over = f.icmp("sgt", "i64", &end, "%n");
    f.if_then(&over, |f| f.ret("i64", "65533"));
    let m2 = f.select(&is2, "i64", "31", "0");
    let m3 = f.select(&is3, "i64", "15", &m2);
    let mask = f.select(&is4, "i64", "7", &m3);
    let acc = f.var_init("i64", "0");
    let a0 = f.bin("and", "i64", &c0, &mask);
    f.store("i64", &a0, &acc);
    f.for_range("1", &l, |f, k, _, _| {
        let j = f.add("%i", k);
        let q = f.off("%d", &j);
        let b = f.load("i8", &q);
        let c = f.cast("zext", "i8", &b, "i64");
        let top = f.bin("and", "i64", &c, "192");
        let cont = f.eq(&top, "128");
        let notc = f.not1(&cont);
        f.if_then(&notc, |f| f.ret("i64", "65533"));
        let a = f.load("i64", &acc);
        let s = f.bin("shl", "i64", &a, "6");
        let low = f.bin("and", "i64", &c, "63");
        let r = f.bin("or", "i64", &s, &low);
        f.store("i64", &r, &acc);
    });
    f.store("i64", &l, "%len");
    let r = f.load("i64", &acc);
    f.ret("i64", &r);
    m.add(f);

    // encode code point into builder
    let mut f = Func::new("internal", "void", "sb_utf8", &[("ptr", "b"), ("i64", "c")], "");
    let put = |f: &mut Func, x: &str| {
        let b = f.cast("trunc", "i64", x, "i8");
        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", &b)]);
    };
    let lt80 = f.icmp("ult", "i64", "%c", "128");
    f.if_then(&lt80, |f| {
        put(f, "%c");
        f.ret_void();
    });
    let lt800 = f.icmp("ult", "i64", "%c", "2048");
    f.if_then(&lt800, |f| {
        let a = f.bin("lshr", "i64", "%c", "6");
        let a = f.bin("or", "i64", &a, "192");
        put(f, &a);
        let b = f.bin("and", "i64", "%c", "63");
        let b = f.bin("or", "i64", &b, "128");
        put(f, &b);
        f.ret_void();
    });
    let lt10000 = f.icmp("ult", "i64", "%c", "65536");
    f.if_then(&lt10000, |f| {
        let a = f.bin("lshr", "i64", "%c", "12");
        let a = f.bin("or", "i64", &a, "224");
        put(f, &a);
        let b = f.bin("lshr", "i64", "%c", "6");
        let b = f.bin("and", "i64", &b, "63");
        let b = f.bin("or", "i64", &b, "128");
        put(f, &b);
        let c = f.bin("and", "i64", "%c", "63");
        let c = f.bin("or", "i64", &c, "128");
        put(f, &c);
        f.ret_void();
    });
    let a = f.bin("lshr", "i64", "%c", "18");
    let a = f.bin("or", "i64", &a, "240");
    put(&mut f, &a);
    for sh in ["12", "6"] {
        let b = f.bin("lshr", "i64", "%c", sh);
        let b = f.bin("and", "i64", &b, "63");
        let b = f.bin("or", "i64", &b, "128");
        put(&mut f, &b);
    }
    let c = f.bin("and", "i64", "%c", "63");
    let c = f.bin("or", "i64", &c, "128");
    put(&mut f, &c);
    f.ret_void();
    m.add(f);

    // number of code points
    let mut f = Func::new("internal", "i64", "utf8_count", &[("ptr", "d"), ("i64", "n")], "");
    let cnt = f.var_init("i64", "0");
    f.for_range("0", "%n", |f, i, _, _| {
        let q = f.off("%d", i);
        let b = f.load("i8", &q);
        let c = f.cast("zext", "i8", &b, "i64");
        let top = f.bin("and", "i64", &c, "192");
        let cont = f.eq(&top, "128");
        let notc = f.not1(&cont);
        let inc = f.cast("zext", "i1", &notc, "i64");
        let x = f.load("i64", &cnt);
        let y = f.add(&x, &inc);
        f.store("i64", &y, &cnt);
    });
    let r = f.load("i64", &cnt);
    f.ret("i64", &r);
    m.add(f);
}

fn type_names(m: &mut Module) {
    let names = ["null", "boolean", "boolean", "number", "string", "array", "object"];
    let g: Vec<String> = names.iter().map(|n| m.cstr(n)).collect();
    let mut f = Func::new("internal", "ptr", "jv_kind_name", &[("i64", "t")], "");
    let labels: Vec<String> = (0..7).map(|i| f.label(&format!("k{}", i))).collect();
    let dflt = f.label("kd");
    let cases: Vec<(i64, String)> = labels.iter().enumerate().map(|(i, l)| (i as i64, l.clone())).collect();
    f.switch("%t", &dflt, &cases);
    for (i, l) in labels.iter().enumerate() {
        f.block(l);
        f.ret("ptr", &g[i]);
    }
    f.block(&dflt);
    let inv = m.cstr("<invalid>");
    f.ret("ptr", &inv);
    m.add(f);

    // type name as a jq string value
    let mut f = Func::new("internal", V, "jv_type", &[(V, "x")], "");
    let t = f.tag("%x");
    let p = f.call("ptr", "jv_kind_name", &[("i64", &t)]);
    let s = f.call(V, "jv_cstr", &[("ptr", &p)]);
    f.ret(V, &s);
    m.add(f);
}

fn dump_num(m: &mut Module) {
    let null = m.cstr("null");
    let pinf = m.cstr("1.7976931348623157e+308");
    let ninf = m.cstr("-1.7976931348623157e+308");
    let efmt = m.cstr("%.*e");
    let ffmt = m.cstr("%.*f");
    let mut f = Func::new("internal", "void", "jv_dump_num", &[("ptr", "b"), ("double", "d")], "");
    let isnan = f.fcmp("uno", "%d", "%d");
    f.if_then(&isnan, |f| {
        f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &null), ("i64", "4")]);
        f.ret_void();
    });
    let a = f.call("double", "llvm.fabs.f64", &[("double", "%d")]);
    let isinf = f.fcmp("oeq", &a, &fconst(f64::INFINITY));
    f.if_then(&isinf, |f| {
        let pos = f.fcmp("ogt", "%d", "0.0");
        let s = f.select(&pos, "ptr", &pinf, &ninf);
        f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &s)]);
        f.ret_void();
    });
    let small = f.fcmp("olt", &a, &fconst(1e16));
    let t = f.call("double", "trunc", &[("double", "%d")]);
    let integral = f.fcmp("oeq", &t, "%d");
    let fast = f.and1(&small, &integral);
    f.if_then(&fast, |f| {
        let i = f.cast("fptosi", "double", "%d", "i64");
        let isz = f.eq(&i, "0");
        f.if_then(&isz, |f| {
            let bits = f.cast("bitcast", "double", "%d", "i64");
            let neg = f.icmp("slt", "i64", &bits, "0");
            f.if_then(&neg, |f| f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "45")]));
        });
        f.call("void", "sb_int", &[("ptr", "%b"), ("i64", &i)]);
        f.ret_void();
    });
    // shortest round-trip representation
    let buf = f.array_var("i8", 40);
    let pv = f.var_init("i64", "1");
    f.while_(
        |f| {
            let p = f.load("i64", &pv);
            f.icmp("slt", "i64", &p, "17")
        },
        |f, brk, _| {
            let p = f.load("i64", &pv);
            let p1 = f.sub(&p, "1");
            let p32 = f.cast("trunc", "i64", &p1, "i32");
            f.call(
                "i32 (ptr, i64, ptr, ...)",
                "snprintf",
                &[("ptr", &buf), ("i64", "40"), ("ptr", &efmt), ("i32", &p32), ("double", "%d")],
            );
            let back = f.call("double", "strtod", &[("ptr", &buf), ("ptr", "null")]);
            let same = f.fcmp("oeq", &back, "%d");
            f.if_then(&same, |f| f.br(brk));
            let p2 = f.add(&p, "1");
            f.store("i64", &p2, &pv);
        },
    );
    let p = f.load("i64", &pv);
    let p1 = f.sub(&p, "1");
    let p32 = f.cast("trunc", "i64", &p1, "i32");
    let n = f.call(
        "i32 (ptr, i64, ptr, ...)",
        "snprintf",
        &[("ptr", &buf), ("i64", "40"), ("ptr", &efmt), ("i32", &p32), ("double", "%d")],
    );
    let n64 = f.cast("sext", "i32", &n, "i64");
    // locate 'e' and parse exponent
    let ev = f.var_init("i64", "0");
    f.for_range("0", &n64, |f, i, brk, _| {
        let q = f.off(&buf, i);
        let c = f.load("i8", &q);
        let ise = f.icmp("eq", "i8", &c, "101");
        f.if_then(&ise, |f| {
            f.store("i64", i, &ev);
            f.br(brk);
        });
    });
    let epos = f.load("i64", &ev);
    let es = f.add(&epos, "1");
    let ep = f.off(&buf, &es);
    let ex = f.call("double", "strtod", &[("ptr", &ep), ("ptr", "null")]);
    let exp = f.cast("fptosi", "double", &ex, "i64");
    let decpt = f.add(&exp, "1");
    let nd = p.clone();
    let lowexp = f.icmp("sle", "i64", &decpt, "-4");
    let nd15 = f.add(&nd, "15");
    let highexp = f.icmp("sgt", "i64", &decpt, &nd15);
    let useexp = f.or1(&lowexp, &highexp);
    f.if_then(&useexp, |f| {
        f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &buf), ("i64", &n64)]);
        f.ret_void();
    });
    let bigint = f.icmp("sgt", "i64", &decpt, &nd);
    f.if_then(&bigint, |f| {
        // digits then zeros
        f.for_range("0", &epos, |f, i, _, _| {
            let q = f.off(&buf, i);
            let c = f.load("i8", &q);
            let isdot = f.icmp("eq", "i8", &c, "46");
            let notdot = f.not1(&isdot);
            f.if_then(&notdot, |f| f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", &c)]));
        });
        let z = f.sub(&decpt, &nd);
        f.for_range("0", &z, |f, _, _, _| {
            f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "48")]);
        });
        f.ret_void();
    });
    let prec = f.sub(&nd, &decpt);
    let prec32 = f.cast("trunc", "i64", &prec, "i32");
    let n = f.call(
        "i32 (ptr, i64, ptr, ...)",
        "snprintf",
        &[("ptr", &buf), ("i64", "40"), ("ptr", &ffmt), ("i32", &prec32), ("double", "%d")],
    );
    let n64 = f.cast("sext", "i32", &n, "i64");
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &buf), ("i64", &n64)]);
    f.ret_void();
    m.add(f);
}

fn dump_str(m: &mut Module) {
    let ufmt = m.cstr("\\u%04x");
    let mut f = Func::new("internal", "void", "jv_dump_strbytes", &[("ptr", "b"), ("ptr", "d"), ("i64", "n"), ("i64", "ascii")], "");
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "34")]);
    let start = f.var_init("i64", "0");
    let iv = f.var_init("i64", "0");
    let lenv = f.var("i64");
    let hex = f.array_var("i8", 16);
    let asc = f.ne("%ascii", "0");
    let put_u = |f: &mut Func, cp: &str| {
        let n = f.call(
            "i32 (ptr, i64, ptr, ...)",
            "snprintf",
            &[("ptr", &hex), ("i64", "16"), ("ptr", &ufmt), ("i64", cp)],
        );
        let n64 = f.cast("sext", "i32", &n, "i64");
        f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &hex), ("i64", &n64)]);
    };
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, "%n")
        },
        |f, _, cont| {
            let i = f.load("i64", &iv);
            let q = f.off("%d", &i);
            let b = f.load("i8", &q);
            let c = f.cast("zext", "i8", &b, "i64");
            let ctl = f.icmp("ult", "i64", &c, "32");
            let quote = f.eq(&c, "34");
            let bs = f.eq(&c, "92");
            let del = f.eq(&c, "127");
            let hi = f.icmp("uge", "i64", &c, "128");
            let hia = f.and1(&hi, &asc);
            let e1 = f.or1(&ctl, &quote);
            let e2 = f.or1(&bs, &del);
            let e3 = f.or1(&e1, &e2);
            let esc = f.or1(&e3, &hia);
            let plain = f.not1(&esc);
            f.if_then(&plain, |f| {
                let i1 = f.add(&i, "1");
                f.store("i64", &i1, &iv);
                f.br(cont);
            });
            // flush pending run
            let s = f.load("i64", &start);
            let sp = f.off("%d", &s);
            let run = f.sub(&i, &s);
            f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &sp), ("i64", &run)]);
            f.if_else(
                &hia,
                |f| {
                    let cp = f.call("i64", "utf8_decode", &[("ptr", "%d"), ("i64", "%n"), ("i64", &i), ("ptr", &lenv)]);
                    let big = f.icmp("sge", "i64", &cp, "65536");
                    f.if_else(
                        &big,
                        |f| {
                            let c2 = f.sub(&cp, "65536");
                            let h = f.bin("lshr", "i64", &c2, "10");
                            let h = f.add(&h, "55296");
                            let l = f.bin("and", "i64", &c2, "1023");
                            let l = f.add(&l, "56320");
                            put_u(f, &h);
                            put_u(f, &l);
                        },
                        |f| put_u(f, &cp),
                    );
                    let l = f.load("i64", &lenv);
                    let i2 = f.add(&i, &l);
                    f.store("i64", &i2, &iv);
                },
                |f| {
                    let (ln, lt, lr, lb, lf, lq, lbs, ld, le) = (
                        f.label("en"),
                        f.label("et"),
                        f.label("er"),
                        f.label("eb"),
                        f.label("ef"),
                        f.label("eq"),
                        f.label("ebs"),
                        f.label("ed"),
                        f.label("ee"),
                    );
                    f.switch(
                        &c,
                        &ld,
                        &[
                            (10, ln.clone()),
                            (9, lt.clone()),
                            (13, lr.clone()),
                            (8, lb.clone()),
                            (12, lf.clone()),
                            (34, lq.clone()),
                            (92, lbs.clone()),
                        ],
                    );
                    for (l, ch) in [(&ln, b'n'), (&lt, b't'), (&lr, b'r'), (&lb, b'b'), (&lf, b'f'), (&lq, b'"'), (&lbs, b'\\')] {
                        f.block(l);
                        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "92")]);
                        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", &(ch as i64).to_string())]);
                        f.br(&le);
                    }
                    f.block(&ld);
                    put_u(f, &c);
                    f.br(&le);
                    f.block(&le);
                    let i1 = f.add(&i, "1");
                    f.store("i64", &i1, &iv);
                },
            );
            let i3 = f.load("i64", &iv);
            f.store("i64", &i3, &start);
        },
    );
    let s = f.load("i64", &start);
    let sp = f.off("%d", &s);
    let run = f.sub("%n", &s);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &sp), ("i64", &run)]);
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "34")]);
    f.ret_void();
    m.add(f);
}

fn dump(m: &mut Module) {
    // flags: 1 pretty, 2 ascii, 4 sort keys, 8 tab
    let snull = m.cstr("null");
    let sfalse = m.cstr("false");
    let strue = m.cstr("true");
    let sinv = m.cstr("<invalid>");
    m.global("@g_indent = internal global i64 2".into());

    let mut f = Func::new("internal", "void", "jv_newline_indent", &[("ptr", "b"), ("i64", "flags"), ("i64", "lvl")], "");
    let tab = f.bin("and", "i64", "%flags", "8");
    let ist = f.ne(&tab, "0");
    let w = f.load("i64", "@g_indent");
    let ns = f.bin("mul", "i64", &w, "%lvl");
    let n = f.select(&ist, "i64", "%lvl", &ns);
    let ch = f.select(&ist, "i32", "9", "32");
    let len = f.ld_at("i64", "%b", 8);
    let n1 = f.add(&n, "1");
    let need = f.add(&len, &n1);
    let cap = f.ld_at("i64", "%b", 16);
    let full = f.icmp("sgt", "i64", &need, &cap);
    f.if_then(&full, |f| f.call("void", "sb_grow", &[("ptr", "%b"), ("i64", &need)]));
    let d = f.ld_at("ptr", "%b", 0);
    let p = f.off(&d, &len);
    f.store("i8", "10", &p);
    let p1 = f.off(&p, "1");
    f.call("ptr", "memset", &[("ptr", &p1), ("i32", &ch), ("i64", &n)]);
    f.st_at("i64", &need, "%b", 8);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "jv_dump", &[("ptr", "b"), (V, "x"), ("i64", "flags"), ("i64", "lvl")], "");
    let t = f.tag("%x");
    let labels: Vec<String> = (0..7).map(|i| f.label(&format!("d{}", i))).collect();
    let dflt = f.label("dd");
    let cases: Vec<(i64, String)> = labels.iter().enumerate().map(|(i, l)| (i as i64, l.clone())).collect();
    f.switch(&t, &dflt, &cases);
    let pretty = {
        // computed in each block as needed
        "%flags"
    };
    let _ = pretty;
    for (i, s, n) in [(0, &snull, 4), (1, &sfalse, 5), (2, &strue, 4)] {
        f.block(&labels[i]);
        f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", s), ("i64", &n.to_string())]);
        f.ret_void();
    }
    f.block(&labels[3]);
    let raw = f.rawtag("%x");
    let haslit = f.icmp("ugt", "i64", &raw, "255");
    f.if_then(&haslit, |f| {
        let a = f.bin("lshr", "i64", &raw, "8");
        let p = f.cast("inttoptr", "i64", &a, "ptr");
        let l = f.ld_at("i64", &p, 8);
        let t = f.off(&p, "16");
        f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &t), ("i64", &l)]);
        f.ret_void();
    });
    let d = f.num_of("%x");
    f.call("void", "jv_dump_num", &[("ptr", "%b"), ("double", &d)]);
    f.ret_void();

    f.block(&labels[4]);
    let sp = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
    let sl = f.call("i64", "jv_str_len", &[(V, "%x")]);
    let asc = f.bin("and", "i64", "%flags", "2");
    f.call("void", "jv_dump_strbytes", &[("ptr", "%b"), ("ptr", &sp), ("i64", &sl), ("i64", &asc)]);
    f.ret_void();

    // array
    f.block(&labels[5]);
    let n = f.call("i64", "jv_arr_len", &[(V, "%x")]);
    let empty = f.eq(&n, "0");
    f.if_then(&empty, |f| {
        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "91")]);
        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "93")]);
        f.ret_void();
    });
    let pr = f.bin("and", "i64", "%flags", "1");
    let pretty = f.ne(&pr, "0");
    let lvl1 = f.add("%lvl", "1");
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "91")]);
    f.for_range("0", &n, |f, i, _, _| {
        let notfirst = f.ne(i, "0");
        f.if_then(&notfirst, |f| f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "44")]));
        f.if_then(&pretty, |f| {
            f.call("void", "jv_newline_indent", &[("ptr", "%b"), ("i64", "%flags"), ("i64", &lvl1)]);
        });
        let e = f.call(V, "jv_arr_get", &[(V, "%x"), ("i64", i)]);
        f.call("void", "jv_dump", &[("ptr", "%b"), (V, &e), ("i64", "%flags"), ("i64", &lvl1)]);
    });
    f.if_then(&pretty, |f| {
        f.call("void", "jv_newline_indent", &[("ptr", "%b"), ("i64", "%flags"), ("i64", "%lvl")]);
    });
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "93")]);
    f.ret_void();

    // object
    f.block(&labels[6]);
    let n = f.call("i64", "jv_obj_len", &[(V, "%x")]);
    let empty = f.eq(&n, "0");
    f.if_then(&empty, |f| {
        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "123")]);
        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "125")]);
        f.ret_void();
    });
    let pr = f.bin("and", "i64", "%flags", "1");
    let pretty = f.ne(&pr, "0");
    let lvl1 = f.add("%lvl", "1");
    let asc = f.bin("and", "i64", "%flags", "2");
    let so = f.bin("and", "i64", "%flags", "4");
    let sorted = f.ne(&so, "0");
    // order: either insertion order or sorted key list
    let keys = f.if_val(
        &sorted,
        V,
        |f| f.call(V, "jv_keys", &[(V, "%x")]),
        |_| NULL.to_string(),
    );
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "123")]);
    f.for_range("0", &n, |f, i, _, _| {
        let notfirst = f.ne(i, "0");
        f.if_then(&notfirst, |f| f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "44")]));
        f.if_then(&pretty, |f| {
            f.call("void", "jv_newline_indent", &[("ptr", "%b"), ("i64", "%flags"), ("i64", &lvl1)]);
        });
        let k = f.if_val(
            &sorted,
            V,
            |f| f.call(V, "jv_arr_get", &[(V, &keys), ("i64", i)]),
            |f| f.call(V, "jv_obj_key_at", &[(V, "%x"), ("i64", i)]),
        );
        let v = f.if_val(
            &sorted,
            V,
            |f| f.call(V, "jv_obj_get", &[(V, "%x"), (V, &k)]),
            |f| f.call(V, "jv_obj_val_at", &[(V, "%x"), ("i64", i)]),
        );
        let kp = f.call("ptr", "jv_str_ptr", &[(V, &k)]);
        let kl = f.call("i64", "jv_str_len", &[(V, &k)]);
        f.call("void", "jv_dump_strbytes", &[("ptr", "%b"), ("ptr", &kp), ("i64", &kl), ("i64", &asc)]);
        f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "58")]);
        f.if_then(&pretty, |f| f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "32")]));
        f.call("void", "jv_dump", &[("ptr", "%b"), (V, &v), ("i64", "%flags"), ("i64", &lvl1)]);
    });
    f.call("void", "jv_release", &[(V, &keys)]);
    f.if_then(&pretty, |f| {
        f.call("void", "jv_newline_indent", &[("ptr", "%b"), ("i64", "%flags"), ("i64", "%lvl")]);
    });
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "125")]);
    f.ret_void();

    f.block(&dflt);
    f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &sinv)]);
    f.ret_void();
    m.add(f);

    // tojson (compact) -> string
    let mut f = Func::new("internal", V, "jv_tojson", &[(V, "x")], "");
    let b = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &b), ("i64", "32")]);
    f.call("void", "jv_dump", &[("ptr", &b), (V, "%x"), ("i64", "0"), ("i64", "0")]);
    let s = f.call(V, "sb_finish", &[("ptr", &b)]);
    f.ret(V, &s);
    m.add(f);

    // tostring
    let mut f = Func::new("internal", V, "jv_tostring", &[(V, "x")], "");
    let isstr = f.tag_is("%x", T_STR);
    f.if_then(&isstr, |f| {
        f.call("void", "jv_retain", &[(V, "%x")]);
        f.ret(V, "%x");
    });
    let s = f.call(V, "jv_tojson", &[(V, "%x")]);
    f.ret(V, &s);
    m.add(f);

    // truncated dump for error messages (max 11 bytes + "...")
    let dots = m.cstr("...");
    // like jq's jv_dump_string_trunc with a buffer of `size` bytes
    let mut f = Func::new("internal", "void", "sb_dump_truncn", &[("ptr", "b"), (V, "x"), ("i64", "size")], "");
    let s = f.call(V, "jv_tojson", &[(V, "%x")]);
    let n = f.call("i64", "jv_str_len", &[(V, &s)]);
    let p = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
    let lim = f.sub("%size", "1");
    let cut = f.sub("%size", "4");
    let long = f.icmp("sgt", "i64", &n, &lim);
    f.if_else(
        &long,
        |f| {
            // cut at a UTF-8 boundary at or before 11 bytes
            let kv = f.var_init("i64", &cut);
            f.while_(
                |f| {
                    let k = f.load("i64", &kv);
                    let q = f.off(&p, &k);
                    let b = f.load("i8", &q);
                    let top = f.bin("and", "i8", &b, "-64");
                    let cont = f.icmp("eq", "i8", &top, "-128");
                    let pos = f.icmp("sgt", "i64", &k, "0");
                    f.and1(&cont, &pos)
                },
                |f, _, _| {
                    let k = f.load("i64", &kv);
                    let k1 = f.sub(&k, "1");
                    f.store("i64", &k1, &kv);
                },
            );
            let k = f.load("i64", &kv);
            f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &p), ("i64", &k)]);
            f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &dots), ("i64", "3")]);
        },
        |f| f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &p), ("i64", &n)]),
    );
    f.call("void", "jv_release", &[(V, &s)]);
    f.ret_void();
    m.add(f);

    let mut f = Func::new("internal", "void", "sb_dump_trunc", &[("ptr", "b"), (V, "x")], "");
    f.call("void", "sb_dump_truncn", &[("ptr", "%b"), (V, "%x"), ("i64", "15")]);
    f.ret_void();
    m.add(f);

    // "type (value)" description
    let mut f = Func::new("internal", "void", "sb_typedesc", &[("ptr", "b"), (V, "x")], "");
    let t = f.tag("%x");
    let kn = f.call("ptr", "jv_kind_name", &[("i64", &t)]);
    f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &kn)]);
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "32")]);
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "40")]);
    f.call("void", "sb_dump_trunc", &[("ptr", "%b"), (V, "%x")]);
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "41")]);
    f.ret_void();
    m.add(f);
}

/// Number literal preservation (jq >= 1.7): numbers whose canonical decimal
/// form (as jq's decNumber would print it) differs from our double formatting
/// keep that text. The text object {rc, len, bytes} hangs off the tag word.
fn numlit(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_numlit", &[("ptr", "s"), ("ptr", "e"), ("double", "d")], "");
    let si = f.cast("ptrtoint", "ptr", "%s", "i64");
    let ei = f.cast("ptrtoint", "ptr", "%e", "i64");
    let n = f.sub(&ei, &si);
    let iv = f.var_init("i64", "0");
    let ch = |f: &mut Func, i: &str| -> String {
        let inb = f.lt(i, &n);
        f.if_val(
            &inb,
            "i8",
            |f| {
                let p = f.off("%s", i);
                f.load("i8", &p)
            },
            |_| "0".into(),
        )
    };
    let isdig = |f: &mut Func, c: &str| -> String {
        let d = f.bin("sub", "i8", c, "48");
        f.icmp("ult", "i8", &d, "10")
    };
    let c0 = ch(&mut f, "0");
    let neg = f.icmp("eq", "i8", &c0, "45");
    let negi = f.cast("zext", "i1", &neg, "i64");
    f.store("i64", &negi, &iv);
    // digits buffer: integer then fraction digits
    let nb = f.add(&n, "8");
    let digs = f.call("ptr", "jv_malloc", &[("i64", &nb)]);
    let nd = f.var_init("i64", "0");
    let push = |f: &mut Func, c: &str| {
        let k = f.load("i64", &nd);
        let p = f.off(&digs, &k);
        f.store("i8", c, &p);
        let k1 = f.add(&k, "1");
        f.store("i64", &k1, &nd);
    };
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            let c = ch(f, &i);
            isdig(f, &c)
        },
        |f, _, _| {
            let i = f.load("i64", &iv);
            let c = ch(f, &i);
            push(f, &c);
            let i1 = f.add(&i, "1");
            f.store("i64", &i1, &iv);
        },
    );
    let nint = f.load("i64", &nd);
    let i = f.load("i64", &iv);
    let c = ch(&mut f, &i);
    let hasdot = f.icmp("eq", "i8", &c, "46");
    f.if_then(&hasdot, |f| {
        let i1 = f.add(&i, "1");
        f.store("i64", &i1, &iv);
        f.while_(
            |f| {
                let i = f.load("i64", &iv);
                let c = ch(f, &i);
                isdig(f, &c)
            },
            |f, _, _| {
                let i = f.load("i64", &iv);
                let c = ch(f, &i);
                push(f, &c);
                let i1 = f.add(&i, "1");
                f.store("i64", &i1, &iv);
            },
        );
    });
    let ntot = f.load("i64", &nd);
    let nfrac = f.sub(&ntot, &nint);
    let expv = f.var_init("i64", "0");
    let i = f.load("i64", &iv);
    let c = ch(&mut f, &i);
    let e1 = f.icmp("eq", "i8", &c, "101");
    let e2 = f.icmp("eq", "i8", &c, "69");
    let hasexp = f.or1(&e1, &e2);
    f.if_then(&hasexp, |f| {
        let i1 = f.add(&i, "1");
        let ep = f.off("%s", &i1);
        let x = f.call("double", "strtod", &[("ptr", &ep), ("ptr", "null")]);
        let xi = f.call("i64", "jv_dtoi", &[("double", &x)]);
        f.store("i64", &xi, &expv);
    });
    // leading zeros of the coefficient
    let lz = f.var_init("i64", "0");
    f.while_(
        |f| {
            let k = f.load("i64", &lz);
            let k1 = f.add(&k, "1");
            let more = f.lt(&k1, &ntot);
            f.if_val(
                &more,
                "i1",
                |f| {
                    let p = f.off(&digs, &k);
                    let b = f.load("i8", &p);
                    f.icmp("eq", "i8", &b, "48")
                },
                |_| "false".into(),
            )
        },
        |f, _, _| {
            let k = f.load("i64", &lz);
            let k1 = f.add(&k, "1");
            f.store("i64", &k1, &lz);
        },
    );
    let z = f.load("i64", &lz);
    let coef = f.off(&digs, &z);
    let clen = f.sub(&ntot, &z);
    // can our double formatting reproduce the literal exactly?
    let few = f.icmp("sle", "i64", &clen, "15");
    let noexp = f.not1(&hasexp);
    let hasfrac = f.icmp("sgt", "i64", &nfrac, "0");
    let lastzero = f.if_val(
        &hasfrac,
        "i1",
        |f| {
            let k = f.sub(&ntot, "1");
            let p = f.off(&digs, &k);
            let b = f.load("i8", &p);
            f.icmp("eq", "i8", &b, "48")
        },
        |_| "false".into(),
    );
    // value < 1e-4 prints in exponent form for us but plain for decNumber
    let small = {
        let lead = f.sub(&z, &nint);
        let intzero = f.icmp("sle", "i64", &nint, &z);
        let many = f.icmp("sge", "i64", &lead, "4");
        let x = f.and1(&intzero, &many);
        f.and1(&x, &hasfrac)
    };
    let ok1 = f.and1(&few, &noexp);
    let nlz = f.not1(&lastzero);
    let nsm = f.not1(&small);
    let ok2 = f.and1(&ok1, &nlz);
    let plain = f.and1(&ok2, &nsm);
    f.if_then(&plain, |f| {
        f.call("void", "free", &[("ptr", &digs)]);
        let v = f.mknum("%d");
        f.ret(V, &v);
    });
    // canonical decNumber to-scientific-string
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "32")]);
    f.st_at("i64", "16", &sb, 8); // reserve header space (rc, len)
    f.if_then(&neg, |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "45")]));
    let ex = f.load("i64", &expv);
    let e = f.sub(&ex, &nfrac);
    let cl1 = f.sub(&clen, "1");
    let adj = f.add(&e, &cl1);
    let le0 = f.icmp("sle", "i64", &e, "0");
    let ge6 = f.icmp("sge", "i64", &adj, "-6");
    let plainfmt = f.and1(&le0, &ge6);
    f.if_else(
        &plainfmt,
        |f| {
            let k = f.add(&clen, &e);
            let iszero = f.eq(&e, "0");
            let pos = f.icmp("sgt", "i64", &k, "0");
            f.if_else(
                &iszero,
                |f| f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &coef), ("i64", &clen)]),
                |f| {
                    f.if_else(
                        &pos,
                        |f| {
                            f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &coef), ("i64", &k)]);
                            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "46")]);
                            let p = f.off(&coef, &k);
                            let r = f.sub(&clen, &k);
                            f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &p), ("i64", &r)]);
                        },
                        |f| {
                            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "48")]);
                            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "46")]);
                            let nz = f.sub("0", &k);
                            f.for_range("0", &nz, |f, _, _, _| {
                                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "48")]);
                            });
                            f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &coef), ("i64", &clen)]);
                        },
                    );
                },
            );
        },
        |f| {
            f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &coef), ("i64", "1")]);
            let more = f.icmp("sgt", "i64", &clen, "1");
            f.if_then(&more, |f| {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "46")]);
                let p = f.off(&coef, "1");
                f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &p), ("i64", &cl1)]);
            });
            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "69")]);
            let nonneg = f.icmp("sge", "i64", &adj, "0");
            let sign = f.select(&nonneg, "i8", "43", "45");
            f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &sign)]);
            let na = f.sub("0", &adj);
            let aa = f.select(&nonneg, "i64", &adj, &na);
            f.call("void", "sb_int", &[("ptr", &sb), ("i64", &aa)]);
        },
    );
    f.call("void", "free", &[("ptr", &digs)]);
    let obj = f.ld_at("ptr", &sb, 0);
    let tl = f.ld_at("i64", &sb, 8);
    let tlen = f.sub(&tl, "16");
    f.st_at("i64", "1", &obj, 0);
    f.st_at("i64", &tlen, &obj, 8);
    let oi = f.cast("ptrtoint", "ptr", &obj, "i64");
    let sh = f.bin("shl", "i64", &oi, "8");
    let tag = f.bin("or", "i64", &sh, "3");
    let b = f.cast("bitcast", "double", "%d", "i64");
    let v = f.mkv(&tag, &b);
    f.ret(V, &v);
    m.add(f);
}

/// Exact decimal comparison / negation for preserved number literals.
pub fn numlit_ops(m: &mut Module) {
    // parse literal text into (neg, digit buffer without leading/trailing zeros, exponent)
    // returns the adjusted magnitude exponent (exp + ndigits) via out params.
    let mut f = Func::new(
        "internal",
        "void",
        "jv_lit_parse",
        &[("ptr", "lit"), ("ptr", "dig"), ("ptr", "pnd"), ("ptr", "pexp"), ("ptr", "pneg")],
        "",
    );
    let n = f.ld_at("i64", "%lit", 8);
    let t = f.off("%lit", "16");
    let nd = f.var_init("i64", "0");
    let fracv = f.var_init("i64", "0");
    let infrac = f.var_init("i1", "false");
    let expv = f.var_init("i64", "0");
    f.store("i1", "false", "%pneg");
    let iv = f.var_init("i64", "0");
    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, &n)
        },
        |f, brk, _| {
            let i = f.load("i64", &iv);
            let p = f.off(&t, &i);
            let c = f.load("i8", &p);
            let i1 = f.add(&i, "1");
            f.store("i64", &i1, &iv);
            let isneg = f.icmp("eq", "i8", &c, "45");
            f.if_then(&isneg, |f| f.store("i1", "true", "%pneg"));
            let isdot = f.icmp("eq", "i8", &c, "46");
            f.if_then(&isdot, |f| f.store("i1", "true", &infrac));
            let ise = f.icmp("eq", "i8", &c, "69");
            f.if_then(&ise, |f| {
                let ep = f.off(&t, &i1);
                let x = f.call("double", "strtod", &[("ptr", &ep), ("ptr", "null")]);
                let xi = f.call("i64", "jv_dtoi", &[("double", &x)]);
                f.store("i64", &xi, &expv);
                f.br(brk);
            });
            let d = f.bin("sub", "i8", &c, "48");
            let isd = f.icmp("ult", "i8", &d, "10");
            f.if_then(&isd, |f| {
                let k = f.load("i64", &nd);
                // skip leading zeros
                let isz = f.icmp("eq", "i8", &d, "0");
                let first = f.eq(&k, "0");
                let skip = f.and1(&isz, &first);
                let fr = f.load("i1", &infrac);
                f.if_then(&fr, |f| {
                    let x = f.load("i64", &fracv);
                    let x1 = f.add(&x, "1");
                    f.store("i64", &x1, &fracv);
                });
                let keep = f.not1(&skip);
                f.if_then(&keep, |f| {
                    let q = f.off("%dig", &k);
                    f.store("i8", &d, &q);
                    let k1 = f.add(&k, "1");
                    f.store("i64", &k1, &nd);
                });
            });
        },
    );
    // strip trailing zeros
    f.while_(
        |f| {
            let k = f.load("i64", &nd);
            let pos = f.icmp("sgt", "i64", &k, "0");
            f.if_val(
                &pos,
                "i1",
                |f| {
                    let k1 = f.sub(&k, "1");
                    let q = f.off("%dig", &k1);
                    let b = f.load("i8", &q);
                    f.icmp("eq", "i8", &b, "0")
                },
                |_| "false".into(),
            )
        },
        |f, _, _| {
            let k = f.load("i64", &nd);
            let k1 = f.sub(&k, "1");
            f.store("i64", &k1, &nd);
            let x = f.load("i64", &fracv);
            let x1 = f.sub(&x, "1");
            f.store("i64", &x1, &fracv);
        },
    );
    let k = f.load("i64", &nd);
    f.store("i64", &k, "%pnd");
    let e = f.load("i64", &expv);
    let fr = f.load("i64", &fracv);
    let e1 = f.sub(&e, &fr);
    let adj = f.add(&e1, &k);
    f.store("i64", &adj, "%pexp");
    f.ret_void();
    m.add(f);

    // i32 jv_lit_cmp(ptr la, ptr lb)
    let mut f = Func::new("internal", "i32", "jv_lit_cmp", &[("ptr", "a"), ("ptr", "b")], "");
    let la = f.ld_at("i64", "%a", 8);
    let lb = f.ld_at("i64", "%b", 8);
    let da = f.call("ptr", "jv_malloc", &[("i64", &la)]);
    let db = f.call("ptr", "jv_malloc", &[("i64", &lb)]);
    let (nda, nea, nga) = (f.var("i64"), f.var("i64"), f.var("i1"));
    let (ndb, neb, ngb) = (f.var("i64"), f.var("i64"), f.var("i1"));
    f.call("void", "jv_lit_parse", &[("ptr", "%a"), ("ptr", &da), ("ptr", &nda), ("ptr", &nea), ("ptr", &nga)]);
    f.call("void", "jv_lit_parse", &[("ptr", "%b"), ("ptr", &db), ("ptr", &ndb), ("ptr", &neb), ("ptr", &ngb)]);
    let na = f.load("i64", &nda);
    let nb = f.load("i64", &ndb);
    let ea = f.load("i64", &nea);
    let eb = f.load("i64", &neb);
    let ga = f.load("i1", &nga);
    let gb = f.load("i1", &ngb);
    let res = f.var_init("i32", "0");
    let za = f.eq(&na, "0");
    let zb = f.eq(&nb, "0");
    let lend = f.label("litcmp_end");
    // signs (zero has no sign)
    let ga = {
        let nz = f.not1(&za);
        f.and1(&ga, &nz)
    };
    let gb = {
        let nz = f.not1(&zb);
        f.and1(&gb, &nz)
    };
    let both0 = f.and1(&za, &zb);
    f.if_then(&both0, |f| f.br(&lend));
    // sign value: -1 negative, 0 zero, 1 positive
    let sa = {
        let p = f.select(&za, "i32", "0", "1");
        f.select(&ga, "i32", "-1", &p)
    };
    let sb_ = {
        let p = f.select(&zb, "i32", "0", "1");
        f.select(&gb, "i32", "-1", &p)
    };
    let sdiff = f.icmp("ne", "i32", &sa, &sb_);
    f.if_then(&sdiff, |f| {
        let lt = f.icmp("slt", "i32", &sa, &sb_);
        let r = f.select(&lt, "i32", "-1", "1");
        f.store("i32", &r, &res);
        f.br(&lend);
    });
    // same sign, both nonzero: compare magnitudes
    let mag = f.var_init("i32", "0");
    let ediff = f.ne(&ea, &eb);
    f.if_else(
        &ediff,
        |f| {
            let lt = f.lt(&ea, &eb);
            let r = f.select(&lt, "i32", "-1", "1");
            f.store("i32", &r, &mag);
        },
        |f| {
            let lt = f.lt(&na, &nb);
            let mn = f.select(&lt, "i64", &na, &nb);
            let c = f.call("i32", "memcmp", &[("ptr", &da), ("ptr", &db), ("i64", &mn)]);
            let nz = f.icmp("ne", "i32", &c, "0");
            f.if_else(
                &nz,
                |f| {
                    let l = f.icmp("slt", "i32", &c, "0");
                    let r = f.select(&l, "i32", "-1", "1");
                    f.store("i32", &r, &mag);
                },
                |f| {
                    let gt = f.icmp("sgt", "i64", &na, &nb);
                    let r1 = f.select(&gt, "i32", "1", "0");
                    let r = f.select(&lt, "i32", "-1", &r1);
                    f.store("i32", &r, &mag);
                },
            );
        },
    );
    let mg = f.load("i32", &mag);
    let neg32 = f.ins(format!("sub i32 0, {}", mg));
    let r = f.select(&ga, "i32", &neg32, &mg);
    f.store("i32", &r, &res);
    f.br(&lend);
    f.block(&lend);
    f.call("void", "free", &[("ptr", &da)]);
    f.call("void", "free", &[("ptr", &db)]);
    let r = f.load("i32", &res);
    f.ret("i32", &r);
    m.add(f);

    // negate a literal number: new literal with the sign toggled
    let mut f = Func::new("internal", V, "jv_lit_neg", &[(V, "x")], "");
    let raw = f.rawtag("%x");
    let a = f.bin("lshr", "i64", &raw, "8");
    let p = f.cast("inttoptr", "i64", &a, "ptr");
    let n = f.ld_at("i64", &p, 8);
    let t = f.off(&p, "16");
    let c0 = f.load("i8", &t);
    let isneg = f.icmp("eq", "i8", &c0, "45");
    let n1 = f.add(&n, "1");
    let sz = f.add(&n1, "16");
    let q = f.call("ptr", "jv_malloc", &[("i64", &sz)]);
    f.st_at("i64", "1", &q, 0);
    let qt = f.off(&q, "16");
    f.if_else(
        &isneg,
        |f| {
            let nl = f.sub(&n, "1");
            f.st_at("i64", &nl, &q, 8);
            let t1 = f.off(&t, "1");
            f.call("ptr", "memcpy", &[("ptr", &qt), ("ptr", &t1), ("i64", &nl)]);
        },
        |f| {
            f.st_at("i64", &n1, &q, 8);
            f.store("i8", "45", &qt);
            let q1 = f.off(&qt, "1");
            f.call("ptr", "memcpy", &[("ptr", &q1), ("ptr", &t), ("i64", &n)]);
        },
    );
    let d = f.num_of("%x");
    let nd = f.ins(format!("fneg double {}", d));
    let b = f.cast("bitcast", "double", &nd, "i64");
    let qi = f.cast("ptrtoint", "ptr", &q, "i64");
    let sh = f.bin("shl", "i64", &qi, "8");
    let tag = f.bin("or", "i64", &sh, "3");
    let v = f.mkv(&tag, &b);
    f.ret(V, &v);
    m.add(f);
}

/// Compare two numbers exactly when both carry literals; returns 2 otherwise.
pub fn numlit_cmp2(m: &mut Module) {
    let mut f = Func::new("internal", "i32", "jv_numlit_cmp2", &[(V, "a"), (V, "b")], "");
    let ra = f.rawtag("%a");
    let rb = f.rawtag("%b");
    let la = f.icmp("ugt", "i64", &ra, "255");
    let lb = f.icmp("ugt", "i64", &rb, "255");
    let both = f.and1(&la, &lb);
    let no = f.not1(&both);
    f.if_then(&no, |f| f.ret("i32", "2"));
    let a = f.bin("lshr", "i64", &ra, "8");
    let pa = f.cast("inttoptr", "i64", &a, "ptr");
    let b = f.bin("lshr", "i64", &rb, "8");
    let pb = f.cast("inttoptr", "i64", &b, "ptr");
    let r = f.call("i32", "jv_lit_cmp", &[("ptr", &pa), ("ptr", &pb)]);
    f.ret("i32", &r);
    m.add(f);
}

/// Coloured output (jq -C): each token and each punctuation mark is wrapped
/// in ESC[<color>m ... ESC[0m. Colours: null false true numbers strings
/// arrays objects object-keys (overridable via JQ_COLORS).
pub fn dump_color(m: &mut Module) {
    let defaults = ["0;90", "0;39", "0;39", "0;39", "0;32", "1;39", "1;39", "1;34"];
    let gs: Vec<String> = defaults.iter().map(|c| m.cstr(c)).collect();
    m.global(format!(
        "@g_colors = internal global [8 x ptr] [{}]",
        gs.iter().map(|g| format!("ptr {}", g)).collect::<Vec<_>>().join(", ")
    ));
    let esc = m.cstr("\x1b[");
    let reset = m.cstr("\x1b[0m");

    // start colour #i
    let mut f = Func::new("internal", "void", "jv_col", &[("ptr", "b"), ("i64", "i")], "");
    let p = f.idx("@g_colors", "%i", 8);
    let c = f.load("ptr", &p);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &esc), ("i64", "2")]);
    f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &c)]);
    f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "109")]);
    f.ret_void();
    m.add(f);

    // coloured punctuation: ESC[col m <ch> ESC[0m
    let mut f = Func::new("internal", "void", "jv_colpunct", &[("ptr", "b"), ("i64", "i"), ("ptr", "s"), ("i64", "n")], "");
    f.call("void", "jv_col", &[("ptr", "%b"), ("i64", "%i")]);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", "%s"), ("i64", "%n")]);
    f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &reset), ("i64", "4")]);
    f.ret_void();
    m.add(f);

    let lb = m.cstr("[");
    let rb = m.cstr("]");
    let lc = m.cstr("{");
    let rc = m.cstr("}");
    let comma = m.cstr(",");
    let colon = m.cstr(":");
    let empty_a = m.cstr("[]");
    let empty_o = m.cstr("{}");

    let mut f = Func::new("internal", "void", "jv_dump_color", &[("ptr", "b"), (V, "x"), ("i64", "flags"), ("i64", "lvl")], "");
    let t = f.tag("%x");
    let pr = f.bin("and", "i64", "%flags", "1");
    let pretty = f.ne(&pr, "0");
    let lvl1 = f.add("%lvl", "1");
    let isarr = f.eq(&t, "5");
    let isobj = f.eq(&t, "6");
    let container = f.or1(&isarr, &isobj);
    let ncont = f.not1(&container);
    f.if_then(&ncont, |f| {
        // scalars: colour index = tag (null 0, false 1, true 2, number 3, string 4)
        f.call("void", "jv_col", &[("ptr", "%b"), ("i64", &t)]);
        f.call("void", "jv_dump", &[("ptr", "%b"), (V, "%x"), ("i64", "%flags"), ("i64", "%lvl")]);
        f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &reset), ("i64", "4")]);
        f.ret_void();
    });
    let ci = f.select(&isarr, "i64", "5", "6");
    let n = f.if_val(&isarr, "i64", |f| f.call("i64", "jv_arr_len", &[(V, "%x")]), |f| f.call("i64", "jv_obj_len", &[(V, "%x")]));
    let empty = f.eq(&n, "0");
    f.if_then(&empty, |f| {
        let s = f.select(&isarr, "ptr", &empty_a, &empty_o);
        f.call("void", "jv_colpunct", &[("ptr", "%b"), ("i64", &ci), ("ptr", &s), ("i64", "2")]);
        f.ret_void();
    });
    let open = f.select(&isarr, "ptr", &lb, &lc);
    let close = f.select(&isarr, "ptr", &rb, &rc);
    f.call("void", "jv_colpunct", &[("ptr", "%b"), ("i64", &ci), ("ptr", &open), ("i64", "1")]);
    let so = f.bin("and", "i64", "%flags", "4");
    let sorted = f.ne(&so, "0");
    let osorted = f.and1(&sorted, &isobj);
    let keys = f.if_val(&osorted, V, |f| f.call(V, "jv_keys", &[(V, "%x")]), |_| NULL.to_string());
    f.for_range("0", &n, |f, i, _, _| {
        let nf = f.ne(i, "0");
        f.if_then(&nf, |f| f.call("void", "jv_colpunct", &[("ptr", "%b"), ("i64", &ci), ("ptr", &comma), ("i64", "1")]));
        f.if_then(&pretty, |f| f.call("void", "jv_newline_indent", &[("ptr", "%b"), ("i64", "%flags"), ("i64", &lvl1)]));
        f.if_else(
            &isarr,
            |f| {
                let e = f.call(V, "jv_arr_get", &[(V, "%x"), ("i64", i)]);
                f.call("void", "jv_dump_color", &[("ptr", "%b"), (V, &e), ("i64", "%flags"), ("i64", &lvl1)]);
            },
            |f| {
                let k = f.if_val(
                    &osorted,
                    V,
                    |f| f.call(V, "jv_arr_get", &[(V, &keys), ("i64", i)]),
                    |f| f.call(V, "jv_obj_key_at", &[(V, "%x"), ("i64", i)]),
                );
                let v = f.if_val(
                    &osorted,
                    V,
                    |f| f.call(V, "jv_obj_get", &[(V, "%x"), (V, &k)]),
                    |f| f.call(V, "jv_obj_val_at", &[(V, "%x"), ("i64", i)]),
                );
                f.call("void", "jv_col", &[("ptr", "%b"), ("i64", "7")]);
                f.call("void", "jv_dump", &[("ptr", "%b"), (V, &k), ("i64", "%flags"), ("i64", "0")]);
                f.call("void", "sb_write", &[("ptr", "%b"), ("ptr", &reset), ("i64", "4")]);
                f.call("void", "jv_colpunct", &[("ptr", "%b"), ("i64", &ci), ("ptr", &colon), ("i64", "1")]);
                f.if_then(&pretty, |f| f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "32")]));
                f.call("void", "jv_dump_color", &[("ptr", "%b"), (V, &v), ("i64", "%flags"), ("i64", &lvl1)]);
            },
        );
    });
    f.call("void", "jv_release", &[(V, &keys)]);
    f.if_then(&pretty, |f| f.call("void", "jv_newline_indent", &[("ptr", "%b"), ("i64", "%flags"), ("i64", "%lvl")]));
    f.call("void", "jv_colpunct", &[("ptr", "%b"), ("i64", &ci), ("ptr", &close), ("i64", "1")]);
    f.ret_void();
    m.add(f);

    // JQ_COLORS="null:false:true:numbers:strings:arrays:objects:objkeys"
    let name = m.cstr("JQ_COLORS");
    let mut f = Func::new("internal", "void", "jq_load_colors", &[], "");
    let v = f.call("ptr", "getenv", &[("ptr", &name)]);
    let none = f.icmp("eq", "ptr", &v, "null");
    f.if_then(&none, |f| f.ret_void());
    let n = f.call("i64", "strlen", &[("ptr", &v)]);
    let n1 = f.add(&n, "1");
    let copy = f.call("ptr", "jv_malloc", &[("i64", &n1)]);
    f.call("ptr", "memcpy", &[("ptr", &copy), ("ptr", &v), ("i64", &n1)]);
    let idx = f.var_init("i64", "0");
    let start = f.var_init("ptr", &copy);
    f.for_range("0", &n1, |f, i, brk, _| {
        let p = f.off(&copy, i);
        let b = f.load("i8", &p);
        let sep = f.icmp("eq", "i8", &b, "58");
        let end = f.icmp("eq", "i8", &b, "0");
        let cut = f.or1(&sep, &end);
        f.if_then(&cut, |f| {
            f.store("i8", "0", &p);
            let k = f.load("i64", &idx);
            let s = f.load("ptr", &start);
            let slot = f.idx("@g_colors", &k, 8);
            f.store("ptr", &s, &slot);
            let k1 = f.add(&k, "1");
            f.store("i64", &k1, &idx);
            let p1 = f.off(&p, "1");
            f.store("ptr", &p1, &start);
            let full = f.icmp("sge", "i64", &k1, "8");
            f.if_then(&full, |f| f.br(brk));
        });
    });
    f.ret_void();
    m.add(f);
}
