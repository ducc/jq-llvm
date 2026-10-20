//! Incremental input: inputs are read with read(2) as they become available
//! (so `tail -f | prog` streams), concatenated across files like jq, and
//! parsed from a NUL-terminated buffer. A value cut off by the end of the
//! buffer is re-parsed once more data has arrived; regular files are read in
//! one go so huge documents are parsed once.
//!
//! Line tracking for error locations: the current file's name, where its data
//! starts in the buffer (null once that point has been discarded) and how many
//! of its lines were discarded.

use super::*;
use crate::ir::{Func, Module, V};

pub fn emit(m: &mut Module) {
    for g in [
        "@g_cap = internal global i64 0",
        "@g_srcs = internal global ptr null",
        "@g_nsrc = internal global i64 0",
        "@g_srci = internal global i64 0",
        "@g_fd = internal global i32 -1",
        "@g_eof = internal global i1 false",
        "@g_regular = internal global i1 false",
        "@g_fname = internal global ptr null",
        "@g_fstart = internal global ptr null",
        "@g_flines = internal global i64 0",
        "@g_bomcheck = internal global i1 true",
        "@g_readok = internal global i1 false",
        "@g_fcol = internal global i64 0",
        "@g_bulk = internal global i1 false",
        "@g_pending = internal global i1 false",
    ] {
        m.global(g.to_string());
    }
    m.declare("open", "declare i32 @open(ptr, i32, ...)");
    m.declare("read", "declare i64 @read(i32, ptr, i64)");
    m.declare("close", "declare i32 @close(i32)");
    m.declare("lseek", "declare i64 @lseek(i32, i64, i32)");
    m.declare("memrchr", "declare ptr @memrchr(ptr, i32, i64)");
    helpers(m);
    complete_scan(m);
    fill(m);
    next_input(m);
    locations(m);
}

