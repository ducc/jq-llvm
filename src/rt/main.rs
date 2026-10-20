//! Program entry point: CLI option parsing, input reading, output, error reporting.
//! The compiled filter provides `@jq_init()` and
//! `%V @jq_program(ptr env, %V input, ptr k, ptr kenv)`.

use super::*;
use crate::ir::{Func, Module, V};

pub fn emit(m: &mut Module) {
    for g in [
        "@g_out = internal global [24 x i8] zeroinitializer",
        "@g_flags = internal global i64 1",
        "@g_raw = internal global i64 0",
        "@g_join = internal global i64 0",
        "@g_nul = internal global i64 0",
        "@g_eflag = internal global i64 0",
        "@g_last = internal global i64 0",
        "@g_rawin = internal global i64 0",
        "@g_slurp = internal global i64 0",
        "@g_slurp_done = internal global i64 0",
        "@g_slurped = internal global %V zeroinitializer",
        "@g_buf = internal global ptr null",
        "@g_cur = internal global ptr null",
        "@g_end = internal global ptr null",
        "@g_any_input = internal global i64 0",
        "@g_named = internal global %V zeroinitializer",
        "@g_positional = internal global %V zeroinitializer",
        "@g_ret = internal global i32 0",
        "@g_env = internal global %V zeroinitializer",
        "@g_tty = internal global i1 false",
        "@g_err_static = internal global %V zeroinitializer",
        "@g_status = internal global i32 0",
        "@g_color = internal global i64 -1",
        "@g_stream = internal global i64 0",
        "@g_seq = internal global i64 0",
        "@g_unbuffered = internal global i64 0",
        "@g_sq = internal global %V zeroinitializer",
        "@g_sqi = internal global i64 0",
        "@g_valstart = internal global ptr null",
    ] {
        m.global(g.to_string());
    }
    output(m);
    input(m);
    report(m);
    main_fn(m);
    thread_main(m);
    tune_malloc(m);
    parse_report(m);
    stream(m);
    next_input_stream(m);
}

fn output(m: &mut Module) {
    // write the output buffer straight to fd 1 (no stdio buffering in between)
    let mut f = Func::new("internal", "void", "jq_flush", &[], "");
    let d = f.ld_at("ptr", "@g_out", 0);
    let n = f.ld_at("i64", "@g_out", 8);
    let done = f.var_init("i64", "0");
    f.while_(
        |f| {
            let k = f.load("i64", &done);
            f.lt(&k, &n)
        },
        |f, brk, _| {
            let k = f.load("i64", &done);
            let p = f.off(&d, &k);
            let left = f.sub(&n, &k);
            let w = f.call("i64", "write", &[("i32", "1"), ("ptr", &p), ("i64", &left)]);
            let bad = f.icmp("sle", "i64", &w, "0");
            f.if_then(&bad, |f| f.br(brk));
            let k2 = f.add(&k, &w);
            f.store("i64", &k2, &done);
        },
    );
    f.st_at("i64", "0", "@g_out", 8);
    f.ret_void();
    m.add(f);

    // output callback: the final continuation of the program
    let mut f = Func::new("internal", V, "jq_out", &[("ptr", "env"), (V, "x")], "");
    let raw = f.load("i64", "@g_raw");
    let israw = f.ne(&raw, "0");
    let isstr = f.tag_is("%x", T_STR);
    let rs = f.and1(&israw, &isstr);
    f.if_else(
        &rs,
        |f| {
            f.call("void", "sb_putv", &[("ptr", "@g_out"), (V, "%x")]);
        },
        |f| {
            let fl = f.load("i64", "@g_flags");
            let c = f.bin("and", "i64", &fl, "16");
            let col = f.ne(&c, "0");
            f.if_else(&col, |f| f.call("void", "jv_dump_color", &[("ptr", "@g_out"), (V, "%x"), ("i64", &fl), ("i64", "0")]), |f| f.call("void", "jv_dump", &[("ptr", "@g_out"), (V, "%x"), ("i64", &fl), ("i64", "0")]));
        },
    );
    let seq = f.load("i64", "@g_seq");
    let isseq = f.ne(&seq, "0");
    f.if_then(&isseq, |f| f.call("void", "sb_putc", &[("ptr", "@g_out"), ("i8", "30")]));
    let nul = f.load("i64", "@g_nul");
    let isnul = f.ne(&nul, "0");
    f.if_then(&isnul, |f| f.call("void", "sb_putc", &[("ptr", "@g_out"), ("i8", "0")]));
    let j = f.load("i64", "@g_join");
    let nl = f.eq(&j, "0");
    let nn = f.not1(&isnul);
    let donl = f.and1(&nl, &nn);
    f.if_then(&donl, |f| f.call("void", "sb_putc", &[("ptr", "@g_out"), ("i8", "10")]));
    let t = f.tag("%x");
    let truthy = f.icmp("sge", "i64", &t, "2");
    let lv = f.select(&truthy, "i64", "2", "1");
    f.store("i64", &lv, "@g_last");
    let n = f.ld_at("i64", "@g_out", 8);
    // line-buffered on a terminal, block-buffered otherwise (like stdio)
    let tty = f.load("i1", "@g_tty");
    let ub = f.load("i64", "@g_unbuffered");
    let ubf = f.ne(&ub, "0");
    let full = f.icmp("sgt", "i64", &n, "65536");
    let big = f.or1(&full, &tty);
    let big = f.or1(&big, &ubf);
    f.if_then(&big, |f| f.call("void", "jq_flush", &[]));
    f.ret(V, OK);
    m.add(f);
}

