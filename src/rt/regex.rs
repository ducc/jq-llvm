//! Regular expressions: a backtracking VM over UTF-8 input.
//!
//! Programs are produced by the regex compiler written in jq (src/regex.jq)
//! and arrive as {code: [i64...], ngroups, names}. Instruction set:
//!   0 MATCH            1 CHAR c          2 ANY (not \n)       4 CLASS neg n (lo hi)*n
//!   5 SPLIT x y        6 JMP x           7 SAVE k             10 WORDB   11 NWORDB
//!   12 BEGIN (\A)      13 END (\z)       14 ENDZ (\Z)         15 BACKREF k
//!   16 CHARI c         17 CLASSI ...     19 LOOK neg len      20 MARK L  21 PROGRESS L
//! MARK/PROGRESS guard unbounded loops against empty iterations.

use super::ops::ret_err1;
use super::*;
use crate::ir::{Func, Module, V};

pub fn emit(m: &mut Module) {
    m.global("@g_recache = internal global [256 x [3 x %V]] zeroinitializer".into());
    helpers(m);
    exec(m);
    cache(m);
    run(m);
}

fn helpers(m: &mut Module) {
    // simple case folding: ASCII and Latin-1
    let mut f = Func::new("internal", "i64", "re_lower", &[("i64", "c")], "");
    let a = f.icmp("sge", "i64", "%c", "65");
    let b = f.icmp("sle", "i64", "%c", "90");
    let up = f.and1(&a, &b);
    let a2 = f.icmp("sge", "i64", "%c", "192");
    let b2 = f.icmp("sle", "i64", "%c", "222");
    let n2 = f.ne("%c", "215");
    let l1 = f.and1(&a2, &b2);
    let l1 = f.and1(&l1, &n2);
    let any = f.or1(&up, &l1);
    let c32 = f.add("%c", "32");
    let r = f.select(&any, "i64", &c32, "%c");
    f.ret("i64", &r);
    m.add(f);

    let mut f = Func::new("internal", "i64", "re_upper", &[("i64", "c")], "");
    let a = f.icmp("sge", "i64", "%c", "97");
    let b = f.icmp("sle", "i64", "%c", "122");
    let lo = f.and1(&a, &b);
    let a2 = f.icmp("sge", "i64", "%c", "224");
    let b2 = f.icmp("sle", "i64", "%c", "254");
    let n2 = f.ne("%c", "247");
    let l1 = f.and1(&a2, &b2);
    let l1 = f.and1(&l1, &n2);
    let any = f.or1(&lo, &l1);
    let c32 = f.sub("%c", "32");
    let r = f.select(&any, "i64", &c32, "%c");
    f.ret("i64", &r);
    m.add(f);

    let mut f = Func::new("internal", "i1", "re_isword", &[("i8", "b")], "");
    let c = f.cast("zext", "i8", "%b", "i64");
    let d1 = f.icmp("sge", "i64", &c, "48");
    let d2 = f.icmp("sle", "i64", &c, "57");
    let dig = f.and1(&d1, &d2);
    let lc = f.bin("or", "i64", &c, "32");
    let a1 = f.icmp("sge", "i64", &lc, "97");
    let a2 = f.icmp("sle", "i64", &lc, "122");
    let alpha = f.and1(&a1, &a2);
    let us = f.eq(&c, "95");
    let hi = f.icmp("sge", "i64", &c, "128");
    let x = f.or1(&dig, &alpha);
    let y = f.or1(&us, &hi);
    let r = f.or1(&x, &y);
    f.ret("i1", &r);
    m.add(f);

    // in-class test: code[at] = n, ranges follow
    let mut f = Func::new("internal", "i1", "re_inclass", &[("ptr", "code"), ("i64", "at"), ("i64", "c")], "");
    let np = f.idx("%code", "%at", 8);
    let n = f.load("i64", &np);
    f.for_range("0", &n, |f, i, _, _| {
        let i2 = f.bin("mul", "i64", i, "2");
        let o = f.add("%at", "1");
        let o = f.add(&o, &i2);
        let lp = f.idx("%code", &o, 8);
        let lo = f.load("i64", &lp);
        let o1 = f.add(&o, "1");
        let hp = f.idx("%code", &o1, 8);
        let hi = f.load("i64", &hp);
        let a = f.icmp("sge", "i64", "%c", &lo);
        let b = f.icmp("sle", "i64", "%c", &hi);
        let inr = f.and1(&a, &b);
        f.if_then(&inr, |f| f.ret("i1", "true"));
    });
    f.ret("i1", "false");
    m.add(f);

    // backtrack stack: {ptr buf, i64 len, i64 cap}, entries of 3 x i64
    let mut f = Func::new("internal", "void", "re_push", &[("ptr", "st"), ("i64", "k"), ("i64", "a"), ("i64", "b")], "alwaysinline");
    let len = f.ld_at("i64", "%st", 8);
    let cap = f.ld_at("i64", "%st", 16);
    let full = f.icmp("sge", "i64", &len, &cap);
    f.if_then(&full, |f| {
        let c2 = f.bin("mul", "i64", &cap, "2");
        let small = f.icmp("slt", "i64", &c2, "64");
        let nc = f.select(&small, "i64", "64", &c2);
        let buf = f.ld_at("ptr", "%st", 0);
        let bytes = f.bin("mul", "i64", &nc, "24");
        let nb = f.call("ptr", "jv_realloc", &[("ptr", &buf), ("i64", &bytes)]);
        f.st_at("ptr", &nb, "%st", 0);
        f.st_at("i64", &nc, "%st", 16);
    });
    let buf = f.ld_at("ptr", "%st", 0);
    let e = f.idx(&buf, &len, 24);
    f.st_at("i64", "%k", &e, 0);
    f.st_at("i64", "%a", &e, 8);
    f.st_at("i64", "%b", &e, 16);
    let l1 = f.add(&len, "1");
    f.st_at("i64", &l1, "%st", 8);
    f.ret_void();
    m.add(f);
}