fn helpers(m: &mut Module) {
    // count newlines in [a, b)
    let mut f = Func::new("internal", "i64", "jq_count_nl", &[("ptr", "a"), ("ptr", "b")], "");
    let ai = f.cast("ptrtoint", "ptr", "%a", "i64");
    let bi = f.cast("ptrtoint", "ptr", "%b", "i64");
    let n = f.sub(&bi, &ai);
    let cnt = f.var_init("i64", "0");
    f.for_range("0", &n, |f, i, _, _| {
        let p = f.off("%a", i);
        let b = f.load("i8", &p);
        let nl = f.icmp("eq", "i8", &b, "10");
        let x = f.cast("zext", "i1", &nl, "i64");
        let c = f.load("i64", &cnt);
        let c2 = f.add(&c, &x);
        f.store("i64", &c2, &cnt);
    });
    let c = f.load("i64", &cnt);
    f.ret("i64", &c);
    m.add(f);

    // set up the reader over `files` (n == 0: stdin)
    let mut f = Func::new("internal", "void", "jq_input_init", &[("ptr", "files"), ("i64", "n")], "");
    let buf = f.call("ptr", "jv_malloc", &[("i64", "65537")]);
    f.store("i8", "0", &buf);
    f.store("ptr", &buf, "@g_buf");
    f.store("ptr", &buf, "@g_cur");
    f.store("ptr", &buf, "@g_end");
    f.store("ptr", &buf, "@g_fstart");
    f.store("i64", "65536", "@g_cap");
    f.store("ptr", "%files", "@g_srcs");
    f.store("i64", "%n", "@g_nsrc");
    f.ret_void();
    m.add(f);

    // open the next source; false when there are none left
    let cant = m.cstr("jq: error: Could not open file ");
    let colon = m.cstr(": ");
    let nl = m.cstr("\n");
    let mut f = Func::new("internal", "i1", "jq_open_next", &[], "");
    let n = f.load("i64", "@g_nsrc");
    let i = f.load("i64", "@g_srci");
    let isstdin = f.eq(&n, "0");
    f.if_then(&isstdin, |f| {
        let first = f.eq(&i, "0");
        f.if_then(&first, |f| {
            f.store("i64", "1", "@g_srci");
            f.store("i32", "0", "@g_fd");
            f.store("ptr", "null", "@g_fname");
            let end = f.load("ptr", "@g_end");
            f.store("ptr", &end, "@g_fstart");
            f.store("i64", "0", "@g_flines");
            let pos = f.call("i64", "lseek", &[("i32", "0"), ("i64", "0"), ("i32", "1")]);
            let reg = f.icmp("sge", "i64", &pos, "0");
            f.store("i1", &reg, "@g_regular");
            f.ret("i1", "true");
        });
        f.ret("i1", "false");
    });
    f.while_(
        |f| {
            let i = f.load("i64", "@g_srci");
            f.lt(&i, &n)
        },
        |f, _, _| {
            let i = f.load("i64", "@g_srci");
            let i1 = f.add(&i, "1");
            f.store("i64", &i1, "@g_srci");
            let srcs = f.load("ptr", "@g_srcs");
            let pp = f.idx(&srcs, &i, 8);
            let path = f.load("ptr", &pp);
            // "-" means standard input
            let dash = m_dash(f, &path);
            let fdo = f.ins(format!("call i32 (ptr, i32, ...) @open(ptr {}, i32 0)", path));
            let fd = f.select(&dash, "i32", "0", &fdo);
            let bad = f.icmp("slt", "i32", &fd, "0");
            f.if_then(&bad, |f| {
                let ep = f.call("ptr", "__errno_location", &[]);
                let errno = f.load("i32", &ep);
                let e = f.load("ptr", "@stderr");
                f.call("i32", "fputs", &[("ptr", &cant), ("ptr", &e)]);
                f.call("i32", "fputs", &[("ptr", &path), ("ptr", &e)]);
                f.call("i32", "fputs", &[("ptr", &colon), ("ptr", &e)]);
                let msg = f.call("ptr", "strerror", &[("i32", &errno)]);
                f.call("i32", "fputs", &[("ptr", &msg), ("ptr", &e)]);
                f.call("i32", "fputs", &[("ptr", &nl), ("ptr", &e)]);
                f.store("i32", "2", "@g_ret");
            });
            let ok = f.not1(&bad);
            f.if_then(&ok, |f| {
                f.store("i32", &fd, "@g_fd");
                let nm = f.select(&dash, "ptr", "null", &path);
                f.store("ptr", &nm, "@g_fname");
                f.store("i1", "false", "@g_readok");
                f.store("i64", "0", "@g_fcol");
                let end = f.load("ptr", "@g_end");
                f.store("ptr", &end, "@g_fstart");
                f.store("i64", "0", "@g_flines");
                let sz = f.call("i64", "lseek", &[("i32", &fd), ("i64", "0"), ("i32", "2")]);
                let reg = f.icmp("sge", "i64", &sz, "0");
                f.store("i1", &reg, "@g_regular");
                f.if_then(&reg, |f| {
                    f.call("i64", "lseek", &[("i32", &fd), ("i64", "0"), ("i32", "0")]);
                });
                f.ret("i1", "true");
            });
        },
    );
    f.ret("i1", "false");
    m.add(f);

    // drop consumed input [buf, cur), keeping line counts right
    let mut f = Func::new("internal", "void", "jq_compact", &[], "");
    let buf = f.load("ptr", "@g_buf");
    let cur = f.load("ptr", "@g_cur");
    let end = f.load("ptr", "@g_end");
    let same = f.icmp("eq", "ptr", &buf, &cur);
    f.if_then(&same, |f| f.ret_void());
    let fs = f.load("ptr", "@g_fstart");
    let fnull = f.icmp("eq", "ptr", &fs, "null");
    let before = f.icmp("ule", "ptr", &fs, &cur);
    let gone = f.or1(&fnull, &before);
    let bi = f.cast("ptrtoint", "ptr", &buf, "i64");
    let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
    let shift = f.sub(&ci, &bi);
    // column at the new buffer start
    let inreg = {
        let a = f.icmp("ne", "ptr", &fs, "null");
        let b = f.icmp("uge", "ptr", &fs, &buf);
        let c = f.icmp("ule", "ptr", &fs, &cur);
        let ab = f.and1(&a, &b);
        f.and1(&ab, &c)
    };
    let rs = f.select(&inreg, "ptr", &fs, &buf);
    let fc = f.load("i64", "@g_fcol");
    let c0 = f.select(&inreg, "i64", "0", &fc);
    let rsi = f.cast("ptrtoint", "ptr", &rs, "i64");
    let rl = f.sub(&ci, &rsi);
    let lastnl = f.call("ptr", "memrchr", &[("ptr", &rs), ("i32", "10"), ("i64", &rl)]);
    let hasnl = f.icmp("ne", "ptr", &lastnl, "null");
    let nli = f.cast("ptrtoint", "ptr", &lastnl, "i64");
    let after = f.sub(&ci, &nli);
    let after = f.sub(&after, "1");
    let plus = f.add(&c0, &rl);
    let ncol = f.select(&hasnl, "i64", &after, &plus);
    f.store("i64", &ncol, "@g_fcol");
    f.if_else(
        &gone,
        |f| {
            let from = f.select(&fnull, "ptr", &buf, &fs);
            let k = f.call("i64", "jq_count_nl", &[("ptr", &from), ("ptr", &cur)]);
            let l = f.load("i64", "@g_flines");
            let l2 = f.add(&l, &k);
            f.store("i64", &l2, "@g_flines");
            f.store("ptr", "null", "@g_fstart");
        },
        |f| {
            let nshift = f.sub("0", &shift);
            let fs2 = f.off(&fs, &nshift);
            f.store("ptr", &fs2, "@g_fstart");
        },
    );
    let ei = f.cast("ptrtoint", "ptr", &end, "i64");
    let keep = f.sub(&ei, &ci);
    f.call("ptr", "memmove", &[("ptr", &buf), ("ptr", &cur), ("i64", &keep)]);
    let ne = f.off(&buf, &keep);
    f.store("i8", "0", &ne);
    f.store("ptr", &buf, "@g_cur");
    f.store("ptr", &ne, "@g_end");
    f.ret_void();
    m.add(f);
}

