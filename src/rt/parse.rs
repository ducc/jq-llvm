//! JSON parser with jq-compatible error messages.
//!
//! Recursive descent over a NUL-terminated buffer; the cursor lives at *pc.
//! Like jq, bare words ("literals") are scanned up to the next delimiter and
//! then classified, and an error is located just after the token that
//! triggered it. The error position is left in @g_perr_pos so callers can
//! render "<msg> at line L, column C" (or "at EOF at ...").

use super::*;
use crate::ir::{fconst, Func, Module, V};

pub fn emit(m: &mut Module) {
    m.global("@g_perr_pos = internal global ptr null".into());
    m.global("@g_perr_eof = internal global i1 false".into());
    m.global("@g_keycache = internal global [1024 x %V] zeroinitializer".into());
    basics(m);
    strings(m);
    literal(m);
    value(m);
    top(m);
    messages(m);
}

fn basics(m: &mut Module) {
    // skip whitespace: cursor stored at *pc
    let mut f = Func::new("internal", "void", "jp_ws", &[("ptr", "pc")], "alwaysinline");
    let c0 = f.load("ptr", "%pc");
    let cv = f.var_init("ptr", &c0);
    f.while_(
        |f| {
            let c = f.load("ptr", &cv);
            let b = f.load("i8", &c);
            let sp = f.icmp("eq", "i8", &b, "32");
            let nl = f.icmp("eq", "i8", &b, "10");
            let cr = f.icmp("eq", "i8", &b, "13");
            let tb = f.icmp("eq", "i8", &b, "9");
            let a = f.or1(&sp, &nl);
            let b2 = f.or1(&cr, &tb);
            f.or1(&a, &b2)
        },
        |f, _, _| {
            let c = f.load("ptr", &cv);
            let c1 = f.off(&c, "1");
            f.store("ptr", &c1, &cv);
        },
    );
    let c = f.load("ptr", &cv);
    f.store("ptr", &c, "%pc");
    f.ret_void();
    m.add(f);

    // error at position
    for (name, eof) in [("jp_err", "false"), ("jp_err_eof", "true")] {
        let mut f = Func::new("internal", V, name, &[("ptr", "msg"), ("ptr", "pos")], "noinline");
        f.store("ptr", "%pos", "@g_perr_pos");
        f.store("i1", eof, "@g_perr_eof");
        let e = f.call(V, "jv_err_c", &[("ptr", "%msg")]);
        f.ret(V, &e);
        m.add(f);
    }

    // delimiter: whitespace, structural characters, quote, NUL
    let mut f = Func::new("internal", "i1", "jp_isdelim", &[("i8", "c")], "alwaysinline");
    let mut acc = f.icmp("eq", "i8", "%c", "0");
    for d in [b' ', b'\t', b'\n', b'\r', b'[', b']', b'{', b'}', b',', b':', b'"'] {
        let e = f.icmp("eq", "i8", "%c", &(d as i64).to_string());
        acc = f.or1(&acc, &e);
    }
    f.ret("i1", &acc);
    m.add(f);

    // end position of the token starting at p (where jq would report an error)
    let mut f = Func::new("internal", "ptr", "jp_token_end", &[("ptr", "p")], "");
    let b = f.load("i8", "%p");
    let isq = f.icmp("eq", "i8", &b, "34");
    f.if_then(&isq, |f| {
        let cv = f.var_init("ptr", "%p");
        f.while_(
            |_| "true".into(),
            |f, brk, _| {
                let c = f.load("ptr", &cv);
                let c1 = f.off(&c, "1");
                let b = f.load("i8", &c1);
                f.store("ptr", &c1, &cv);
                let z = f.icmp("eq", "i8", &b, "0");
                f.if_then(&z, |f| f.ret("ptr", &c1));
                let q = f.icmp("eq", "i8", &b, "34");
                f.if_then(&q, |f| {
                    let c2 = f.off(&c1, "1");
                    f.ret("ptr", &c2);
                });
                let bs = f.icmp("eq", "i8", &b, "92");
                f.if_then(&bs, |f| {
                    let c2 = f.off(&c1, "1");
                    let b2 = f.load("i8", &c2);
                    let z = f.icmp("eq", "i8", &b2, "0");
                    f.if_then(&z, |f| f.ret("ptr", &c2));
                    f.store("ptr", &c2, &cv);
                });
                let _ = brk;
            },
        );
        f.unreachable();
    });
    let z = f.icmp("eq", "i8", &b, "0");
    f.if_then(&z, |f| f.ret("ptr", "%p"));
    let d = f.call("i1", "jp_isdelim", &[("i8", &b)]);
    f.if_then(&d, |f| {
        let p1 = f.off("%p", "1");
        f.ret("ptr", &p1);
    });
    let cv = f.var_init("ptr", "%p");
    f.while_(
        |f| {
            let c = f.load("ptr", &cv);
            let b = f.load("i8", &c);
            let d = f.call("i1", "jp_isdelim", &[("i8", &b)]);
            f.not1(&d)
        },
        |f, _, _| {
            let c = f.load("ptr", &cv);
            let c1 = f.off(&c, "1");
            f.store("ptr", &c1, &cv);
        },
    );
    let c = f.load("ptr", &cv);
    let b = f.load("i8", &c);
    let z = f.icmp("eq", "i8", &b, "0");
    let c1 = f.off(&c, "1");
    let r = f.select(&z, "ptr", &c, &c1);
    f.ret("ptr", &r);
    m.add(f);

    // hex digit value or -1
    let mut f = Func::new("internal", "i64", "hexval", &[("i8", "c")], "");
    let c = f.cast("zext", "i8", "%c", "i64");
    let d0 = f.sub(&c, "48");
    let isd = f.icmp("ult", "i64", &d0, "10");
    f.if_then(&isd, |f| f.ret("i64", &d0));
    let lc = f.bin("or", "i64", &c, "32");
    let a0 = f.sub(&lc, "97");
    let isa = f.icmp("ult", "i64", &a0, "6");
    f.if_then(&isa, |f| {
        let r = f.add(&a0, "10");
        f.ret("i64", &r);
    });
    f.ret("i64", "-1");
    m.add(f);

    // read 4 hex digits at p; -1 if invalid
    let mut f = Func::new("internal", "i64", "hex4", &[("ptr", "p")], "");
    let acc = f.var_init("i64", "0");
    f.for_range("0", "4", |f, i, _, _| {
        let q = f.off("%p", i);
        let c = f.load("i8", &q);
        let h = f.call("i64", "hexval", &[("i8", &c)]);
        let bad = f.lt(&h, "0");
        f.if_then(&bad, |f| f.ret("i64", "-1"));
        let a = f.load("i64", &acc);
        let s = f.bin("shl", "i64", &a, "4");
        let r = f.bin("or", "i64", &s, &h);
        f.store("i64", &r, &acc);
    });
    let r = f.load("i64", &acc);
    f.ret("i64", &r);
    m.add(f);
}