fn input(m: &mut Module) {
    let rmode = m.cstr("rb");
    // read a whole FILE* into the builder
    let mut f = Func::new("internal", "void", "jq_read_all", &[("ptr", "b"), ("ptr", "fp")], "");
    f.while_(
        |_| "true".into(),
        |f, brk, _| {
            let len = f.ld_at("i64", "%b", 8);
            let need = f.add(&len, "65536");
            let cap = f.ld_at("i64", "%b", 16);
            let small = f.icmp("sgt", "i64", &need, &cap);
            f.if_then(&small, |f| f.call("void", "sb_grow", &[("ptr", "%b"), ("i64", &need)]));
            let d = f.ld_at("ptr", "%b", 0);
            let p = f.off(&d, &len);
            let n = f.call("i64", "fread", &[("ptr", &p), ("i64", "1"), ("i64", "65536"), ("ptr", "%fp")]);
            let nl = f.add(&len, &n);
            f.st_at("i64", &nl, "%b", 8);
            let z = f.eq(&n, "0");
            f.if_then(&z, |f| f.br(brk));
        },
    );
    f.ret_void();
    m.add(f);

    let cant = m.cstr("jq: error: Could not open file ");
    let mut f = Func::new("internal", "i1", "jq_read_file", &[("ptr", "b"), ("ptr", "path")], "");
    let fp = f.call("ptr", "fopen", &[("ptr", "%path"), ("ptr", &rmode)]);
    let isnull = f.icmp("eq", "ptr", &fp, "null");
    f.if_then(&isnull, |f| {
        let e = f.load("ptr", "@stderr");
        let n = f.call("i64", "strlen", &[("ptr", &cant)]);
        f.call("i64", "fwrite", &[("ptr", &cant), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
        let ep = f.call("ptr", "__errno_location", &[]);
        let errno = f.load("i32", &ep);
        let n = f.call("i64", "strlen", &[("ptr", "%path")]);
        f.call("i64", "fwrite", &[("ptr", "%path"), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
        let colon = m.cstr(": ");
        f.call("i32", "fputs", &[("ptr", &colon), ("ptr", &e)]);
        let msg = f.call("ptr", "strerror", &[("i32", &errno)]);
        f.call("i32", "fputs", &[("ptr", &msg), ("ptr", &e)]);
        let nl = m.cstr("\n");
        f.call("i64", "fwrite", &[("ptr", &nl), ("i64", "1"), ("i64", "1"), ("ptr", &e)]);
        f.ret("i1", "false");
    });
    f.call("void", "jq_read_all", &[("ptr", "%b"), ("ptr", &fp)]);
    f.call("i32", "fclose", &[("ptr", &fp)]);
    f.ret("i1", "true");
    m.add(f);

}

fn report(m: &mut Module) {
    let pre = m.cstr("jq: error (at ");
    let unk = m.cstr("<unknown>");
    let stdin_ = m.cstr("<stdin>:");
    let mid = m.cstr("): ");
    let notstr = m.cstr(") (not a string): ");
    let brk = m.cstr("break");

    // location string into builder
    let mut f = Func::new("internal", "void", "jq_loc", &[("ptr", "b")], "");
    let any = f.load("i64", "@g_any_input");
    let none = f.eq(&any, "0");
    f.if_then(&none, |f| {
        f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &unk)]);
        f.ret_void();
    });
    // "<stdin>:N" or "<file>:N" with N counted within that file
    let name = f.load("ptr", "@g_fname");
    let isstdin = f.icmp("eq", "ptr", &name, "null");
    f.if_else(
        &isstdin,
        |f| f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &stdin_)]),
        |f| {
            f.call("void", "sb_puts", &[("ptr", "%b"), ("ptr", &name)]);
            f.call("void", "sb_putc", &[("ptr", "%b"), ("i8", "58")]);
        },
    );
    let c = f.call("i64", "jq_cur_lines", &[]);
    f.call("void", "sb_int", &[("ptr", "%b"), ("i64", &c)]);
    f.ret_void();
    m.add(f);
    // print an error status (error / bypass / break) to stderr
    let mut f = Func::new("internal", "void", "jq_report", &[(V, "st")], "");
    let sv = f.var_init(V, "%st");
    f.call("void", "jv_retain", &[(V, "%st")]);
    f.while_(
        |f| {
            let s = f.load(V, &sv);
            f.tag_is(&s, T_BYPASS)
        },
        |f, _, _| {
            let s = f.load(V, &sv);
            let u = f.call(V, "jv_bypass_unwrap", &[(V, &s)]);
            f.store(V, &u, &sv);
        },
    );
    let st = f.load(V, &sv);
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "128")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &pre)]);
    f.call("void", "jq_loc", &[("ptr", &sb)]);
    let iserr = f.tag_is(&st, T_ERR);
    f.if_else(
        &iserr,
        |f| {
            let msg = f.call(V, "jv_err_msg", &[(V, &st)]);
            let isstr = f.tag_is(&msg, T_STR);
            f.if_else(
                &isstr,
                |f| {
                    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &mid)]);
                    f.call("void", "sb_putv", &[("ptr", &sb), (V, &msg)]);
                },
                |f| {
                    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &notstr)]);
                    f.call("void", "jv_dump", &[("ptr", &sb), (V, &msg), ("i64", "0"), ("i64", "0")]);
                },
            );
        },
        |f| {
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &mid)]);
            f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &brk)]);
        },
    );
    f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "10")]);
    let d = f.ld_at("ptr", &sb, 0);
    let n = f.ld_at("i64", &sb, 8);
    let e = f.load("ptr", "@stderr");
    f.call("i64", "fwrite", &[("ptr", &d), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
    f.call("void", "free", &[("ptr", &d)]);
    f.call("void", "jv_release", &[(V, &st)]);
    f.ret_void();
    m.add(f);
}

fn main_fn(m: &mut Module) {
    let mut f = Func::new("internal", "i32", "jq_main_real", &[("i32", "argc"), ("ptr", "argv")], "");
    f.call("void", "sb_init", &[("ptr", "@g_out"), ("i64", "65536")]);
    // immortal error raised where the message is discarded anyway (`.[]?` etc.)
    let es = m.cstr("error");
    let ev = f.call(V, "jv_err_c", &[("ptr", &es)]);
    f.call("void", "jv_immortal", &[(V, &ev)]);
    f.store(V, &ev, "@g_err_static");
    let tty = f.call("i32", "isatty", &[("i32", "1")]);
    let istty = f.icmp("ne", "i32", &tty, "0");
    f.store("i1", &istty, "@g_tty");
    let named = f.call(V, "jv_obj", &[]);
    f.store(V, &named, "@g_named");
    let pos = f.call(V, "jv_arr", &[("i64", "4")]);
    f.store(V, &pos, "@g_positional");
    f.call("void", "jq_init", &[]);

    let null_input = f.var_init("i64", "0");
    let exit_status = f.var_init("i64", "0");
    let args_mode = f.var_init("i64", "0"); // 1 = --args, 2 = --jsonargs
    let files = f.var("ptr");
    let argc64 = f.cast("sext", "i32", "%argc", "i64");
    let fbytes = f.bin("mul", "i64", &argc64, "8");
    let fl = f.call("ptr", "jv_malloc", &[("i64", &fbytes)]);
    f.store("ptr", &fl, &files);
    let nfiles = f.var_init("i64", "0");
    let iv = f.var_init("i64", "1");
    let noopts = f.var_init("i1", "false");

    let usage = m.cstr("Usage: <program> [options] [file...]\n\nA jq filter compiled to native code by jqc.\nOptions: -n -r -j -a -c -s -e -S -R --tab --indent n --arg name v --argjson name json\n         --slurpfile name f --rawfile name f --args --jsonargs --raw-output0\n");
    let bad_opt = m.cstr("jq: Unknown option: ");
    let bad_json = m.cstr("jq: Invalid JSON text passed to --argjson\n");

    let setflag = |f: &mut Func, g: &str, val: &str| f.store("i64", val, g);
    let or_flags = |f: &mut Func, bit: &str| {
        let x = f.load("i64", "@g_flags");
        let y = f.bin("or", "i64", &x, bit);
        f.store("i64", &y, "@g_flags");
    };
    let clear_pretty = |f: &mut Func| {
        let x = f.load("i64", "@g_flags");
        let y = f.bin("and", "i64", &x, "-2");
        f.store("i64", &y, "@g_flags");
    };

    // long options with no argument
    let longs: Vec<(&str, String)> = [
        "--null-input",
        "--raw-output",
        "--join-output",
        "--ascii-output",
        "--compact-output",
        "--slurp",
        "--exit-status",
        "--sort-keys",
        "--raw-input",
        "--tab",
        "--color-output",
        "--monochrome-output",
        "--args",
        "--jsonargs",
        "--raw-output0",
        "--help",
        "--seq",
        "--unbuffered",
        "--stream",
        "--version",
    ]
    .iter()
    .map(|s| (*s, m.cstr(s)))
    .collect();
    let s_indent = m.cstr("--indent");
    let s_arg = m.cstr("--arg");
    let s_argjson = m.cstr("--argjson");
    let s_slurpfile = m.cstr("--slurpfile");
    let s_rawfile = m.cstr("--rawfile");

    let version = m.cstr("jq-1.8.1\n");
    let short = |f: &mut Func, c: &str, m_usage: &str, m_version: &str| {
        // handle one short option char; returns nothing
        let opts: [(u8, &str); 14] = [
            (b'n', "n"),
            (b'r', "r"),
            (b'j', "j"),
            (b'a', "a"),
            (b'c', "c"),
            (b's', "s"),
            (b'e', "e"),
            (b'S', "S"),
            (b'R', "R"),
            (b'C', "C"),
            (b'M', "M"),
            (b'h', "h"),
            (b'0', "0"),
            (b'V', "V"),
        ];
        for (ch, name) in opts {
            let is = f.icmp("eq", "i8", c, &(ch as i64).to_string());
            f.if_then(&is, |f| {
                match name {
                    "n" => setflag(f, &null_input, "1"),
                    "r" => setflag(f, "@g_raw", "1"),
                    "j" => {
                        setflag(f, "@g_raw", "1");
                        setflag(f, "@g_join", "1");
                    }
                    "a" => or_flags(f, "2"),
                    "c" => clear_pretty(f),
                    "s" => setflag(f, "@g_slurp", "1"),
                    "e" => setflag(f, "@g_eflag", "1"),
                    "S" => or_flags(f, "4"),
                    "R" => setflag(f, "@g_rawin", "1"),
                    "C" => setflag(f, "@g_color", "1"),
                    "M" => setflag(f, "@g_color", "0"),
                    "V" => {
                        let so = f.load("ptr", "@stdout");
                        f.call("i32", "fputs", &[("ptr", m_version), ("ptr", &so)]);
                        f.call("void", "exit", &[("i32", "0")]);
                        f.unreachable();
                    }
                    "h" => {
                        let so = f.load("ptr", "@stdout");
                        let n = f.call("i64", "strlen", &[("ptr", m_usage)]);
                        f.call("i64", "fwrite", &[("ptr", m_usage), ("i64", "1"), ("i64", &n), ("ptr", &so)]);
                        f.call("void", "exit", &[("i32", "0")]);
                        f.unreachable();
                    }
                    _ => {}
                }
            });
        }
    };

    f.while_(
        |f| {
            let i = f.load("i64", &iv);
            f.lt(&i, &argc64)
        },
        |f, _, cont| {
            let i = f.load("i64", &iv);
            let ap = f.idx("%argv", &i, 8);
            let a = f.load("ptr", &ap);
            let i1 = f.add(&i, "1");
            f.store("i64", &i1, &iv);
            // "--" ends option processing
            let dd = m.cstr("--");
            let isdd = f.call("i32", "strcmp", &[("ptr", &a), ("ptr", &dd)]);
            let isdd = f.icmp("eq", "i32", &isdd, "0");
            let nop = f.load("i1", &noopts);
            let nnop = f.not1(&nop);
            let first_dd = f.and1(&isdd, &nnop);
            f.if_then(&first_dd, |f| {
                f.store("i1", "true", &noopts);
                f.br(cont);
            });
            let b0 = f.load("i8", &a);
            let a1 = f.off(&a, "1");
            let b1 = f.load("i8", &a1);
            let dash = f.icmp("eq", "i8", &b0, "45");
            let nnop2 = f.not1(&nop);
            let dash = f.and1(&dash, &nnop2);
            let am = f.load("i64", &args_mode);
            // positional (non-option) arguments
            let notdash = f.not1(&dash);
            let single = f.icmp("eq", "i8", &b1, "0");
            let plain = f.or1(&notdash, &single);
            f.if_then(&plain, |f| {
                let isargs = f.ne(&am, "0");
                f.if_then(&isargs, |f| {
                    let s = f.call(V, "jv_cstr", &[("ptr", &a)]);
                    let isjson = f.eq(&am, "2");
                    let v = f.if_val(
                        &isjson,
                        V,
                        |f| {
                            let v = f.call(V, "jv_fromjson", &[(V, &s)]);
                            f.call("void", "jv_release", &[(V, &s)]);
                            let bad = f.tag_is(&v, T_ERR);
                            f.if_then(&bad, |f| {
                                let e = f.load("ptr", "@stderr");
                                let n = f.call("i64", "strlen", &[("ptr", &bad_json)]);
                                f.call("i64", "fwrite", &[("ptr", &bad_json), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
                                f.call("void", "exit", &[("i32", "2")]);
                                f.unreachable();
                            });
                            v
                        },
                        |_| s.clone(),
                    );
                    let p = f.load(V, "@g_positional");
                    let p2 = f.call(V, "jv_arr_push", &[(V, &p), (V, &v)]);
                    f.call("void", "jv_release", &[(V, &v)]);
                    f.store(V, &p2, "@g_positional");
                    f.br(cont);
                });
                let fl = f.load("ptr", &files);
                let n = f.load("i64", &nfiles);
                let fp = f.idx(&fl, &n, 8);
                f.store("ptr", &a, &fp);
                let n1 = f.add(&n, "1");
                f.store("i64", &n1, &nfiles);
                f.br(cont);
            });
            let islong = f.icmp("eq", "i8", &b1, "45");
            f.if_then(&islong, |f| {
                for (name, g) in &longs {
                    let c = f.call("i32", "strcmp", &[("ptr", &a), ("ptr", g)]);
                    let is = f.icmp("eq", "i32", &c, "0");
                    f.if_then(&is, |f| {
                        match *name {
                            "--null-input" => setflag(f, &null_input, "1"),
                            "--raw-output" => setflag(f, "@g_raw", "1"),
                            "--join-output" => {
                                setflag(f, "@g_raw", "1");
                                setflag(f, "@g_join", "1");
                            }
                            "--raw-output0" => {
                                setflag(f, "@g_raw", "1");
                                setflag(f, "@g_nul", "1");
                            }
                            "--ascii-output" => or_flags(f, "2"),
                            "--compact-output" => clear_pretty(f),
                            "--slurp" => setflag(f, "@g_slurp", "1"),
                            "--exit-status" => setflag(f, "@g_eflag", "1"),
                            "--sort-keys" => or_flags(f, "4"),
                            "--raw-input" => setflag(f, "@g_rawin", "1"),
                            "--color-output" => setflag(f, "@g_color", "1"),
                            "--stream" => setflag(f, "@g_stream", "1"),
                            "--seq" => setflag(f, "@g_seq", "1"),
                            "--unbuffered" => setflag(f, "@g_unbuffered", "1"),
                            "--version" => {
                                let so = f.load("ptr", "@stdout");
                                let v = m.cstr("jq-1.8.1\n");
                                f.call("i32", "fputs", &[("ptr", &v), ("ptr", &so)]);
                                f.call("void", "exit", &[("i32", "0")]);
                                f.unreachable();
                            }
                            "--monochrome-output" => setflag(f, "@g_color", "0"),
                            "--tab" => or_flags(f, "8"),
                            "--args" => setflag(f, &args_mode, "1"),
                            "--jsonargs" => setflag(f, &args_mode, "2"),
                            "--help" => {
                                let so = f.load("ptr", "@stdout");
                                let n = f.call("i64", "strlen", &[("ptr", &usage)]);
                                f.call("i64", "fwrite", &[("ptr", &usage), ("i64", "1"), ("i64", &n), ("ptr", &so)]);
                                f.call("void", "exit", &[("i32", "0")]);
                                f.unreachable();
                            }
                            _ => {}
                        }
                        f.br(cont);
                    });
                }
                // options with arguments
                let argn = |f: &mut Func, k: i64| -> String {
                    let i = f.load("i64", &iv);
                    let j = f.add(&i, &(k - 1).to_string());
                    let missing = f.icmp("sge", "i64", &j, &argc64);
                    f.if_then(&missing, |f| {
                        let e = f.load("ptr", "@stderr");
                        let n = f.call("i64", "strlen", &[("ptr", &usage)]);
                        f.call("i64", "fwrite", &[("ptr", &usage), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
                        f.call("void", "exit", &[("i32", "2")]);
                        f.unreachable();
                    });
                    let p = f.idx("%argv", &j, 8);
                    f.load("ptr", &p)
                };
                let c = f.call("i32", "strcmp", &[("ptr", &a), ("ptr", &s_indent)]);
                let is = f.icmp("eq", "i32", &c, "0");
                f.if_then(&is, |f| {
                    let v = argn(f, 1);
                    let d = f.call("double", "strtod", &[("ptr", &v), ("ptr", "null")]);
                    let n = f.cast("fptosi", "double", &d, "i64");
                    let toobig = f.icmp("sgt", "i64", &n, "7");
                    let toosmall = f.icmp("slt", "i64", &n, "-1");
                    let bad = f.or1(&toobig, &toosmall);
                    f.if_then(&bad, |f| {
                        let e = f.load("ptr", "@stderr");
                        let msg = m.cstr("jq: --indent takes a number between -1 and 7\nUse jq --help for help with command-line options,\nor see the jq manpage, or online docs  at https://jqlang.org\n");
                        f.call("i32", "fputs", &[("ptr", &msg), ("ptr", &e)]);
                        f.call("void", "exit", &[("i32", "2")]);
                        f.unreachable();
                    });
                    f.store("i64", &n, "@g_indent");
                    let istab = f.eq(&n, "-1");
                    f.if_then(&istab, |f| or_flags(f, "8"));
                    let i = f.load("i64", &iv);
                    let i2 = f.add(&i, "1");
                    f.store("i64", &i2, &iv);
                    f.br(cont);
                });
                for (opt, kind) in [(&s_arg, 0), (&s_argjson, 1), (&s_slurpfile, 2), (&s_rawfile, 3)] {
                    let c = f.call("i32", "strcmp", &[("ptr", &a), ("ptr", opt)]);
                    let is = f.icmp("eq", "i32", &c, "0");
                    f.if_then(&is, |f| {
                        let name = argn(f, 1);
                        let val = argn(f, 2);
                        let k = f.call(V, "jv_cstr", &[("ptr", &name)]);
                        let v = match kind {
                            0 => f.call(V, "jv_cstr", &[("ptr", &val)]),
                            1 => {
                                let s = f.call(V, "jv_cstr", &[("ptr", &val)]);
                                let v = f.call(V, "jv_fromjson", &[(V, &s)]);
                                f.call("void", "jv_release", &[(V, &s)]);
                                let bad = f.tag_is(&v, T_ERR);
                                f.if_then(&bad, |f| {
                                    let e = f.load("ptr", "@stderr");
                                    let n = f.call("i64", "strlen", &[("ptr", &bad_json)]);
                                    f.call("i64", "fwrite", &[("ptr", &bad_json), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
                                    f.call("void", "exit", &[("i32", "2")]);
                                    f.unreachable();
                                });
                                v
                            }
                            _ => {
                                let sb = f.array_var("i8", 24);
                                f.call("void", "sb_init", &[("ptr", &sb), ("i64", "4096")]);
                                let ok = f.call("i1", "jq_read_file", &[("ptr", &sb), ("ptr", &val)]);
                                let bad = f.not1(&ok);
                                f.if_then(&bad, |f| {
                                    f.call("void", "exit", &[("i32", "2")]);
                                    f.unreachable();
                                });
                                let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
                                if kind == 3 {
                                    s
                                } else {
                                    // parse all values into an array
                                    let arr = f.call(V, "jv_arr", &[("i64", "4")]);
                                    let av = f.var_init(V, &arr);
                                    let pc = f.var("ptr");
                                    let d = f.call("ptr", "jv_str_ptr", &[(V, &s)]);
                                    f.store("ptr", &d, &pc);
                                    f.while_(
                                        |_| "true".into(),
                                        |f, brk, _| {
                                            f.call("void", "jp_ws", &[("ptr", &pc)]);
                                            let c = f.load("ptr", &pc);
                                            let b = f.load("i8", &c);
                                            let z = f.icmp("eq", "i8", &b, "0");
                                            f.if_then(&z, |f| f.br(brk));
                                            let v = f.call(V, "jp_value", &[("ptr", &pc), ("i64", "0")]);
                                            let bad = f.tag_is(&v, T_ERR);
                                            f.if_then(&bad, |f| {
                                                f.call("void", "jq_parse_report", &[(V, &v)]);
                                                f.call("void", "exit", &[("i32", "2")]);
                                                f.unreachable();
                                            });
                                            let a = f.load(V, &av);
                                            let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &v)]);
                                            f.call("void", "jv_release", &[(V, &v)]);
                                            f.store(V, &a2, &av);
                                        },
                                    );
                                    f.call("void", "jv_release", &[(V, &s)]);
                                    f.load(V, &av)
                                }
                            }
                        };
                        let o = f.load(V, "@g_named");
                        let o2 = f.call(V, "jv_obj_set", &[(V, &o), (V, &k), (V, &v)]);
                        f.store(V, &o2, "@g_named");
                        f.call("void", "jv_release", &[(V, &k)]);
                        f.call("void", "jv_release", &[(V, &v)]);
                        let i = f.load("i64", &iv);
                        let i2 = f.add(&i, "2");
                        f.store("i64", &i2, &iv);
                        f.br(cont);
                    });
                }
                let e = f.load("ptr", "@stderr");
                let n = f.call("i64", "strlen", &[("ptr", &bad_opt)]);
                f.call("i64", "fwrite", &[("ptr", &bad_opt), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
                let n = f.call("i64", "strlen", &[("ptr", &a)]);
                f.call("i64", "fwrite", &[("ptr", &a), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
                let nl = m.cstr("\n");
                f.call("i64", "fwrite", &[("ptr", &nl), ("i64", "1"), ("i64", "1"), ("ptr", &e)]);
                f.call("void", "exit", &[("i32", "2")]);
                f.unreachable();
            });
            // short options, possibly combined
            let n = f.call("i64", "strlen", &[("ptr", &a)]);
            f.for_range("1", &n, |f, j, _, _| {
                let p = f.off(&a, j);
                let c = f.load("i8", &p);
                short(f, &c, &usage, &version);
            });
        },
    );

    // colour: -C forces, -M disables, otherwise only on a terminal without NO_COLOR
    let col = f.load("i64", "@g_color");
    let auto = f.eq(&col, "-1");
    f.if_then(&auto, |f| {
        let tty = f.load("i1", "@g_tty");
        let nc = m.cstr("NO_COLOR");
        let v = f.call("ptr", "getenv", &[("ptr", &nc)]);
        let unset = f.icmp("eq", "ptr", &v, "null");
        let empty = f.if_val(&unset, "i1", |_| "true".into(), |f| {
            let b = f.load("i8", &v);
            f.icmp("eq", "i8", &b, "0")
        });
        let on = f.and1(&tty, &empty);
        let x = f.cast("zext", "i1", &on, "i64");
        f.store("i64", &x, "@g_color");
    });
    let col = f.load("i64", "@g_color");
    let on = f.eq(&col, "1");
    f.if_then(&on, |f| {
        f.call("void", "jq_load_colors", &[]);
        let fl = f.load("i64", "@g_flags");
        let fl2 = f.bin("or", "i64", &fl, "16");
        f.store("i64", &fl2, "@g_flags");
    });

    // input is read incrementally (see rt::input)
    let fl = f.load("ptr", &files);
    let nf = f.load("i64", &nfiles);
    f.call("void", "jq_input_init", &[("ptr", &fl), ("i64", &nf)]);
    let ni = f.load("i64", &null_input);
    let isnull = f.ne(&ni, "0");

    // slurp
    let sl = f.load("i64", "@g_slurp");
    let iss = f.ne(&sl, "0");
    f.if_then(&iss, |f| {
        f.store("i64", "0", "@g_slurp");
        let raw = f.load("i64", "@g_rawin");
        let israw = f.ne(&raw, "0");
        f.if_else(
            &israw,
            |f| {
                let s = f.call(V, "jq_read_rest", &[]);
                f.store(V, &s, "@g_slurped");
            },
            |f| {
                let arr = f.call(V, "jv_arr", &[("i64", "16")]);
                let av = f.var_init(V, &arr);
                f.while_(
                    |_| "true".into(),
                    |f, brk, _| {
                        let v = f.call(V, "jv_next_input", &[]);
                        let eof = f.tag_is(&v, T_EOF);
                        f.if_then(&eof, |f| f.br(brk));
                        let bad = f.tag_is(&v, T_ERR);
                        f.if_then(&bad, |f| {
                            f.call("void", "jq_parse_report", &[(V, &v)]);
                            f.call("void", "exit", &[("i32", "2")]);
                            f.unreachable();
                        });
                        let a = f.load(V, &av);
                        let a2 = f.call(V, "jv_arr_push", &[(V, &a), (V, &v)]);
                        f.call("void", "jv_release", &[(V, &v)]);
                        f.store(V, &a2, &av);
                    },
                );
                let a = f.load(V, &av);
                f.store(V, &a, "@g_slurped");
            },
        );
        f.store("i64", "1", "@g_slurp");
        f.store("i64", "0", "@g_any_input");
    });

    // run
    let run = |f: &mut Func, input: &str| {
        let st = f.call(V, "jq_program", &[("ptr", "null"), (V, input), ("ptr", "@jq_out"), ("ptr", "null")]);
        let t = f.tag(&st);
        let abort = f.icmp("uge", "i64", &t, "7");
        // like jq, the exit status reflects the last input processed
        let s = f.select(&abort, "i32", "5", "0");
        f.store("i32", &s, "@g_status");
        f.if_then(&abort, |f| {
            f.call("void", "jq_report", &[(V, &st)]);
            f.call("void", "jv_release", &[(V, &st)]);
        });
    };
    let _ = exit_status;
    let prev_in = f.var_init(V, NULL);
    f.if_else(
        &isnull,
        |f| run(f, NULL),
        |f| {
            f.while_(
                |_| "true".into(),
                |f, brk, _| {
                    let v = f.call(V, "jv_next_input", &[]);
                    let eof = f.tag_is(&v, T_EOF);
                    f.if_then(&eof, |f| f.br(brk));
                    let bad = f.tag_is(&v, T_ERR);
                    f.if_then(&bad, |f| {
                        f.call("void", "jq_parse_report", &[(V, &v)]);
                        f.store("i32", "5", "@g_ret");
                        f.br(brk);
                    });
                    run(f, &v);
                    // release the previous input lazily: the last one is never freed
                    // (the process exits right after), which saves a full teardown.
                    let prev = f.load(V, &prev_in);
                    f.call("void", "jv_release", &[(V, &prev)]);
                    f.store(V, &v, &prev_in);
                },
            );
        },
    );
    f.call("void", "jq_flush", &[]);
    let r1 = f.load("i32", "@g_ret");
    let st = f.load("i32", "@g_status");
    let r1z = f.icmp("eq", "i32", &r1, "0");
    let r = f.select(&r1z, "i32", &st, &r1);
    let ef = f.load("i64", "@g_eflag");
    let ise = f.ne(&ef, "0");
    let ok = f.icmp("eq", "i32", &r, "0");
    let both = f.and1(&ise, &ok);
    f.if_then(&both, |f| {
        let last = f.load("i64", "@g_last");
        let none = f.eq(&last, "0");
        f.if_then(&none, |f| f.ret("i32", "4"));
        let falsy = f.eq(&last, "1");
        f.if_then(&falsy, |f| f.ret("i32", "1"));
    });
    f.ret("i32", &r);
    m.add(f);
}

/// `main` runs the program on a thread with a very large (lazily committed)
/// stack: continuation-passing code nests calls deeply for recursive filters.
pub fn thread_main(m: &mut Module) {
    m.declare("pthread_attr_init", "declare i32 @pthread_attr_init(ptr)");
    m.declare("pthread_attr_setstacksize", "declare i32 @pthread_attr_setstacksize(ptr, i64)");
    m.declare("pthread_create", "declare i32 @pthread_create(ptr, ptr, ptr, ptr)");
    m.declare("pthread_join", "declare i32 @pthread_join(i64, ptr)");

    let mut f = Func::new("internal", "ptr", "jq_thread", &[("ptr", "a")], "");
    let argc = f.ld_at("i32", "%a", 0);
    let argv = f.ld_at("ptr", "%a", 8);
    let r = f.call("i32", "jq_main_real", &[("i32", &argc), ("ptr", &argv)]);
    f.st_at("i32", &r, "%a", 16);
    f.ret("ptr", "null");
    m.add(f);

    let mut f = Func::new("", "i32", "main", &[("i32", "argc"), ("ptr", "argv")], "");
    f.call("void", "jq_tune_malloc", &[("ptr", "%argv")]);
    let args = f.array_var("i64", 3);
    f.st_at("i32", "%argc", &args, 0);
    f.st_at("ptr", "%argv", &args, 8);
    let attr = f.array_var("i64", 16);
    let tid = f.var("i64");
    f.call("i32", "pthread_attr_init", &[("ptr", &attr)]);
    f.call("i32", "pthread_attr_setstacksize", &[("ptr", &attr), ("i64", "1073741824")]);
    let rc = f.call("i32", "pthread_create", &[("ptr", &tid), ("ptr", &attr), ("ptr", "@jq_thread"), ("ptr", &args)]);
    let failed = f.icmp("ne", "i32", &rc, "0");
    f.if_then(&failed, |f| {
        let r = f.call("i32", "jq_main_real", &[("i32", "%argc"), ("ptr", "%argv")]);
        f.ret("i32", &r);
    });
    let t = f.load("i64", &tid);
    f.call("i32", "pthread_join", &[("i64", &t), ("ptr", "null")]);
    let r = f.ld_at("i32", &args, 16);
    f.ret("i32", &r);
    m.add(f);
}

/// Re-exec once with glibc malloc tunables that back the heap with
/// transparent huge pages: for large inputs this removes most page-fault cost.
pub fn tune_malloc(m: &mut Module) {
    m.declare("setenv", "declare i32 @setenv(ptr, ptr, i32)");
    m.declare("unsetenv", "declare i32 @unsetenv(ptr)");
    m.declare("execv", "declare i32 @execv(ptr, ptr)");
    let marker = m.cstr("__JQC_TUNED");
    let tun = m.cstr("GLIBC_TUNABLES");
    let val = m.cstr("glibc.malloc.hugetlb=1:glibc.malloc.top_pad=67108864");
    let one = m.cstr("1");
    let exe = m.cstr("/proc/self/exe");
    let notune = m.cstr("JQC_NO_TUNE");
    let mut f = Func::new("internal", "void", "jq_tune_malloc", &[("ptr", "argv")], "");
    let done = f.call("ptr", "getenv", &[("ptr", &marker)]);
    let isdone = f.icmp("ne", "ptr", &done, "null");
    f.if_then(&isdone, |f| {
        // we are the re-executed process: hide our variables from $ENV
        f.call("i32", "unsetenv", &[("ptr", &tun)]);
        f.call("i32", "unsetenv", &[("ptr", &marker)]);
        f.ret_void();
    });
    let opt = f.call("ptr", "getenv", &[("ptr", &notune)]);
    let has_opt = f.icmp("ne", "ptr", &opt, "null");
    let user = f.call("ptr", "getenv", &[("ptr", &tun)]);
    let has_user = f.icmp("ne", "ptr", &user, "null");
    let skip = f.or1(&has_opt, &has_user);
    f.if_then(&skip, |f| f.ret_void());
    f.call("i32", "setenv", &[("ptr", &tun), ("ptr", &val), ("i32", "1")]);
    f.call("i32", "setenv", &[("ptr", &marker), ("ptr", &one), ("i32", "1")]);
    f.call("i32", "execv", &[("ptr", &exe), ("ptr", "%argv")]);
    // exec failed: carry on untuned
    f.call("i32", "unsetenv", &[("ptr", &tun)]);
    f.call("i32", "unsetenv", &[("ptr", &marker)]);
    f.ret_void();
    m.add(f);
}

/// "jq: parse error: <msg> at line L, column C" for invalid JSON input.
pub fn parse_report(m: &mut Module) {
    let pre = m.cstr("jq: parse error: ");
    let mut f = Func::new("internal", "void", "jq_parse_report", &[(V, "e")], "");
    // position relative to the current file, across buffer compactions
    let pos = f.load("ptr", "@g_perr_pos");
    let fs = f.load("ptr", "@g_fstart");
    let buf = f.load("ptr", "@g_buf");
    let fnn = f.icmp("ne", "ptr", &fs, "null");
    let le = f.icmp("ule", "ptr", &fs, &pos);
    let infile = f.and1(&fnn, &le);
    let base = f.select(&infile, "ptr", &fs, &buf);
    let fl = f.load("i64", "@g_flines");
    let fc = f.load("i64", "@g_fcol");
    let line0 = f.select(&infile, "i64", "0", &fl);
    let col0 = f.select(&infile, "i64", "0", &fc);
    let msg = f.call(V, "jv_parse_errmsg2", &[(V, "%e"), ("ptr", &base), ("i64", &line0), ("i64", &col0)]);
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "128")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &pre)]);
    f.call("void", "sb_putv", &[("ptr", &sb), (V, &msg)]);
    f.call("void", "sb_putc", &[("ptr", &sb), ("i8", "10")]);
    f.call("void", "jv_release", &[(V, &msg)]);
    let d = f.ld_at("ptr", &sb, 0);
    let n = f.ld_at("i64", &sb, 8);
    let e = f.load("ptr", "@stderr");
    f.call("i64", "fwrite", &[("ptr", &d), ("i64", "1"), ("i64", &n), ("ptr", &e)]);
    f.call("void", "free", &[("ptr", &d)]);
    f.ret_void();
    m.add(f);
}

/// --stream: each parsed value is replaced by its `tostream` events.
pub fn stream(m: &mut Module) {
    // append the stream events of v (at `path`, borrowed) to the array in *out
    let mut f = Func::new("internal", "void", "jv_stream_into", &[("ptr", "out"), (V, "x"), (V, "path")], "");
    let t = f.tag("%x");
    let isarr = f.eq(&t, "5");
    let isobj = f.eq(&t, "6");
    let cont = f.or1(&isarr, &isobj);
    let n = f.if_val(&cont, "i64", |f| {
        f.if_val(&isarr, "i64", |f| f.call("i64", "jv_arr_len", &[(V, "%x")]), |f| f.call("i64", "jv_obj_len", &[(V, "%x")]))
    }, |_| "0".into());
    let nonempty = f.icmp("sgt", "i64", &n, "0");
    f.if_else(
        &nonempty,
        |f| {
            let lastp = f.var_init(V, NULL);
            f.for_range("0", &n, |f, i, _, _| {
                let k = f.if_val(&isarr, V, |f| {
                    let d = f.cast("sitofp", "i64", i, "double");
                    f.mknum(&d)
                }, |f| f.call(V, "jv_obj_key_at", &[(V, "%x"), ("i64", i)]));
                let child = f.if_val(&isarr, V, |f| f.call(V, "jv_arr_get", &[(V, "%x"), ("i64", i)]), |f| f.call(V, "jv_obj_val_at", &[(V, "%x"), ("i64", i)]));
                f.call("void", "jv_retain", &[(V, "%path")]);
                let p2 = f.call(V, "jv_arr_push", &[(V, "%path"), (V, &k)]);
                f.call("void", "jv_stream_into", &[("ptr", "%out"), (V, &child), (V, &p2)]);
                let old = f.load(V, &lastp);
                f.call("void", "jv_release", &[(V, &old)]);
                f.store(V, &p2, &lastp);
            });
            let lp = f.load(V, &lastp);
            let ev = f.call(V, "jv_arr", &[("i64", "1")]);
            let ev = f.call(V, "jv_arr_push", &[(V, &ev), (V, &lp)]);
            f.call("void", "jv_release", &[(V, &lp)]);
            let o = f.load(V, "%out");
            let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &ev)]);
            f.call("void", "jv_release", &[(V, &ev)]);
            f.store(V, &o2, "%out");
        },
        |f| {
            let ev = f.call(V, "jv_arr", &[("i64", "2")]);
            let ev = f.call(V, "jv_arr_push", &[(V, &ev), (V, "%path")]);
            let ev = f.call(V, "jv_arr_push", &[(V, &ev), (V, "%x")]);
            let o = f.load(V, "%out");
            let o2 = f.call(V, "jv_arr_push", &[(V, &o), (V, &ev)]);
            f.call("void", "jv_release", &[(V, &ev)]);
            f.store(V, &o2, "%out");
        },
    );
    f.ret_void();
    m.add(f);
}

/// Next input, honouring --stream (events of each value, one at a time).
pub fn next_input_stream(m: &mut Module) {
    let mut f = Func::new("internal", V, "jv_next_input", &[], "");
    let st = f.load("i64", "@g_stream");
    let sl = f.load("i64", "@g_slurp");
    let on = f.ne(&st, "0");
    let notslurped = f.eq(&sl, "0");
    let streaming = f.and1(&on, &notslurped);
    let plain = f.not1(&streaming);
    f.if_then(&plain, |f| {
        let v = f.call(V, "jv_next_input_raw", &[]);
        f.ret(V, &v);
    });
    let q = f.load(V, "@g_sq");
    let have = f.tag_is(&q, T_ARR);
    let pending = f.if_val(&have, "i1", |f| {
        let i = f.load("i64", "@g_sqi");
        let n = f.call("i64", "jv_arr_len", &[(V, &q)]);
        f.lt(&i, &n)
    }, |_| "false".into());
    let empty = f.not1(&pending);
    f.if_then(&empty, |f| {
        let v = f.call(V, "jv_next_input_raw", &[]);
        let t = f.tag(&v);
        let special = f.icmp("uge", "i64", &t, "7");
        f.if_then(&special, |f| f.ret(V, &v));
        let out = f.var(V);
        let a = f.call(V, "jv_arr", &[("i64", "8")]);
        f.store(V, &a, &out);
        let root = f.call(V, "jv_arr", &[("i64", "0")]);
        f.call("void", "jv_stream_into", &[("ptr", &out), (V, &v), (V, &root)]);
        f.call("void", "jv_release", &[(V, &root)]);
        f.call("void", "jv_release", &[(V, &v)]);
        let old = f.load(V, "@g_sq");
        f.call("void", "jv_release", &[(V, &old)]);
        let ev = f.load(V, &out);
        f.store(V, &ev, "@g_sq");
        f.store("i64", "0", "@g_sqi");
    });
    let q = f.load(V, "@g_sq");
    let i = f.load("i64", "@g_sqi");
    let e = f.call(V, "jv_arr_get", &[(V, &q), ("i64", &i)]);
    f.call("void", "jv_retain", &[(V, &e)]);
    let i1 = f.add(&i, "1");
    f.store("i64", &i1, "@g_sqi");
    f.ret(V, &e);
    m.add(f);
}