fn fill(m: &mut Module) {
    let bom = m.cstr([0xEFu8, 0xBB, 0xBF]);
    let pre_err = m.cstr("jq: error: ");
    let nl2 = m.cstr("\n");
    // read more input; false once every source is exhausted
    // `minadd`: keep reading until at least this many bytes were added (or EOF)
    let mut f = Func::new("internal", "i1", "jq_fill", &[("i64", "minadd")], "");
    let eof = f.load("i1", "@g_eof");
    f.if_then(&eof, |f| f.ret("i1", "false"));
    f.call("void", "jq_compact", &[]);
    let added = f.var_init("i64", "0");
    f.while_(
        |_| "true".into(),
        |f, _, cont| {
            let fd = f.load("i32", "@g_fd");
            let nofd = f.icmp("slt", "i32", &fd, "0");
            f.if_then(&nofd, |f| {
                let ok = f.call("i1", "jq_open_next", &[]);
                let none = f.not1(&ok);
                f.if_then(&none, |f| {
                    f.store("i1", "true", "@g_eof");
                    let a = f.load("i64", &added);
                    let any = f.icmp("sgt", "i64", &a, "0");
                    f.ret("i1", &any);
                });
            });
            let fd = f.load("i32", "@g_fd");
            let buf = f.load("ptr", "@g_buf");
            let cur = f.load("ptr", "@g_cur");
            let end = f.load("ptr", "@g_end");
            let fs = f.load("ptr", "@g_fstart");
            let bi = f.cast("ptrtoint", "ptr", &buf, "i64");
            let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
            let ei = f.cast("ptrtoint", "ptr", &end, "i64");
            let keep = f.sub(&ei, &bi);
            // regular files: everything that is left; pipes: a chunk (growing with the value)
            let reg0 = f.load("i1", "@g_regular");
            let rok = f.load("i1", "@g_readok");
            let reg = f.and1(&reg0, &rok);
            let want = f.if_val(
                &reg,
                "i64",
                |f| {
                    let pos = f.call("i64", "lseek", &[("i32", &fd), ("i64", "0"), ("i32", "1")]);
                    let size = f.call("i64", "lseek", &[("i32", &fd), ("i64", "0"), ("i32", "2")]);
                    f.call("i64", "lseek", &[("i32", &fd), ("i64", &pos), ("i32", "0")]);
                    let rem = f.sub(&size, &pos);
                    let small = f.icmp("slt", "i64", &rem, "65536");
                    f.select(&small, "i64", "65536", &rem)
                },
                |f| {
                    let small = f.icmp("slt", "i64", &keep, "65536");
                    f.select(&small, "i64", "65536", &keep)
                },
            );
            let cap = f.load("i64", "@g_cap");
            let need = f.add(&keep, &want);
            let grow = f.icmp("sgt", "i64", &need, &cap);
            f.if_then(&grow, |f| {
                let sz = f.add(&need, "1");
                let nb = f.call("ptr", "jv_realloc", &[("ptr", &buf), ("i64", &sz)]);
                f.store("i64", &need, "@g_cap");
                f.store("ptr", &nb, "@g_buf");
                let co = f.sub(&ci, &bi);
                let nc = f.off(&nb, &co);
                f.store("ptr", &nc, "@g_cur");
                let ne = f.off(&nb, &keep);
                f.store("ptr", &ne, "@g_end");
                let fsnull = f.icmp("eq", "ptr", &fs, "null");
                let nf = f.not1(&fsnull);
                f.if_then(&nf, |f| {
                    let fi = f.cast("ptrtoint", "ptr", &fs, "i64");
                    let fo = f.sub(&fi, &bi);
                    let nfs = f.off(&nb, &fo);
                    f.store("ptr", &nfs, "@g_fstart");
                });
            });
            let end = f.load("ptr", "@g_end");
            let r = f.call("i64", "read", &[("i32", &fd), ("ptr", &end), ("i64", &want)]);
            let got = f.icmp("sgt", "i64", &r, "0");
            let failed = f.icmp("slt", "i64", &r, "0");
            f.if_then(&failed, |f| {
                let ep = f.call("ptr", "__errno_location", &[]);
                let errno = f.load("i32", &ep);
                let e = f.load("ptr", "@stderr");
                f.call("i32", "fputs", &[("ptr", &pre_err), ("ptr", &e)]);
                let msg = f.call("ptr", "strerror", &[("i32", &errno)]);
                f.call("i32", "fputs", &[("ptr", &msg), ("ptr", &e)]);
                f.call("i32", "fputs", &[("ptr", &nl2), ("ptr", &e)]);
                f.store("i32", "2", "@g_ret");
            });
            f.if_then(&got, |f| {
                f.store("i1", "true", "@g_readok");
                let full = f.icmp("sge", "i64", &r, "65536");
                f.store("i1", &full, "@g_bulk");
                let ne = f.off(&end, &r);
                f.store("i8", "0", &ne);
                f.store("ptr", &ne, "@g_end");
                // a UTF-8 byte order mark is skipped at the start of the input
                let bc = f.load("i1", "@g_bomcheck");
                f.if_then(&bc, |f| {
                    let cur = f.load("ptr", "@g_cur");
                    let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
                    let nei = f.cast("ptrtoint", "ptr", &ne, "i64");
                    let avail = f.sub(&nei, &ci);
                    let enough = f.icmp("sge", "i64", &avail, "3");
                    f.if_then(&enough, |f| {
                        f.store("i1", "false", "@g_bomcheck");
                        let c = f.call("i32", "memcmp", &[("ptr", &cur), ("ptr", &bom), ("i64", "3")]);
                        let z = f.icmp("eq", "i32", &c, "0");
                        f.if_then(&z, |f| {
                            let c3 = f.off(&cur, "3");
                            f.store("ptr", &c3, "@g_cur");
                        });
                    });
                });
                let a = f.load("i64", &added);
                let a2 = f.add(&a, &r);
                f.store("i64", &a2, &added);
                let enough = f.icmp("sge", "i64", &a2, "%minadd");
                f.if_then(&enough, |f| f.ret("i1", "true"));
                f.br(cont);
            });
            // end of this source: move on to the next one
            let notstdin = f.icmp("ne", "i32", &fd, "0");
            f.if_then(&notstdin, |f| f.call("i32", "close", &[("i32", &fd)]));
            f.store("i32", "-1", "@g_fd");
        },
    );
    f.unreachable();
    m.add(f);

    // all remaining input as one string (for -R -s)
    let mut f = Func::new("internal", V, "jq_read_rest", &[], "");
    f.while_(|f| f.call("i1", "jq_fill", &[("i64", "1073741824")]), |_, _, _| {});
    let cur = f.load("ptr", "@g_cur");
    let end = f.load("ptr", "@g_end");
    let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
    let ei = f.cast("ptrtoint", "ptr", &end, "i64");
    let n = f.sub(&ei, &ci);
    let s = f.call(V, "jv_str", &[("ptr", &cur), ("i64", &n)]);
    f.store("ptr", &end, "@g_cur");
    f.ret(V, &s);
    m.add(f);
}