fn strings(m: &mut Module) {
    let e_unf = m.cstr("Unfinished string");
    let e_esc = m.cstr("Invalid escape");
    let e_uesc = m.cstr("Invalid \\uXXXX escape");
    let e_ctl = m.cstr("Invalid string: control characters from U+0000 through U+001F must be escaped");

    // string: *pc points at the opening quote
    let mut f = Func::new("internal", V, "jp_string", &[("ptr", "pc")], "");
    let c0 = f.load("ptr", "%pc");
    let start = f.off(&c0, "1");
    let cv = f.var_init("ptr", &start);
    // fast scan: plain bytes up to the closing quote
    f.while_(
        |f| {
            let c = f.load("ptr", &cv);
            let b = f.load("i8", &c);
            let q = f.icmp("ne", "i8", &b, "34");
            let bs = f.icmp("ne", "i8", &b, "92");
            let ctl = f.icmp("uge", "i8", &b, "32");
            let a = f.and1(&q, &bs);
            f.and1(&a, &ctl)
        },
        |f, _, _| {
            let c = f.load("ptr", &cv);
            let c1 = f.off(&c, "1");
            f.store("ptr", &c1, &cv);
        },
    );
    let c = f.load("ptr", &cv);
    let b = f.load("i8", &c);
    let done = f.icmp("eq", "i8", &b, "34");
    f.if_then(&done, |f| {
        let si = f.cast("ptrtoint", "ptr", &start, "i64");
        let ci = f.cast("ptrtoint", "ptr", &c, "i64");
        let n = f.sub(&ci, &si);
        let s = f.call(V, "jv_str", &[("ptr", &start), ("i64", &n)]);
        let c1 = f.off(&c, "1");
        f.store("ptr", &c1, "%pc");
        f.ret(V, &s);
    });
    // slow path with a builder; the first error is reported after the closing quote
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    let si = f.cast("ptrtoint", "ptr", &start, "i64");
    let ci = f.cast("ptrtoint", "ptr", &c, "i64");
    let n = f.sub(&ci, &si);
    f.call("void", "sb_write", &[("ptr", &sb), ("ptr", &start), ("i64", &n)]);
    let errv = f.var_init("ptr", "null");
    let seterr = |f: &mut Func, msg: &str| {
        let cur = f.load("ptr", &errv);
        let none = f.icmp("eq", "ptr", &cur, "null");
        f.if_then(&none, |f| f.store("ptr", msg, &errv));
    };
    f.while_(
        |_| "true".into(),
        |f, _, _| {
            let c = f.load("ptr", &cv);
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let iseof = f.icmp("eq", "i8", &b, "0");
            f.if_then(&iseof, |f| {
                let d = f.ld_at("ptr", &sb, 0);
                f.call("void", "free", &[("ptr", &d)]);
                let e = f.call(V, "jp_err_eof", &[("ptr", &e_unf), ("ptr", &c)]);
                f.ret(V, &e);
            });
            let isq = f.icmp("eq", "i8", &b, "34");
            f.if_then(&isq, |f| {
                f.store("ptr", &c1, "%pc");
                let em = f.load("ptr", &errv);
                let haserr = f.icmp("ne", "ptr", &em, "null");
                f.if_then(&haserr, |f| {
                    let d = f.ld_at("ptr", &sb, 0);
                    f.call("void", "free", &[("ptr", &d)]);
                    let e = f.call(V, "jp_err", &[("ptr", &em), ("ptr", &c1)]);
                    f.ret(V, &e);
                });
                let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
                f.ret(V, &s);
            });
            let isctl = f.icmp("ult", "i8", &b, "32");
            f.if_then(&isctl, |f| seterr(f, &e_ctl));
            let isbs = f.icmp("eq", "i8", &b, "92");
            let notbs = f.not1(&isbs);
            f.if_then(&notbs, |f| {
                f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &b)]);
                f.store("ptr", &c1, &cv);
            });
            f.if_then(&isbs, |f| {
                let e = f.load("i8", &c1);
                let eeof = f.icmp("eq", "i8", &e, "0");
                f.if_then(&eeof, |f| {
                    let d = f.ld_at("ptr", &sb, 0);
                    f.call("void", "free", &[("ptr", &d)]);
                    let er = f.call(V, "jp_err_eof", &[("ptr", &e_unf), ("ptr", &c1)]);
                    f.ret(V, &er);
                });
                let c2 = f.off(&c, "2");
                f.store("ptr", &c2, &cv);
                let (lu, ld, le) = (f.label("u"), f.label("simple"), f.label("escdone"));
                let isu = f.icmp("eq", "i8", &e, "117");
                f.cbr(&isu, &lu, &ld);
                f.block(&ld);
                let pairs: [(u8, u8); 8] = [
                    (b'n', b'\n'),
                    (b't', b'\t'),
                    (b'r', b'\r'),
                    (b'b', 8),
                    (b'f', 12),
                    (b'/', b'/'),
                    (b'\\', b'\\'),
                    (b'"', b'"'),
                ];
                let out = f.var_init("i8", "0");
                for (k, v) in pairs {
                    let is = f.icmp("eq", "i8", &e, &(k as i64).to_string());
                    f.if_then(&is, |f| f.store("i8", &(v as i64).to_string(), &out));
                }
                let o = f.load("i8", &out);
                let bad = f.icmp("eq", "i8", &o, "0");
                f.if_else(&bad, |f| seterr(f, &e_esc), |f| f.call("void", "sb_putc", &[("ptr", &sb), ("i8", &o)]));
                f.br(&le);
                f.block(&lu);
                let h = f.call("i64", "hex4", &[("ptr", &c2)]);
                let bad = f.lt(&h, "0");
                let (lbad, lgood) = (f.label("ubad"), f.label("ugood"));
                f.cbr(&bad, &lbad, &lgood);
                f.block(&lbad);
                seterr(f, &e_uesc);
                f.br(&le);
                f.block(&lgood);
                let c6 = f.off(&c, "6");
                f.store("ptr", &c6, &cv);
                let cpv = f.var_init("i64", &h);
                let hs = {
                    let x = f.icmp("sge", "i64", &h, "55296");
                    let y = f.icmp("slt", "i64", &h, "56320");
                    f.and1(&x, &y)
                };
                f.if_then(&hs, |f| {
                    let b6 = f.load("i8", &c6);
                    let c7 = f.off(&c, "7");
                    let b7 = f.load("i8", &c7);
                    let x = f.icmp("eq", "i8", &b6, "92");
                    let y = f.icmp("eq", "i8", &b7, "117");
                    let pair = f.and1(&x, &y);
                    f.if_else(
                        &pair,
                        |f| {
                            let c8 = f.off(&c, "8");
                            let l = f.call("i64", "hex4", &[("ptr", &c8)]);
                            let lo = {
                                let x = f.icmp("sge", "i64", &l, "56320");
                                let y = f.icmp("slt", "i64", &l, "57344");
                                f.and1(&x, &y)
                            };
                            f.if_else(
                                &lo,
                                |f| {
                                    let a = f.sub(&h, "55296");
                                    let a = f.bin("shl", "i64", &a, "10");
                                    let b = f.sub(&l, "56320");
                                    let r = f.add(&a, &b);
                                    let r = f.add(&r, "65536");
                                    f.store("i64", &r, &cpv);
                                    let c12 = f.off(&c, "12");
                                    f.store("ptr", &c12, &cv);
                                },
                                |f| f.store("i64", "65533", &cpv),
                            );
                        },
                        |f| f.store("i64", "65533", &cpv),
                    );
                });
                let lone = {
                    let x = f.icmp("sge", "i64", &h, "56320");
                    let y = f.icmp("slt", "i64", &h, "57344");
                    f.and1(&x, &y)
                };
                f.if_then(&lone, |f| f.store("i64", "65533", &cpv));
                let cp = f.load("i64", &cpv);
                f.call("void", "sb_utf8", &[("ptr", &sb), ("i64", &cp)]);
                f.br(&le);
                f.block(&le);
            });
        },
    );
    f.unreachable();
    m.add(f);

    // object key: interned through a small cache so repeated keys share one string
    let mut f = Func::new("internal", V, "jp_key", &[("ptr", "pc")], "");
    let c0 = f.load("ptr", "%pc");
    let start = f.off(&c0, "1");
    let cv = f.var_init("ptr", &start);
    f.while_(
        |f| {
            let c = f.load("ptr", &cv);
            let b = f.load("i8", &c);
            let q = f.icmp("ne", "i8", &b, "34");
            let bs = f.icmp("ne", "i8", &b, "92");
            let ctl = f.icmp("uge", "i8", &b, "32");
            let a = f.and1(&q, &bs);
            f.and1(&a, &ctl)
        },
        |f, _, _| {
            let c = f.load("ptr", &cv);
            let c1 = f.off(&c, "1");
            f.store("ptr", &c1, &cv);
        },
    );
    let c = f.load("ptr", &cv);
    let b = f.load("i8", &c);
    let si = f.cast("ptrtoint", "ptr", &start, "i64");
    let ci = f.cast("ptrtoint", "ptr", &c, "i64");
    let n = f.sub(&ci, &si);
    let notq = f.icmp("ne", "i8", &b, "34");
    let long = f.icmp("sgt", "i64", &n, "64");
    let slow = f.or1(&notq, &long);
    f.if_then(&slow, |f| {
        let s = f.call(V, "jp_string", &[("ptr", "%pc")]);
        f.ret(V, &s);
    });
    let c1 = f.off(&c, "1");
    f.store("ptr", &c1, "%pc");
    let h = f.call("i64", "jv_hash_bytes", &[("ptr", &start), ("i64", &n)]);
    let idx = f.bin("and", "i64", &h, "1023");
    let slot = f.idx("@g_keycache", &idx, 16);
    let e = f.load(V, &slot);
    let isstr = f.tag_is(&e, T_STR);
    f.if_then(&isstr, |f| {
        let p = f.ptr_of(&e);
        let l = f.ld_at("i64", &p, 8);
        let same = f.eq(&l, &n);
        f.if_then(&same, |f| {
            let d = f.off(&p, "32");
            let r = f.call("i32", "memcmp", &[("ptr", &d), ("ptr", &start), ("i64", &n)]);
            let z = f.icmp("eq", "i32", &r, "0");
            f.if_then(&z, |f| {
                f.call("void", "jv_retain", &[(V, &e)]);
                f.ret(V, &e);
            });
        });
    });
    let s = f.call(V, "jv_str", &[("ptr", &start), ("i64", &n)]);
    let sp = f.ptr_of(&s);
    f.st_at("i64", &h, &sp, 24);
    f.call("void", "jv_release", &[(V, &e)]);
    f.call("void", "jv_retain", &[(V, &s)]);
    f.store(V, &s, &slot);
    f.ret(V, &s);
    m.add(f);
}