fn exec(m: &mut Module) {
    // i64 re_exec(code, pc0, d, n, s, slots, nslots, st) -> end offset or -1
    let mut f = Func::new(
        "internal",
        "i64",
        "re_exec",
        &[("ptr", "code"), ("i64", "pc0"), ("ptr", "d"), ("i64", "n"), ("i64", "s"), ("ptr", "slots"), ("i64", "nslots"), ("ptr", "st")],
        "",
    );
    let pcv = f.var_init("i64", "%pc0");
    let spv = f.var_init("i64", "%s");
    let lenv = f.var("i64");
    let base = f.ld_at("i64", "%st", 8);
    let lfail = f.label("fail");
    let lloop = f.label("loop");
    f.br(&lloop);
    f.block(&lloop);
    let pc = f.load("i64", &pcv);
    let sp = f.load("i64", &spv);
    let opp = f.idx("%code", &pc, 8);
    let op = f.load("i64", &opp);
    let a1 = f.add(&pc, "1");
    let ap = f.idx("%code", &a1, 8);
    let ops = [0, 1, 2, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21];
    let labels: Vec<String> = ops.iter().map(|o| f.label(&format!("op{}", o))).collect();
    let cases: Vec<(i64, String)> = ops.iter().zip(labels.iter()).map(|(o, l)| (*o as i64, l.clone())).collect();
    f.switch(&op, &lfail, &cases);
    let lab = |o: i64| labels[ops.iter().position(|x| *x == o).unwrap()].clone();
    let goto = |f: &mut Func, pcn: &str, spn: &str, lloop: &str| {
        f.store("i64", pcn, &pcv);
        f.store("i64", spn, &spv);
        f.br(lloop);
    };

    // MATCH
    f.block(&lab(0));
    f.st_at("i64", &base, "%st", 8);
    f.ret("i64", &sp);

    // CHAR / CHARI
    for (o, ic) in [(1, false), (16, true)] {
        f.block(&lab(o));
        let inb = f.lt(&sp, "%n");
        let lok = f.label("char_in");
        f.cbr(&inb, &lok, &lfail);
        f.block(&lok);
        let want = f.load("i64", &ap);
        let cp = f.call("i64", "utf8_decode", &[("ptr", "%d"), ("i64", "%n"), ("i64", &sp), ("ptr", &lenv)]);
        let cp = if ic { f.call("i64", "re_lower", &[("i64", &cp)]) } else { cp };
        let eq = f.eq(&cp, &want);
        let lm = f.label("char_m");
        f.cbr(&eq, &lm, &lfail);
        f.block(&lm);
        let l = f.load("i64", &lenv);
        let nsp = f.add(&sp, &l);
        let npc = f.add(&pc, "2");
        goto(&mut f, &npc, &nsp, &lloop);
    }

    // ANY
    f.block(&lab(2));
    let inb = f.lt(&sp, "%n");
    let lok = f.label("any_in");
    f.cbr(&inb, &lok, &lfail);
    f.block(&lok);
    let bp = f.off("%d", &sp);
    let b = f.load("i8", &bp);
    let nl = f.icmp("eq", "i8", &b, "10");
    let lm = f.label("any_m");
    f.cbr(&nl, &lfail, &lm);
    f.block(&lm);
    f.call("i64", "utf8_decode", &[("ptr", "%d"), ("i64", "%n"), ("i64", &sp), ("ptr", &lenv)]);
    let l = f.load("i64", &lenv);
    let nsp = f.add(&sp, &l);
    let npc = f.add(&pc, "1");
    goto(&mut f, &npc, &nsp, &lloop);

    // CLASS / CLASSI
    for (o, ic) in [(4, false), (17, true)] {
        f.block(&lab(o));
        let inb = f.lt(&sp, "%n");
        let lok = f.label("cls_in");
        f.cbr(&inb, &lok, &lfail);
        f.block(&lok);
        let neg = f.load("i64", &ap);
        let a2 = f.add(&pc, "2");
        let cp = f.call("i64", "utf8_decode", &[("ptr", "%d"), ("i64", "%n"), ("i64", &sp), ("ptr", &lenv)]);
        let hit = f.call("i1", "re_inclass", &[("ptr", "%code"), ("i64", &a2), ("i64", &cp)]);
        let hit = if ic {
            let lo = f.call("i64", "re_lower", &[("i64", &cp)]);
            let up = f.call("i64", "re_upper", &[("i64", &cp)]);
            let h2 = f.call("i1", "re_inclass", &[("ptr", "%code"), ("i64", &a2), ("i64", &lo)]);
            let h3 = f.call("i1", "re_inclass", &[("ptr", "%code"), ("i64", &a2), ("i64", &up)]);
            let x = f.or1(&hit, &h2);
            f.or1(&x, &h3)
        } else {
            hit
        };
        let isneg = f.ne(&neg, "0");
        let ok = f.bin("xor", "i1", &hit, &isneg);
        let lm = f.label("cls_m");
        f.cbr(&ok, &lm, &lfail);
        f.block(&lm);
        let np = f.idx("%code", &a2, 8);
        let nr = f.load("i64", &np);
        let nr2 = f.bin("mul", "i64", &nr, "2");
        let npc = f.add(&pc, "3");
        let npc = f.add(&npc, &nr2);
        let l = f.load("i64", &lenv);
        let nsp = f.add(&sp, &l);
        goto(&mut f, &npc, &nsp, &lloop);
    }

    // SPLIT x y
    f.block(&lab(5));
    let x = f.load("i64", &ap);
    let a2 = f.add(&pc, "2");
    let yp = f.idx("%code", &a2, 8);
    let y = f.load("i64", &yp);
    f.call("void", "re_push", &[("ptr", "%st"), ("i64", "0"), ("i64", &y), ("i64", &sp)]);
    goto(&mut f, &x, &sp, &lloop);

    // JMP
    f.block(&lab(6));
    let x = f.load("i64", &ap);
    goto(&mut f, &x, &sp, &lloop);

    // SAVE k / MARK L
    for o in [7, 20] {
        f.block(&lab(o));
        let k = f.load("i64", &ap);
        let k = if o == 20 { f.add(&k, "%nslots") } else { k };
        let sp_ = f.idx("%slots", &k, 8);
        let old = f.load("i64", &sp_);
        f.call("void", "re_push", &[("ptr", "%st"), ("i64", "1"), ("i64", &k), ("i64", &old)]);
        f.store("i64", &sp, &sp_);
        let npc = f.add(&pc, "2");
        goto(&mut f, &npc, &sp, &lloop);
    }

    // PROGRESS L
    f.block(&lab(21));
    let k = f.load("i64", &ap);
    let k = f.add(&k, "%nslots");
    let sp_ = f.idx("%slots", &k, 8);
    let mark = f.load("i64", &sp_);
    let same = f.eq(&mark, &sp);
    let lm = f.label("prog_ok");
    f.cbr(&same, &lfail, &lm);
    f.block(&lm);
    let npc = f.add(&pc, "2");
    goto(&mut f, &npc, &sp, &lloop);

    // WORDB / NWORDB
    for o in [10, 11] {
        f.block(&lab(o));
        let gt0 = f.icmp("sgt", "i64", &sp, "0");
        let a = f.if_val(
            &gt0,
            "i1",
            |f| {
                let p = f.sub(&sp, "1");
                let q = f.off("%d", &p);
                let b = f.load("i8", &q);
                f.call("i1", "re_isword", &[("i8", &b)])
            },
            |_| "false".into(),
        );
        let ltn = f.lt(&sp, "%n");
        let b = f.if_val(
            &ltn,
            "i1",
            |f| {
                let q = f.off("%d", &sp);
                let b = f.load("i8", &q);
                f.call("i1", "re_isword", &[("i8", &b)])
            },
            |_| "false".into(),
        );
        let wb = f.bin("xor", "i1", &a, &b);
        let ok = if o == 10 { wb } else { f.not1(&wb) };
        let lm = f.label("wb_ok");
        f.cbr(&ok, &lm, &lfail);
        f.block(&lm);
        let npc = f.add(&pc, "1");
        goto(&mut f, &npc, &sp, &lloop);
    }

    // BEGIN / END / ENDZ
    f.block(&lab(12));
    let ok = f.eq(&sp, "0");
    let lm = f.label("bol_ok");
    f.cbr(&ok, &lm, &lfail);
    f.block(&lm);
    let npc = f.add(&pc, "1");
    goto(&mut f, &npc, &sp, &lloop);

    f.block(&lab(13));
    let ok = f.eq(&sp, "%n");
    let lm = f.label("eos_ok");
    f.cbr(&ok, &lm, &lfail);
    f.block(&lm);
    let npc = f.add(&pc, "1");
    goto(&mut f, &npc, &sp, &lloop);

    f.block(&lab(14));
    let atend = f.eq(&sp, "%n");
    let n1 = f.sub("%n", "1");
    let atlast = f.eq(&sp, &n1);
    let lastnl = f.if_val(
        &atlast,
        "i1",
        |f| {
            let q = f.off("%d", &sp);
            let b = f.load("i8", &q);
            f.icmp("eq", "i8", &b, "10")
        },
        |_| "false".into(),
    );
    let ok = f.or1(&atend, &lastnl);
    let lm = f.label("eoz_ok");
    f.cbr(&ok, &lm, &lfail);
    f.block(&lm);
    let npc = f.add(&pc, "1");
    goto(&mut f, &npc, &sp, &lloop);

    // BACKREF k
    f.block(&lab(15));
    let k = f.load("i64", &ap);
    let k2 = f.bin("mul", "i64", &k, "2");
    let s0p = f.idx("%slots", &k2, 8);
    let s0 = f.load("i64", &s0p);
    let k21 = f.add(&k2, "1");
    let e0p = f.idx("%slots", &k21, 8);
    let e0 = f.load("i64", &e0p);
    let bad1 = f.lt(&s0, "0");
    let bad2 = f.lt(&e0, "0");
    let bad = f.or1(&bad1, &bad2);
    let lchk = f.label("br_chk");
    f.cbr(&bad, &lfail, &lchk);
    f.block(&lchk);
    let len = f.sub(&e0, &s0);
    let end = f.add(&sp, &len);
    let fits = f.icmp("sle", "i64", &end, "%n");
    let lcmp = f.label("br_cmp");
    f.cbr(&fits, &lcmp, &lfail);
    f.block(&lcmp);
    let p1 = f.off("%d", &sp);
    let p2 = f.off("%d", &s0);
    let c = f.call("i32", "memcmp", &[("ptr", &p1), ("ptr", &p2), ("i64", &len)]);
    let z = f.icmp("eq", "i32", &c, "0");
    let lm = f.label("br_ok");
    f.cbr(&z, &lm, &lfail);
    f.block(&lm);
    let npc = f.add(&pc, "2");
    goto(&mut f, &npc, &end, &lloop);

    // LOOK neg len
    f.block(&lab(19));
    let neg = f.load("i64", &ap);
    let a2 = f.add(&pc, "2");
    let lp = f.idx("%code", &a2, 8);
    let ln = f.load("i64", &lp);
    let sub = f.add(&pc, "3");
    let st2 = f.array_var("i64", 3);
    f.st_at("ptr", "null", &st2, 0);
    f.st_at("i64", "0", &st2, 8);
    f.st_at("i64", "0", &st2, 16);
    let r = f.call(
        "i64",
        "re_exec",
        &[("ptr", "%code"), ("i64", &sub), ("ptr", "%d"), ("i64", "%n"), ("i64", &sp), ("ptr", "%slots"), ("i64", "%nslots"), ("ptr", &st2)],
    );
    let b2 = f.ld_at("ptr", &st2, 0);
    f.call("void", "free", &[("ptr", &b2)]);
    let matched = f.icmp("sge", "i64", &r, "0");
    let isneg = f.ne(&neg, "0");
    let ok = f.bin("xor", "i1", &matched, &isneg);
    let lm = f.label("look_ok");
    f.cbr(&ok, &lm, &lfail);
    f.block(&lm);
    let npc = f.add(&sub, &ln);
    goto(&mut f, &npc, &sp, &lloop);

    // backtrack
    f.block(&lfail);
    let (lpop, lempty) = (f.label("pop"), f.label("empty"));
    f.br(&lpop);
    f.block(&lpop);
    let len = f.ld_at("i64", "%st", 8);
    let empty = f.icmp("sle", "i64", &len, &base);
    let lhave = f.label("have");
    f.cbr(&empty, &lempty, &lhave);
    f.block(&lhave);
    let l1 = f.sub(&len, "1");
    f.st_at("i64", &l1, "%st", 8);
    let buf = f.ld_at("ptr", "%st", 0);
    let e = f.idx(&buf, &l1, 24);
    let kind = f.ld_at("i64", &e, 0);
    let a = f.ld_at("i64", &e, 8);
    let b = f.ld_at("i64", &e, 16);
    let isrestore = f.eq(&kind, "1");
    let (lr, lb) = (f.label("restore"), f.label("branch"));
    f.cbr(&isrestore, &lr, &lb);
    f.block(&lr);
    let sp_ = f.idx("%slots", &a, 8);
    f.store("i64", &b, &sp_);
    f.br(&lpop);
    f.block(&lb);
    goto(&mut f, &a, &b, &lloop);
    f.block(&lempty);
    f.ret("i64", "-1");
    m.add(f);
}