fn next_input(m: &mut Module) {
    // next input value; tag 10 = EOF
    let mut f = Func::new("internal", V, "jv_next_input_raw", &[], "");
    let sl = f.load("i64", "@g_slurp");
    let iss = f.ne(&sl, "0");
    f.if_then(&iss, |f| {
        let done = f.load("i64", "@g_slurp_done");
        let d = f.ne(&done, "0");
        f.if_then(&d, |f| f.ret(V, "{ i64 10, i64 0 }"));
        f.store("i64", "1", "@g_slurp_done");
        f.store("i64", "1", "@g_any_input");
        let v = f.load(V, "@g_slurped");
        f.call("void", "jv_retain", &[(V, &v)]);
        f.ret(V, &v);
    });
    let raw = f.load("i64", "@g_rawin");
    let israw = f.ne(&raw, "0");
    f.if_then(&israw, |f| {
        f.while_(
            |_| "true".into(),
            |f, _, _| {
                let cur = f.load("ptr", "@g_cur");
                let end = f.load("ptr", "@g_end");
                let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
                let ei = f.cast("ptrtoint", "ptr", &end, "i64");
                let rem = f.sub(&ei, &ci);
                let nl = f.call("ptr", "memchr", &[("ptr", &cur), ("i32", "10"), ("i64", &rem)]);
                let found = f.icmp("ne", "ptr", &nl, "null");
                f.if_then(&found, |f| {
                    f.store("i64", "1", "@g_any_input");
                    let ni = f.cast("ptrtoint", "ptr", &nl, "i64");
                    let len = f.sub(&ni, &ci);
                    let s = f.call(V, "jv_str", &[("ptr", &cur), ("i64", &len)]);
                    let nx = f.off(&nl, "1");
                    f.store("ptr", &nx, "@g_cur");
                    f.ret(V, &s);
                });
                let more = f.call("i1", "jq_fill", &[("i64", "0")]);
                let done = f.not1(&more);
                f.if_then(&done, |f| {
                    let cur = f.load("ptr", "@g_cur");
                    let end = f.load("ptr", "@g_end");
                    let empty = f.icmp("uge", "ptr", &cur, &end);
                    f.if_then(&empty, |f| f.ret(V, "{ i64 10, i64 0 }"));
                    f.store("i64", "1", "@g_any_input");
                    let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
                    let ei = f.cast("ptrtoint", "ptr", &end, "i64");
                    let rem = f.sub(&ei, &ci);
                    let s = f.call(V, "jv_str", &[("ptr", &cur), ("i64", &rem)]);
                    f.store("ptr", &end, "@g_cur");
                    f.ret(V, &s);
                });
            },
        );
        f.unreachable();
    });
    f.while_(
        |_| "true".into(),
        |f, _, cont| {
            f.call("void", "jp_ws", &[("ptr", "@g_cur")]);
            let cur = f.load("ptr", "@g_cur");
            let end = f.load("ptr", "@g_end");
            let atend = f.icmp("uge", "ptr", &cur, &end);
            f.if_then(&atend, |f| {
                let more = f.call("i1", "jq_fill", &[("i64", "0")]);
                f.if_then(&more, |f| f.br(cont));
                f.ret(V, "{ i64 10, i64 0 }");
            });
            // streaming input: make sure a container is complete before parsing it
            let eof0 = f.load("i1", "@g_eof");
            let reg = f.load("i1", "@g_regular");
            let b0 = f.load("i8", &cur);
            let o1 = f.icmp("eq", "i8", &b0, "91");
            let o2 = f.icmp("eq", "i8", &b0, "123");
            let iscont = f.or1(&o1, &o2);
            let ne = f.not1(&eof0);
            let nr = f.not1(&reg);
            let pend = f.load("i1", "@g_pending");
            let chk = f.and1(&iscont, &ne);
            let chk = f.and1(&chk, &nr);
            let chk = f.and1(&chk, &pend);
            f.if_then(&chk, |f| {
                let done = f.call("i1", "jp_complete", &[("ptr", &cur), ("ptr", &end)]);
                let nd = f.not1(&done);
                f.if_then(&nd, |f| {
                    let mn = retry_min(f);
                    f.call("i1", "jq_fill", &[("i64", &mn)]);
                    f.br(cont);
                });
            });
            f.store("ptr", &cur, "@g_valstart");
            let v = f.call(V, "jp_top", &[("ptr", "@g_cur")]);
            let eof = f.load("i1", "@g_eof");
            let iserr = f.tag_is(&v, T_ERR);
            f.if_then(&iserr, |f| {
                // ran off the end of the buffer: read more and parse the value again
                let pe = f.load("i1", "@g_perr_eof");
                let pp = f.load("ptr", "@g_perr_pos");
                let end = f.load("ptr", "@g_end");
                let atend = f.icmp("uge", "ptr", &pp, &end);
                let partial = f.and1(&pe, &atend);
                let noteof = f.not1(&eof);
                let retry = f.and1(&partial, &noteof);
                f.if_then(&retry, |f| {
                    f.call("void", "jv_release", &[(V, &v)]);
                    f.store("i1", "true", "@g_pending");
                    f.store("ptr", &cur, "@g_cur");
                    let mn = retry_min(f);
                    f.call("i1", "jq_fill", &[("i64", &mn)]);
                    f.br(cont);
                });
                f.store("i64", "1", "@g_any_input");
                f.ret(V, &v);
            });
            // a bare word ending exactly at the buffer end might continue
            let ncur = f.load("ptr", "@g_cur");
            let end = f.load("ptr", "@g_end");
            let atend = f.icmp("uge", "ptr", &ncur, &end);
            let b0 = f.load("i8", &cur);
            let isd = f.call("i1", "jp_isdelim", &[("i8", &b0)]);
            let lit = f.not1(&isd);
            let noteof = f.not1(&eof);
            let x = f.and1(&atend, &lit);
            let retry = f.and1(&x, &noteof);
            f.if_then(&retry, |f| {
                f.call("void", "jv_release", &[(V, &v)]);
                f.store("i1", "true", "@g_pending");
                f.store("ptr", &cur, "@g_cur");
                let mn = retry_min(f);
                f.call("i1", "jq_fill", &[("i64", &mn)]);
                f.br(cont);
            });
            f.store("i64", "1", "@g_any_input");
            f.store("i1", "false", "@g_pending");
            f.ret(V, &v);
        },
    );
    f.unreachable();
    m.add(f);
}