fn literal(m: &mut Module) {
    let e_lit = m.cstr("Invalid literal");
    let e_num = m.cstr("Invalid numeric literal");
    let e_sq = m.cstr("Invalid string literal; expected \", but got '");
    let w_true = m.cstr("true");
    let w_false = m.cstr("false");
    let w_null = m.cstr("null");
    let w_nan = m.cstr("nan");

    // bare word / number at *pc
    let mut f = Func::new("internal", V, "jp_literal", &[("ptr", "pc")], "");
    let s = f.load("ptr", "%pc");
    let ev = f.var_init("ptr", &s);
    f.while_(
        |f| {
            let c = f.load("ptr", &ev);
            let b = f.load("i8", &c);
            let d = f.call("i1", "jp_isdelim", &[("i8", &b)]);
            f.not1(&d)
        },
        |f, _, _| {
            let c = f.load("ptr", &ev);
            let c1 = f.off(&c, "1");
            f.store("ptr", &c1, &ev);
        },
    );
    let e = f.load("ptr", &ev);
    let si = f.cast("ptrtoint", "ptr", &s, "i64");
    let ei = f.cast("ptrtoint", "ptr", &e, "i64");
    let n = f.sub(&ei, &si);
    // error position: just past the delimiter, or at EOF
    let eb = f.load("i8", &e);
    let atend = f.icmp("eq", "i8", &eb, "0");
    let e1 = f.off(&e, "1");
    let errpos = f.select(&atend, "ptr", &e, &e1);
    let fail = |f: &mut Func, msg: &str| {
        let r = f.if_val(&atend, V, |f| f.call(V, "jp_err_eof", &[("ptr", msg), ("ptr", &errpos)]), |f| f.call(V, "jp_err", &[("ptr", msg), ("ptr", &errpos)]));
        f.ret(V, &r);
    };
    let b0 = f.load("i8", &s);
    let word = |f: &mut Func, w: &str, len: i64, val: &str| {
        let ok = f.eq(&n, &len.to_string());
        f.if_then(&ok, |f| {
            let r = f.call("i32", "memcmp", &[("ptr", &s), ("ptr", w), ("i64", &len.to_string())]);
            let z = f.icmp("eq", "i32", &r, "0");
            f.if_then(&z, |f| {
                f.store("ptr", &e, "%pc");
                f.ret(V, val);
            });
        });
        fail(f, &e_lit);
    };
    let ist = f.icmp("eq", "i8", &b0, "116");
    f.if_then(&ist, |f| word(f, &w_true, 4, TRUE));
    let isf = f.icmp("eq", "i8", &b0, "102");
    f.if_then(&isf, |f| word(f, &w_false, 5, FALSE));
    let isn = f.icmp("eq", "i8", &b0, "110");
    f.if_then(&isn, |f| {
        let three = f.eq(&n, "3");
        f.if_then(&three, |f| {
            let nan = f.mknum(&fconst(f64::NAN));
            word(f, &w_nan, 3, &nan)
        });
        word(f, &w_null, 4, NULL);
    });
    let isq = f.icmp("eq", "i8", &b0, "39");
    f.if_then(&isq, |f| fail(f, &e_sq));
    // numbers: fast path for short plain integers
    let neg = f.icmp("eq", "i8", &b0, "45");
    let negi = f.cast("zext", "i1", &neg, "i64");
    let acc = f.var_init("i64", "0");
    let plain = f.var_init("i1", "true");
    f.for_range(&negi, &n, |f, i, brk, _| {
        let p = f.off(&s, i);
        let b = f.load("i8", &p);
        let d = f.bin("sub", "i8", &b, "48");
        let isd = f.icmp("ult", "i8", &d, "10");
        let notd = f.not1(&isd);
        f.if_then(&notd, |f| {
            f.store("i1", "false", &plain);
            f.br(brk);
        });
        let d64 = f.cast("zext", "i8", &d, "i64");
        let a = f.load("i64", &acc);
        let a10 = f.bin("mul", "i64", &a, "10");
        let a2 = f.add(&a10, &d64);
        f.store("i64", &a2, &acc);
    });
    let pl = f.load("i1", &plain);
    let ndig = f.sub(&n, &negi);
    let some = f.icmp("sgt", "i64", &ndig, "0");
    let short = f.icmp("sle", "i64", &ndig, "15");
    let fast = f.and1(&pl, &some);
    let fast = f.and1(&fast, &short);
    f.if_then(&fast, |f| {
        f.store("ptr", &e, "%pc");
        let a = f.load("i64", &acc);
        let d = f.cast("sitofp", "i64", &a, "double");
        let nd = f.ins(format!("fneg double {}", d));
        let r = f.select(&neg, "double", &nd, &d);
        let v = f.mknum(&r);
        f.ret(V, &v);
    });
    // general numbers via strtod; only number characters (or nan/infinity words)
    let okchars = f.var_init("i1", "true");
    f.for_range("0", &n, |f, i, _, _| {
        let p = f.off(&s, i);
        let b = f.load("i8", &p);
        let d = f.bin("sub", "i8", &b, "48");
        let isd = f.icmp("ult", "i8", &d, "10");
        let lc = f.bin("or", "i8", &b, "32");
        let mut ok = isd;
        for ch in [b'.', b'+', b'-'] {
            let e = f.icmp("eq", "i8", &b, &(ch as i64).to_string());
            ok = f.or1(&ok, &e);
        }
        // letters allowed only for exponent / nan / infinity
        for ch in [b'e', b'n', b'a', b'i', b'f', b't', b'y'] {
            let e = f.icmp("eq", "i8", &lc, &(ch as i64).to_string());
            ok = f.or1(&ok, &e);
        }
        let bad = f.not1(&ok);
        f.if_then(&bad, |f| f.store("i1", "false", &okchars));
    });
    let okc = f.load("i1", &okchars);
    let badc = f.not1(&okc);
    f.if_then(&badc, |f| fail(f, &e_num));
    // NUL-terminate a copy of the token for strtod
    let n1 = f.add(&n, "1");
    let tmp = f.call("ptr", "jv_malloc", &[("i64", &n1)]);
    f.call("ptr", "memcpy", &[("ptr", &tmp), ("ptr", &s), ("i64", &n)]);
    let tend = f.off(&tmp, &n);
    f.store("i8", "0", &tend);
    let endp = f.var("ptr");
    let d = f.call("double", "strtod", &[("ptr", &tmp), ("ptr", &endp)]);
    let ep = f.load("ptr", &endp);
    let full = f.icmp("eq", "ptr", &ep, &tend);
    let nz = f.icmp("sgt", "i64", &n, "0");
    let ok = f.and1(&full, &nz);
    f.call("void", "free", &[("ptr", &tmp)]);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| fail(f, &e_num));
    f.store("ptr", &e, "%pc");
    let v = f.call(V, "jv_numlit", &[("ptr", &s), ("ptr", &e), ("double", &d)]);
    f.ret(V, &v);
    m.add(f);
}