fn cache(m: &mut Module) {
    // _re_cache_get(re; flags)
    let mut f = Func::new("internal", V, "jvb_re_cache_get", &[(V, "x"), (V, "re"), (V, "fl")], "");
    let ok = f.tag_is("%re", T_STR);
    let notok = f.not1(&ok);
    f.if_then(&notok, |f| f.ret(V, NULL));
    let slot = f.call("ptr", "re_cache_slot", &[(V, "%re"), (V, "%fl")]);
    let r = f.load(V, &slot);
    let fv = f.ld_at(V, &slot, 16);
    let p = f.ld_at(V, &slot, 32);
    let have = f.tag_is(&r, T_STR);
    f.if_then(&have, |f| {
        let e1 = f.call("i1", "jv_str_eq", &[(V, &r), (V, "%re")]);
        let e2 = f.call("i1", "jv_eq", &[(V, &fv), (V, "%fl")]);
        let both = f.and1(&e1, &e2);
        f.if_then(&both, |f| {
            f.call("void", "jv_retain", &[(V, &p)]);
            f.ret(V, &p);
        });
    });
    f.ret(V, NULL);
    m.add(f);

    let mut f = Func::new("internal", "ptr", "re_cache_slot", &[(V, "re"), (V, "fl")], "");
    let h = f.call("i64", "jv_str_hash", &[(V, "%re")]);
    let isstr = f.tag_is("%fl", T_STR);
    let fh = f.if_val(&isstr, "i64", |f| f.call("i64", "jv_str_hash", &[(V, "%fl")]), |_| "0".into());
    let x = f.bin("xor", "i64", &h, &fh);
    let i = f.bin("and", "i64", &x, "255");
    let p = f.idx("@g_recache", &i, 48);
    f.ret("ptr", &p);
    m.add(f);

    // _re_cache_put(re; flags; prog) -> prog
    let mut f = Func::new("internal", V, "jvb_re_cache_put", &[(V, "x"), (V, "re"), (V, "fl"), (V, "prog")], "");
    let slot = f.call("ptr", "re_cache_slot", &[(V, "%re"), (V, "%fl")]);
    for (off, v) in [(0, "%re"), (16, "%fl"), (32, "%prog")] {
        let old = f.ld_at(V, &slot, off);
        f.call("void", "jv_release", &[(V, &old)]);
        f.call("void", "jv_retain", &[(V, v)]);
        f.st_at(V, v, &slot, off);
    }
    f.call("void", "jv_retain", &[(V, "%prog")]);
    f.ret(V, "%prog");
    m.add(f);
}