fn locations(m: &mut Module) {
    // lines of the current file consumed by the parser (jq counts a newline
    // right after the value too)
    let mut f = Func::new("internal", "i64", "jq_cur_lines", &[], "");
    let buf = f.load("ptr", "@g_buf");
    let cur = f.load("ptr", "@g_cur");
    let end = f.load("ptr", "@g_end");
    let fs = f.load("ptr", "@g_fstart");
    let fnull = f.icmp("eq", "ptr", &fs, "null");
    let from0 = f.select(&fnull, "ptr", &buf, &fs);
    // the current file may start after the cursor (value finished in the previous file)
    let after = f.icmp("ugt", "ptr", &from0, &cur);
    let from = f.select(&after, "ptr", &cur, &from0);
    let k = f.call("i64", "jq_count_nl", &[("ptr", &from), ("ptr", &cur)]);
    let more = f.icmp("ult", "ptr", &cur, &end);
    let nxt = f.if_val(&more, "i64", |f| {
        let b = f.load("i8", &cur);
        let nl = f.icmp("eq", "i8", &b, "10");
        f.cast("zext", "i1", &nl, "i64")
    }, |_| "0".into());
    let l = f.load("i64", "@g_flines");
    let a = f.add(&l, &k);
    let r = f.add(&a, &nxt);
    f.ret("i64", &r);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_input_line_number", &[(V, "x")], "");
    let n = f.call("i64", "jq_cur_lines", &[]);
    let d = f.cast("sitofp", "i64", &n, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_input_filename", &[(V, "x")], "");
    let name = f.load("ptr", "@g_fname");
    let none = f.icmp("eq", "ptr", &name, "null");
    f.if_then(&none, |f| f.ret(V, NULL));
    let s = f.call(V, "jv_cstr", &[("ptr", &name)]);
    f.ret(V, &s);
    m.add(f);
}

fn m_dash(f: &mut Func, path: &str) -> String {
    let b0 = f.load("i8", path);
    let p1 = f.off(path, "1");
    let b1 = f.load("i8", &p1);
    let d = f.icmp("eq", "i8", &b0, "45");
    let z = f.icmp("eq", "i8", &b1, "0");
    f.and1(&d, &z)
}

/// Bytes to read before re-parsing a value cut off by the buffer end: small
/// values re-parse after any new data (good for interactive streams); values
/// over 1 MB double the buffer first so huge documents parse O(n log n).
fn retry_min(f: &mut Func) -> String {
    let cur = f.load("ptr", "@g_cur");
    let end = f.load("ptr", "@g_end");
    let ci = f.cast("ptrtoint", "ptr", &cur, "i64");
    let ei = f.cast("ptrtoint", "ptr", &end, "i64");
    let pending = f.sub(&ei, &ci);
    let big = f.icmp("sge", "i64", &pending, "1048576");
    // data arriving in full chunks: read ahead as well
    let bulk = f.load("i1", "@g_bulk");
    let ahead = f.or1(&big, &bulk);
    let small = f.icmp("slt", "i64", &pending, "65536");
    let amt = f.select(&small, "i64", "65536", &pending);
    f.select(&ahead, "i64", &amt, "0")
}

/// Is the container value starting at `p` complete before `end`? (bracket
/// balance, skipping string contents) — cheap check before a full parse.
pub fn complete_scan(m: &mut Module) {
    let mut f = Func::new("internal", "i1", "jp_complete", &[("ptr", "p"), ("ptr", "end")], "");
    let depth = f.var_init("i64", "0");
    let cv = f.var_init("ptr", "%p");
    let instr = f.var_init("i1", "false");
    f.while_(
        |f| {
            let c = f.load("ptr", &cv);
            f.icmp("ult", "ptr", &c, "%end")
        },
        |f, _, _| {
            let c = f.load("ptr", &cv);
            let b = f.load("i8", &c);
            let c1 = f.off(&c, "1");
            f.store("ptr", &c1, &cv);
            let s = f.load("i1", &instr);
            f.if_else(
                &s,
                |f| {
                    let bs = f.icmp("eq", "i8", &b, "92");
                    f.if_then(&bs, |f| {
                        let c2 = f.off(&c, "2");
                        f.store("ptr", &c2, &cv);
                    });
                    let q = f.icmp("eq", "i8", &b, "34");
                    f.if_then(&q, |f| f.store("i1", "false", &instr));
                },
                |f| {
                    let q = f.icmp("eq", "i8", &b, "34");
                    f.if_then(&q, |f| f.store("i1", "true", &instr));
                    let o1 = f.icmp("eq", "i8", &b, "91");
                    let o2 = f.icmp("eq", "i8", &b, "123");
                    let open = f.or1(&o1, &o2);
                    f.if_then(&open, |f| {
                        let d = f.load("i64", &depth);
                        let d1 = f.add(&d, "1");
                        f.store("i64", &d1, &depth);
                    });
                    let c1_ = f.icmp("eq", "i8", &b, "93");
                    let c2_ = f.icmp("eq", "i8", &b, "125");
                    let close = f.or1(&c1_, &c2_);
                    f.if_then(&close, |f| {
                        let d = f.load("i64", &depth);
                        let d1 = f.sub(&d, "1");
                        f.store("i64", &d1, &depth);
                        let z = f.icmp("sle", "i64", &d1, "0");
                        f.if_then(&z, |f| f.ret("i1", "true"));
                    });
                },
            );
        },
    );
    f.ret("i1", "false");
    m.add(f);
}