fn value(m: &mut Module) {
    let e_unf = m.cstr("Unfinished JSON term");
    let e_depth = m.cstr("Exceeds depth limit for parsing");
    let e_ubr = m.cstr("Unmatched ']'");
    let e_ucb = m.cstr("Unmatched '}'");
    let e_valcomma = m.cstr("Expected value before ','");
    let e_colon = m.cstr("':' not as part of an object");
    let e_sep = m.cstr("Expected separator between values");
    let e_arrel = m.cstr("Expected another array element");
    let e_kv = m.cstr("Expected another key-value pair");
    let e_keys = m.cstr("Object keys must be strings");
    let e_pairs = m.cstr("Objects must consist of key:value pairs");

    let mut f = Func::new("internal", V, "jp_value", &[("ptr", "pc"), ("i64", "depth")], "");
    let deep = f.icmp("sgt", "i64", "%depth", "10000");
    f.if_then(&deep, |f| {
        let c = f.load("ptr", "%pc");
        let e = f.call(V, "jp_err", &[("ptr", &e_depth), ("ptr", &c)]);
        f.ret(V, &e);
    });
    let d1 = f.add("%depth", "1");
    f.call("void", "jp_ws", &[("ptr", "%pc")]);
    let c = f.load("ptr", "%pc");
    let b = f.load("i8", &c);
    let bi = f.cast("zext", "i8", &b, "i64");
    let c1 = f.off(&c, "1");
    let (lobj, larr, lstr, leof, lub, luc, lcomma, lcolon, llit) = (
        f.label("obj"),
        f.label("arr"),
        f.label("str"),
        f.label("eof"),
        f.label("ubr"),
        f.label("ucb"),
        f.label("comma"),
        f.label("colon"),
        f.label("lit"),
    );
    f.switch(
        &bi,
        &llit,
        &[
            (123, lobj.clone()),
            (91, larr.clone()),
            (34, lstr.clone()),
            (0, leof.clone()),
            (93, lub.clone()),
            (125, luc.clone()),
            (44, lcomma.clone()),
            (58, lcolon.clone()),
        ],
    );
    f.block(&leof);
    let e = f.call(V, "jp_err_eof", &[("ptr", &e_unf), ("ptr", &c)]);
    f.ret(V, &e);
    for (l, msg, pos) in [(&lub, &e_ubr, &c1), (&luc, &e_ucb, &c1), (&lcomma, &e_valcomma, &c1), (&lcolon, &e_colon, &c1)] {
        f.block(l);
        let e = f.call(V, "jp_err", &[("ptr", msg), ("ptr", pos)]);
        f.ret(V, &e);
    }
    f.block(&lstr);
    let s = f.call(V, "jp_string", &[("ptr", "%pc")]);
    f.ret(V, &s);
    f.block(&llit);
    let s = f.call(V, "jp_literal", &[("ptr", "%pc")]);
    f.ret(V, &s);

    // after a value inside a container: report an unexpected token
    let unexpected = |f: &mut Func, at: &str, close: u8| {
        let b = f.load("i8", at);
        let at1 = f.off(at, "1");
        let iseof = f.icmp("eq", "i8", &b, "0");
        f.if_then(&iseof, |f| {
            let e = f.call(V, "jp_err_eof", &[("ptr", &e_unf), ("ptr", at)]);
            f.ret(V, &e);
        });
        let iscolon = f.icmp("eq", "i8", &b, "58");
        f.if_then(&iscolon, |f| {
            let e = f.call(V, "jp_err", &[("ptr", &e_colon), ("ptr", &at1)]);
            f.ret(V, &e);
        });
        let wrong = if close == b']' { &e_ucb } else { &e_ubr };
        let wc = if close == b']' { "125" } else { "93" };
        let iswrong = f.icmp("eq", "i8", &b, wc);
        f.if_then(&iswrong, |f| {
            let e = f.call(V, "jp_err", &[("ptr", wrong), ("ptr", &at1)]);
            f.ret(V, &e);
        });
        let te = f.call("ptr", "jp_token_end", &[("ptr", at)]);
        // a bare word cut off by EOF: same message, reported "at EOF"
        let b0 = f.load("i8", at);
        let isq0 = f.icmp("eq", "i8", &b0, "34");
        let isd0 = f.call("i1", "jp_isdelim", &[("i8", &b0)]);
        let lit = f.not1(&isd0);
        let tb = f.load("i8", &te);
        let tz = f.icmp("eq", "i8", &tb, "0");
        let tm1 = f.off(&te, "-1");
        let pb = f.load("i8", &tm1);
        let pdel = f.call("i1", "jp_isdelim", &[("i8", &pb)]);
        let nodelim = f.not1(&pdel);
        let lit_eof = f.and1(&lit, &tz);
        let lit_eof = f.and1(&lit_eof, &nodelim);
        let _ = isq0;
        f.if_then(&lit_eof, |f| {
            let e = f.call(V, "jp_err_eof", &[("ptr", &e_sep), ("ptr", &te)]);
            f.ret(V, &e);
        });
        let e = f.call(V, "jp_err", &[("ptr", &e_sep), ("ptr", &te)]);
        f.ret(V, &e);
    };

    // array
    f.block(&larr);
    f.store("ptr", &c1, "%pc");
    let av = f.var(V);
    let a0 = f.call(V, "jv_arr", &[("i64", "4")]);
    f.store(V, &a0, &av);
    f.call("void", "jp_ws", &[("ptr", "%pc")]);
    let c = f.load("ptr", "%pc");
    let b = f.load("i8", &c);
    let close = f.icmp("eq", "i8", &b, "93");
    f.if_then(&close, |f| {
        let c1 = f.off(&c, "1");
        f.store("ptr", &c1, "%pc");
        f.ret(V, &a0);
    });
    let first = f.var_init("i1", "true");
    f.while_(
        |_| "true".into(),
        |f, _, _| {
            let release = |f: &mut Func| {
                let a = f.load(V, &av);
                f.call("void", "jv_release", &[(V, &a)]);
            };
            f.call("void", "jp_ws", &[("ptr", "%pc")]);
            let c = f.load("ptr", "%pc");
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let isclose = f.icmp("eq", "i8", &b, "93");
            let fst = f.load("i1", &first);
            let nf = f.not1(&fst);
            let trailing = f.and1(&isclose, &nf);
            f.if_then(&trailing, |f| {
                release(f);
                let e = f.call(V, "jp_err", &[("ptr", &e_arrel), ("ptr", &c1)]);
                f.ret(V, &e);
            });
            f.store("i1", "false", &first);
            let v = f.call(V, "jp_value", &[("ptr", "%pc"), ("i64", &d1)]);
            let iserr = f.tag_is(&v, T_ERR);
            f.if_then(&iserr, |f| {
                release(f);
                f.ret(V, &v);
            });
            let a = f.load(V, &av);
            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &v)]);
            f.call("void", "jv_release", &[(V, &v)]);
            f.store(V, &a2, &av);
            f.call("void", "jp_ws", &[("ptr", "%pc")]);
            let c = f.load("ptr", "%pc");
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let close = f.icmp("eq", "i8", &b, "93");
            f.if_then(&close, |f| {
                f.store("ptr", &c1, "%pc");
                f.ret(V, &a2);
            });
            let comma = f.icmp("eq", "i8", &b, "44");
            let notc = f.not1(&comma);
            f.if_then(&notc, |f| {
                release(f);
                unexpected(f, &c, b']');
            });
            f.store("ptr", &c1, "%pc");
        },
    );
    f.unreachable();

    // object
    f.block(&lobj);
    f.store("ptr", &c1, "%pc");
    let ov = f.var(V);
    let o0 = f.call(V, "jv_obj", &[]);
    f.store(V, &o0, &ov);
    f.call("void", "jp_ws", &[("ptr", "%pc")]);
    let c = f.load("ptr", "%pc");
    let b = f.load("i8", &c);
    let close = f.icmp("eq", "i8", &b, "125");
    f.if_then(&close, |f| {
        let c1 = f.off(&c, "1");
        f.store("ptr", &c1, "%pc");
        f.ret(V, &o0);
    });
    f.while_(
        |_| "true".into(),
        |f, _, _| {
            let release = |f: &mut Func| {
                let o = f.load(V, &ov);
                f.call("void", "jv_release", &[(V, &o)]);
            };
            let fail = |f: &mut Func, msg: &str, pos: &str| {
                release(f);
                let e = f.call(V, "jp_err", &[("ptr", msg), ("ptr", pos)]);
                f.ret(V, &e);
            };
            f.call("void", "jp_ws", &[("ptr", "%pc")]);
            let c = f.load("ptr", "%pc");
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let isclose = f.icmp("eq", "i8", &b, "125");
            f.if_then(&isclose, |f| fail(f, &e_kv, &c1));
            let iscomma = f.icmp("eq", "i8", &b, "44");
            f.if_then(&iscomma, |f| fail(f, &e_valcomma, &c1));
            let iseof = f.icmp("eq", "i8", &b, "0");
            f.if_then(&iseof, |f| {
                release(f);
                let e = f.call(V, "jp_err_eof", &[("ptr", &e_unf), ("ptr", &c)]);
                f.ret(V, &e);
            });
            let isq = f.icmp("eq", "i8", &b, "34");
            let notq = f.not1(&isq);
            f.if_then(&notq, |f| {
                // a non-string key: parse it, then complain at the following ':'
                let v = f.call(V, "jp_value", &[("ptr", "%pc"), ("i64", &d1)]);
                let iserr = f.tag_is(&v, T_ERR);
                f.if_then(&iserr, |f| {
                    release(f);
                    f.ret(V, &v);
                });
                f.call("void", "jv_release", &[(V, &v)]);
                f.call("void", "jp_ws", &[("ptr", "%pc")]);
                let c = f.load("ptr", "%pc");
                let b = f.load("i8", &c);
                let c1 = f.off(&c, "1");
                let iscolon = f.icmp("eq", "i8", &b, "58");
                f.if_then(&iscolon, |f| fail(f, &e_keys, &c1));
                release(f);
                unexpected(f, &c, b'}');
            });
            let k = f.call(V, "jp_key", &[("ptr", "%pc")]);
            let kerr = f.tag_is(&k, T_ERR);
            f.if_then(&kerr, |f| {
                release(f);
                f.ret(V, &k);
            });
            f.call("void", "jp_ws", &[("ptr", "%pc")]);
            let c = f.load("ptr", "%pc");
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let col = f.icmp("eq", "i8", &b, "58");
            let notcol = f.not1(&col);
            f.if_then(&notcol, |f| {
                f.call("void", "jv_release", &[(V, &k)]);
                let isclose = f.icmp("eq", "i8", &b, "125");
                let iscomma = f.icmp("eq", "i8", &b, "44");
                let pairs = f.or1(&isclose, &iscomma);
                f.if_then(&pairs, |f| fail(f, &e_pairs, &c1));
                release(f);
                unexpected(f, &c, b'}');
            });
            f.store("ptr", &c1, "%pc");
            f.call("void", "jp_ws", &[("ptr", "%pc")]);
            let c = f.load("ptr", "%pc");
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let isclose = f.icmp("eq", "i8", &b, "125");
            let iscomma = f.icmp("eq", "i8", &b, "44");
            let novalue = f.or1(&isclose, &iscomma);
            f.if_then(&novalue, |f| {
                f.call("void", "jv_release", &[(V, &k)]);
                let msg = f.select(&iscomma, "ptr", &e_valcomma, &e_ucb);
                fail(f, &msg, &c1);
            });
            let v = f.call(V, "jp_value", &[("ptr", "%pc"), ("i64", &d1)]);
            let verr = f.tag_is(&v, T_ERR);
            f.if_then(&verr, |f| {
                f.call("void", "jv_release", &[(V, &k)]);
                release(f);
                f.ret(V, &v);
            });
            let o = f.load(V, &ov);
            let o2 = f.call(V, "jv_obj_set", &[(V, &o), (V, &k), (V, &v)]);
            f.call("void", "jv_release", &[(V, &k)]);
            f.call("void", "jv_release", &[(V, &v)]);
            f.store(V, &o2, &ov);
            f.call("void", "jp_ws", &[("ptr", "%pc")]);
            let c = f.load("ptr", "%pc");
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            let close = f.icmp("eq", "i8", &b, "125");
            f.if_then(&close, |f| {
                f.store("ptr", &c1, "%pc");
                f.ret(V, &o2);
            });
            let comma = f.icmp("eq", "i8", &b, "44");
            let notc = f.not1(&comma);
            f.if_then(&notc, |f| {
                release(f);
                unexpected(f, &c, b'}');
            });
            f.store("ptr", &c1, "%pc");
        },
    );
    f.unreachable();
    m.add(f);
}