fn run(m: &mut Module) {
    let k_code = m.cstr("code");
    let k_ng = m.cstr("ngroups");
    let k_names = m.cstr("names");
    let k_offset = m.cstr("offset");
    let k_length = m.cstr("length");
    let k_string = m.cstr("string");
    let k_captures = m.cstr("captures");
    let k_name = m.cstr("name");

    // object helper: set key (C string) to owned value
    let mut f = Func::new("internal", V, "re_set", &[(V, "o"), ("ptr", "k"), (V, "v")], "");
    let k = f.call(V, "jv_cstr", &[("ptr", "%k")]);
    let o = f.call(V, "jv_obj_set", &[(V, "%o"), (V, &k), (V, "%v")]);
    f.call("void", "jv_release", &[(V, &k)]);
    f.call("void", "jv_release", &[(V, "%v")]);
    f.ret(V, &o);
    m.add(f);

    let mut f = Func::new("internal", V, "re_num", &[("i64", "x")], "alwaysinline");
    let d = f.cast("sitofp", "i64", "%x", "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    // _re_run(prog; mode; test)
    let mut f = Func::new("internal", V, "jvb_re_run", &[(V, "x"), (V, "prog"), (V, "mode"), (V, "test")], "");
    let iss = f.tag_is("%x", T_STR);
    let nots = f.not1(&iss);
    f.if_then(&nots, |f| ret_err1(m, f, "%x", "cannot be matched, as it is not a string"));
    let d = f.call("ptr", "jv_str_ptr", &[(V, "%x")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%x")]);
    let kc = f.call(V, "jv_cstr", &[("ptr", &k_code)]);
    let code = f.call(V, "jv_obj_get", &[(V, "%prog"), (V, &kc)]);
    f.call("void", "jv_release", &[(V, &kc)]);
    let kg = f.call(V, "jv_cstr", &[("ptr", &k_ng)]);
    let ngv = f.call(V, "jv_obj_get", &[(V, "%prog"), (V, &kg)]);
    f.call("void", "jv_release", &[(V, &kg)]);
    let kn = f.call(V, "jv_cstr", &[("ptr", &k_names)]);
    let names = f.call(V, "jv_obj_get", &[(V, "%prog"), (V, &kn)]);
    f.call("void", "jv_release", &[(V, &kn)]);
    let ngd = f.num_of(&ngv);
    let ng = f.cast("fptosi", "double", &ngd, "i64");
    let plen = f.call("i64", "jv_arr_len", &[(V, &code)]);
    let pbytes = f.bin("mul", "i64", &plen, "8");
    let buf = f.call("ptr", "jv_malloc", &[("i64", &pbytes)]);
    f.for_range("0", &plen, |f, i, _, _| {
        let e = f.call(V, "jv_arr_get", &[(V, &code), ("i64", i)]);
        let dd = f.num_of(&e);
        let x = f.cast("fptosi", "double", &dd, "i64");
        let p = f.idx(&buf, i, 8);
        f.store("i64", &x, &p);
    });
    // flags
    let modestr = f.tag_is("%mode", T_STR);
    let has = |f: &mut Func, ch: i64| -> String {
        f.if_val(
            &modestr,
            "i1",
            |f| {
                let md = f.call("ptr", "jv_str_ptr", &[(V, "%mode")]);
                let ml = f.call("i64", "jv_str_len", &[(V, "%mode")]);
                let r = f.call("ptr", "memchr", &[("ptr", &md), ("i32", &ch.to_string()), ("i64", &ml)]);
                f.icmp("ne", "ptr", &r, "null")
            },
            |_| "false".into(),
        )
    };
    let global = has(&mut f, 103);
    let noempty = has(&mut f, 110);
    let testb = f.tag_is("%test", T_TRUE);
    let ng1 = f.add(&ng, "1");
    let nslots = f.bin("mul", "i64", &ng1, "2");
    let tot = f.add(&nslots, &plen);
    let sbytes = f.bin("mul", "i64", &tot, "8");
    let slots = f.call("ptr", "jv_malloc", &[("i64", &sbytes)]);
    let st = f.array_var("i64", 3);
    f.st_at("ptr", "null", &st, 0);
    f.st_at("i64", "0", &st, 8);
    f.st_at("i64", "0", &st, 16);
    let res = f.call(V, "jv_arr", &[("i64", "4")]);
    let resv = f.var_init(V, &res);
    let startv = f.var_init("i64", "0");
    let lenv = f.var("i64");
    let found_any = f.var_init("i1", "false");
    let cleanup = |f: &mut Func| {
        f.call("void", "free", &[("ptr", &buf)]);
        f.call("void", "free", &[("ptr", &slots)]);
        let b = f.ld_at("ptr", &st, 0);
        f.call("void", "free", &[("ptr", &b)]);
    };
    f.while_(
        |_| "true".into(),
        |f, brk, cont| {
            let start = f.load("i64", &startv);
            let past = f.icmp("sgt", "i64", &start, &n);
            f.if_then(&past, |f| f.br(brk));
            // search for a match starting at or after `start`
            let atv = f.var_init("i64", &start);
            let mend = f.var_init("i64", "-1");
            f.while_(
                |_| "true".into(),
                |f, sbrk, _| {
                    let at = f.load("i64", &atv);
                    f.call("ptr", "memset", &[("ptr", &slots), ("i32", "255"), ("i64", &sbytes)]);
                    let r = f.call(
                        "i64",
                        "re_exec",
                        &[("ptr", &buf), ("i64", "0"), ("ptr", &d), ("i64", &n), ("i64", &at), ("ptr", &slots), ("i64", &nslots), ("ptr", &st)],
                    );
                    let ok = f.icmp("sge", "i64", &r, "0");
                    f.if_then(&ok, |f| {
                        f.store("i64", &r, &mend);
                        f.br(sbrk);
                    });
                    let atn = f.icmp("sge", "i64", &at, &n);
                    f.if_then(&atn, |f| f.br(sbrk));
                    f.call("i64", "utf8_decode", &[("ptr", &d), ("i64", &n), ("i64", &at), ("ptr", &lenv)]);
                    let l = f.load("i64", &lenv);
                    let at2 = f.add(&at, &l);
                    f.store("i64", &at2, &atv);
                },
            );
            let me = f.load("i64", &mend);
            let none = f.lt(&me, "0");
            f.if_then(&none, |f| f.br(brk));
            let ms = f.load("i64", &slots);
            let me_p = f.off(&slots, "8");
            let me = f.load("i64", &me_p);
            let isempty = f.eq(&ms, &me);
            // next start
            let adv = f.if_val(
                &isempty,
                "i64",
                |f| {
                    let atn = f.icmp("sge", "i64", &me, &n);
                    f.if_val(
                        &atn,
                        "i64",
                        |f| f.add(&n, "1"),
                        |f| {
                            f.call("i64", "utf8_decode", &[("ptr", &d), ("i64", &n), ("i64", &me), ("ptr", &lenv)]);
                            let l = f.load("i64", &lenv);
                            f.add(&me, &l)
                        },
                    )
                },
                |_| me.clone(),
            );
            f.store("i64", &adv, &startv);
            let skip = f.and1(&isempty, &noempty);
            f.if_then(&skip, |f| {
                f.if_then(&global, |f| f.br(cont));
                f.br(brk);
            });
            f.store("i1", "true", &found_any);
            f.if_then(&testb, |f| f.br(brk));
            // build the match object
            let off = f.call("i64", "utf8_count", &[("ptr", &d), ("i64", &ms)]);
            let mp = f.off(&d, &ms);
            let ml = f.sub(&me, &ms);
            let mlen = f.call("i64", "utf8_count", &[("ptr", &mp), ("i64", &ml)]);
            let o = f.call(V, "jv_obj", &[]);
            let v = f.call(V, "re_num", &[("i64", &off)]);
            let o = f.call(V, "re_set", &[(V, &o), ("ptr", &k_offset), (V, &v)]);
            let v = f.call(V, "re_num", &[("i64", &mlen)]);
            let o = f.call(V, "re_set", &[(V, &o), ("ptr", &k_length), (V, &v)]);
            let v = f.call(V, "jv_str", &[("ptr", &mp), ("i64", &ml)]);
            let o = f.call(V, "re_set", &[(V, &o), ("ptr", &k_string), (V, &v)]);
            let caps = f.call(V, "jv_arr", &[("i64", &ng)]);
            let capv = f.var_init(V, &caps);
            f.for_range("1", &ng1, |f, g, _, _| {
                let g2 = f.bin("mul", "i64", g, "2");
                let sp_ = f.idx(&slots, &g2, 8);
                let cs = f.load("i64", &sp_);
                let g21 = f.add(&g2, "1");
                let ep = f.idx(&slots, &g21, 8);
                let ce = f.load("i64", &ep);
                let nm = f.call(V, "jv_arr_get", &[(V, &names), ("i64", g)]);
                f.call("void", "jv_retain", &[(V, &nm)]);
                let un1 = f.lt(&cs, "0");
                let un2 = f.lt(&ce, "0");
                let unmatched = f.or1(&un1, &un2);
                let c = f.if_val(
                    &unmatched,
                    V,
                    |f| {
                        let c = f.call(V, "jv_obj", &[]);
                        let v = f.call(V, "re_num", &[("i64", "-1")]);
                        let c = f.call(V, "re_set", &[(V, &c), ("ptr", &k_offset), (V, &v)]);
                        let c = f.call(V, "re_set", &[(V, &c), ("ptr", &k_string), (V, NULL)]);
                        let v = f.call(V, "re_num", &[("i64", "0")]);
                        let c = f.call(V, "re_set", &[(V, &c), ("ptr", &k_length), (V, &v)]);
                        f.call(V, "re_set", &[(V, &c), ("ptr", &k_name), (V, &nm)])
                    },
                    |f| {
                        let c = f.call(V, "jv_obj", &[]);
                        let co = f.call("i64", "utf8_count", &[("ptr", &d), ("i64", &cs)]);
                        let cp = f.off(&d, &cs);
                        let cl = f.sub(&ce, &cs);
                        let clen = f.call("i64", "utf8_count", &[("ptr", &cp), ("i64", &cl)]);
                        let v = f.call(V, "re_num", &[("i64", &co)]);
                        let c = f.call(V, "re_set", &[(V, &c), ("ptr", &k_offset), (V, &v)]);
                        // jq emits empty captures as {offset, string, length, name}
                        let empty = f.eq(&cl, "0");
                        let c = f.if_val(&empty, V, |f| {
                            let v = f.call(V, "jv_str", &[("ptr", &cp), ("i64", &cl)]);
                            let c = f.call(V, "re_set", &[(V, &c), ("ptr", &k_string), (V, &v)]);
                            let v = f.call(V, "re_num", &[("i64", &clen)]);
                            f.call(V, "re_set", &[(V, &c), ("ptr", &k_length), (V, &v)])
                        }, |f| {
                            let v = f.call(V, "re_num", &[("i64", &clen)]);
                            let c = f.call(V, "re_set", &[(V, &c), ("ptr", &k_length), (V, &v)]);
                            let v = f.call(V, "jv_str", &[("ptr", &cp), ("i64", &cl)]);
                            f.call(V, "re_set", &[(V, &c), ("ptr", &k_string), (V, &v)])
                        });
                        f.call(V, "re_set", &[(V, &c), ("ptr", &k_name), (V, &nm)])
                    },
                );
                let a = f.load(V, &capv);
                let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &c)]);
                f.call("void", "jv_release", &[(V, &c)]);
                f.store(V, &a2, &capv);
            });
            let caps = f.load(V, &capv);
            let o = f.call(V, "re_set", &[(V, &o), ("ptr", &k_captures), (V, &caps)]);
            let r = f.load(V, &resv);
            let r2 = f.call(V, "jv_arr_push", &[(V, &r), (V, &o)]);
            f.call("void", "jv_release", &[(V, &o)]);
            f.store(V, &r2, &resv);
            let ng_ = f.not1(&global);
            f.if_then(&ng_, |f| f.br(brk));
        },
    );
    cleanup(&mut f);
    f.if_then(&testb, |f| {
        let r = f.load(V, &resv);
        f.call("void", "jv_release", &[(V, &r)]);
        let fa = f.load("i1", &found_any);
        let b = f.select(&fa, V, TRUE, FALSE);
        f.ret(V, &b);
    });
    let r = f.load(V, &resv);
    f.ret(V, &r);
    m.add(f);
}