fn messages(m: &mut Module) {
    let at_eof = m.cstr(" at EOF");
    let at_line = m.cstr(" at line ");
    let col = m.cstr(", column ");
    // "<msg>[ at EOF] at line L, column C", position from @g_perr_pos relative to base
    let mut f = Func::new("internal", V, "jv_parse_errmsg", &[(V, "e"), ("ptr", "base")], "");
    let r = f.call(V, "jv_parse_errmsg2", &[(V, "%e"), ("ptr", "%base"), ("i64", "0"), ("i64", "0")]);
    f.ret(V, &r);
    m.add(f);

    // same, with the line/column already consumed before `base`
    let mut f = Func::new("internal", V, "jv_parse_errmsg2", &[(V, "e"), ("ptr", "base"), ("i64", "line0"), ("i64", "col0")], "");
    let msg = f.call(V, "jv_err_msg", &[(V, "%e")]);
    let pos = f.load("ptr", "@g_perr_pos");
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "96")]);
    f.call("void", "sb_putv", &[("ptr", &sb), (V, &msg)]);
    let eof = f.load("i1", "@g_perr_eof");
    f.if_then(&eof, |f| f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &at_eof)]));
    let bi = f.cast("ptrtoint", "ptr", "%base", "i64");
    let pi = f.cast("ptrtoint", "ptr", &pos, "i64");
    let n = f.sub(&pi, &bi);
    let l1 = f.add("%line0", "1");
    let line = f.var_init("i64", &l1);
    let colv = f.var_init("i64", "%col0");
    f.for_range("0", &n, |f, i, _, _| {
        let p = f.off("%base", i);
        let b = f.load("i8", &p);
        let nl = f.icmp("eq", "i8", &b, "10");
        f.if_else(
            &nl,
            |f| {
                let l = f.load("i64", &line);
                let l1 = f.add(&l, "1");
                f.store("i64", &l1, &line);
                f.store("i64", "0", &colv);
            },
            |f| {
                let c = f.load("i64", &colv);
                let c1 = f.add(&c, "1");
                f.store("i64", &c1, &colv);
            },
        );
    });
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &at_line)]);
    let l = f.load("i64", &line);
    f.call("void", "sb_int", &[("ptr", &sb), ("i64", &l)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &col)]);
    let c = f.load("i64", &colv);
    f.call("void", "sb_int", &[("ptr", &sb), ("i64", &c)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    f.ret(V, &s);
    m.add(f);

    // fromjson(string) -> value or error (error message includes the location)
    let e_extra = m.cstr("Unexpected extra JSON values");
    let mut f = Func::new("internal", V, "jv_fromjson", &[(V, "s")], "");
    let pc = f.var("ptr");
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%s")]);
    f.store("ptr", &d, &pc);
    let v = f.call(V, "jp_value", &[("ptr", &pc), ("i64", "0")]);
    let iserr = f.tag_is(&v, T_ERR);
    f.if_then(&iserr, |f| {
        let msg = f.call(V, "jv_parse_errmsg", &[(V, &v), ("ptr", &d)]);
        f.call("void", "jv_release", &[(V, &v)]);
        let e = f.call(V, "jv_err_take", &[(V, &msg)]);
        f.ret(V, &e);
    });
    f.call("void", "jp_ws", &[("ptr", &pc)]);
    let c = f.load("ptr", &pc);
    let b = f.load("i8", &c);
    let end = f.icmp("eq", "i8", &b, "0");
    f.if_then(&end, |f| f.ret(V, &v));
    f.call("void", "jv_release", &[(V, &v)]);
    let e = f.call(V, "jv_err_c", &[("ptr", &e_extra)]);
    f.ret(V, &e);
    m.add(f);
}

/// Top-level value: like jp_value, but a bare word glued to a structural
/// character is an error before the value is produced (as in jq).
pub fn top(m: &mut Module) {
    let e_key = m.cstr("Expected string key before ':'");
    let e_valcomma = m.cstr("Expected value before ','");
    let e_ubr = m.cstr("Unmatched ']'");
    let e_ucb = m.cstr("Unmatched '}'");
    let mut f = Func::new("internal", V, "jp_top", &[("ptr", "pc")], "");
    f.call("void", "jp_ws", &[("ptr", "%pc")]);
    let c = f.load("ptr", "%pc");
    let b = f.load("i8", &c);
    let c1 = f.off(&c, "1");
    let iscolon = f.icmp("eq", "i8", &b, "58");
    f.if_then(&iscolon, |f| {
        let e = f.call(V, "jp_err", &[("ptr", &e_key), ("ptr", &c1)]);
        f.ret(V, &e);
    });
    let v = f.call(V, "jp_value", &[("ptr", "%pc"), ("i64", "0")]);
    let iserr = f.tag_is(&v, T_ERR);
    f.if_then(&iserr, |f| f.ret(V, &v));
    let isd = f.call("i1", "jp_isdelim", &[("i8", &b)]);
    let lit = f.not1(&isd);
    f.if_then(&lit, |f| {
        let e = f.load("ptr", "%pc");
        let nb = f.load("i8", &e);
        let e1 = f.off(&e, "1");
        for (ch, msg) in [(b',', &e_valcomma), (b':', &e_key), (b']', &e_ubr), (b'}', &e_ucb)] {
            let is = f.icmp("eq", "i8", &nb, &(ch as i64).to_string());
            f.if_then(&is, |f| {
                f.call("void", "jv_release", &[(V, &v)]);
                let er = f.call(V, "jp_err", &[("ptr", msg), ("ptr", &e1)]);
                f.ret(V, &er);
            });
        }
    });
    f.ret(V, &v);
    m.add(f);
}
