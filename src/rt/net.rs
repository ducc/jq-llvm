//! Raw socket builtins (jq-llvm extension). File descriptors are plain
//! numbers; data is passed as strings of raw bytes. Protocols (HTTP etc.) are
//! meant to be written in jq on top of these.
//!
//!   socket_connect($host; $port)  -> fd     TCP connect (IPv4/IPv6 via getaddrinfo)
//!   socket_listen($host; $port)   -> fd     bind + listen (host null = any)
//!   socket_accept($fd)            -> fd     accept a connection
//!   socket_send($fd; $data)       -> n      send all of a string
//!   socket_recv($fd; $max)        -> str    up to $max bytes, "" at end of stream
//!   socket_shutdown($fd)          -> null   close the sending side
//!   socket_close($fd)             -> null

use super::*;
use crate::ir::{Func, Module, V};

pub const NATIVES: &[(&str, usize, &str)] = &[
    ("socket_connect", 2, "jvb_socket_connect"),
    ("socket_listen", 2, "jvb_socket_listen"),
    ("socket_accept", 1, "jvb_socket_accept"),
    ("socket_send", 2, "jvb_socket_send"),
    ("socket_recv", 2, "jvb_socket_recv"),
    ("socket_shutdown", 1, "jvb_socket_shutdown"),
    ("socket_close", 1, "jvb_socket_close"),
];

pub fn emit(m: &mut Module) {
    for (n, d) in [
        ("getaddrinfo", "declare i32 @getaddrinfo(ptr, ptr, ptr, ptr)"),
        ("freeaddrinfo", "declare void @freeaddrinfo(ptr)"),
        ("gai_strerror", "declare ptr @gai_strerror(i32)"),
        ("socket", "declare i32 @socket(i32, i32, i32)"),
        ("connect", "declare i32 @connect(i32, ptr, i32)"),
        ("bind", "declare i32 @bind(i32, ptr, i32)"),
        ("listen", "declare i32 @listen(i32, i32)"),
        ("accept", "declare i32 @accept(i32, ptr, ptr)"),
        ("setsockopt", "declare i32 @setsockopt(i32, i32, i32, ptr, i32)"),
        ("send", "declare i64 @send(i32, ptr, i64, i32)"),
        ("recv", "declare i64 @recv(i32, ptr, i64, i32)"),
        ("shutdown", "declare i32 @shutdown(i32, i32)"),
        ("close", "declare i32 @close(i32)"),
    ] {
        m.declare(n, d);
    }
    errors(m);
    resolve(m);
    ops(m);
}

/// "<what>: <strerror(errno)>" as an error value.
fn errors(m: &mut Module) {
    let sep = m.cstr(": ");
    let mut f = Func::new("internal", V, "jv_errno_err", &[("ptr", "what")], "noinline");
    let ep = f.call("ptr", "__errno_location", &[]);
    let errno = f.load("i32", &ep);
    let msg = f.call("ptr", "strerror", &[("i32", &errno)]);
    let sb = f.array_var("i8", 24);
    f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", "%what")]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &sep)]);
    f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &msg)]);
    let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
    let e = f.call(V, "jv_err_take", &[(V, &s)]);
    f.ret(V, &e);
    m.add(f);

    // fd argument: a non-negative integer
    let bad = m.cstr("socket file descriptor must be a non-negative integer");
    let mut f = Func::new("internal", "i32", "jv_fd_arg", &[(V, "x")], "");
    let isn = f.tag_is("%x", T_NUM);
    let notn = f.not1(&isn);
    f.if_then(&notn, |f| f.ret("i32", "-1"));
    let d = f.num_of("%x");
    let neg = f.fcmp("olt", &d, "0.0");
    f.if_then(&neg, |f| f.ret("i32", "-1"));
    let i = f.cast("fptosi", "double", &d, "i32");
    f.ret("i32", &i);
    m.add(f);
    let mut f = Func::new("internal", V, "jv_fd_err", &[], "noinline");
    let e = f.call(V, "jv_err_c", &[("ptr", &bad)]);
    f.ret(V, &e);
    m.add(f);
}

/// Connect or bind to host:port, trying every address getaddrinfo returns.
/// `listen` = 1: bind+listen (host may be null for any address).
fn resolve(m: &mut Module) {
    let what_c = m.cstr("socket_connect");
    let what_l = m.cstr("socket_listen");
    let badargs = m.cstr("socket_connect/socket_listen: host must be a string (or null to listen on any address) and port a number or string");
    let pfmt = m.cstr("%d");
    let resolve_pre = m.cstr("cannot resolve host: ");
    let mut f = Func::new("internal", V, "jv_sock_open", &[(V, "host"), (V, "port"), ("i1", "listen")], "");
    // port: number or string
    let ps = f.tag_is("%port", T_STR);
    let pn = f.tag_is("%port", T_NUM);
    let hs = f.tag_is("%host", T_STR);
    let hn = f.tag_is("%host", T_NULL);
    let hostok = {
        let ln = f.and1(&hn, "%listen");
        f.or1(&hs, &ln)
    };
    let portok = f.or1(&ps, &pn);
    let ok = f.and1(&hostok, &portok);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| {
        let e = f.call(V, "jv_err_c", &[("ptr", &badargs)]);
        f.ret(V, &e);
    });
    let pbuf = f.array_var("i8", 32);
    let service = f.if_val(&ps, "ptr", |f| f.call("ptr", "jv_str_ptr", &[(V, "%port")]), |f| {
        let d = f.num_of("%port");
        let i = f.cast("fptosi", "double", &d, "i32");
        f.call("i32 (ptr, i64, ptr, ...)", "snprintf", &[("ptr", &pbuf), ("i64", "32"), ("ptr", &pfmt), ("i32", &i)]);
        pbuf.clone()
    });
    let node = f.if_val(&hs, "ptr", |f| f.call("ptr", "jv_str_ptr", &[(V, "%host")]), |_| "null".into());
    // struct addrinfo hints: flags@0 family@4 socktype@8 protocol@12
    let hints = f.array_var("i64", 6);
    f.call("ptr", "memset", &[("ptr", &hints), ("i32", "0"), ("i64", "48")]);
    let flags = f.select("%listen", "i32", "1", "0"); // AI_PASSIVE
    f.st_at("i32", &flags, &hints, 0);
    f.st_at("i32", "0", &hints, 4); // AF_UNSPEC
    f.st_at("i32", "1", &hints, 8); // SOCK_STREAM
    let res = f.var_init("ptr", "null");
    let rc = f.call("i32", "getaddrinfo", &[("ptr", &node), ("ptr", &service), ("ptr", &hints), ("ptr", &res)]);
    let failed = f.icmp("ne", "i32", &rc, "0");
    f.if_then(&failed, |f| {
        let msg = f.call("ptr", "gai_strerror", &[("i32", &rc)]);
        let sb = f.array_var("i8", 24);
        f.call("void", "sb_init", &[("ptr", &sb), ("i64", "64")]);
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &resolve_pre)]);
        f.call("void", "sb_puts", &[("ptr", &sb), ("ptr", &msg)]);
        let s = f.call(V, "sb_finish", &[("ptr", &sb)]);
        let e = f.call(V, "jv_err_take", &[(V, &s)]);
        f.ret(V, &e);
    });
    let fdv = f.var_init("i32", "-1");
    let ai = f.var("ptr");
    let first = f.load("ptr", &res);
    f.store("ptr", &first, &ai);
    f.while_(
        |f| {
            let a = f.load("ptr", &ai);
            f.icmp("ne", "ptr", &a, "null")
        },
        |f, brk, cont| {
            let a = f.load("ptr", &ai);
            let next = f.ld_at("ptr", &a, 40);
            f.store("ptr", &next, &ai);
            let fam = f.ld_at("i32", &a, 4);
            let st = f.ld_at("i32", &a, 8);
            let pr = f.ld_at("i32", &a, 12);
            let alen = f.ld_at("i32", &a, 16);
            let addr = f.ld_at("ptr", &a, 24);
            let fd = f.call("i32", "socket", &[("i32", &fam), ("i32", &st), ("i32", &pr)]);
            let bad = f.icmp("slt", "i32", &fd, "0");
            f.if_then(&bad, |f| f.br(cont));
            let rc = f.if_val(
                "%listen",
                "i32",
                |f| {
                    let one = f.var_init("i32", "1");
                    // SOL_SOCKET=1, SO_REUSEADDR=2
                    f.call("i32", "setsockopt", &[("i32", &fd), ("i32", "1"), ("i32", "2"), ("ptr", &one), ("i32", "4")]);
                    let b = f.call("i32", "bind", &[("i32", &fd), ("ptr", &addr), ("i32", &alen)]);
                    let ok = f.icmp("eq", "i32", &b, "0");
                    f.if_val(&ok, "i32", |f| f.call("i32", "listen", &[("i32", &fd), ("i32", "128")]), |_| "-1".into())
                },
                |f| f.call("i32", "connect", &[("i32", &fd), ("ptr", &addr), ("i32", &alen)]),
            );
            let okc = f.icmp("eq", "i32", &rc, "0");
            f.if_then(&okc, |f| {
                f.store("i32", &fd, &fdv);
                f.br(brk);
            });
            // keep errno from the failed call across close()
            let ep = f.call("ptr", "__errno_location", &[]);
            let saved = f.load("i32", &ep);
            f.call("i32", "close", &[("i32", &fd)]);
            f.store("i32", &saved, &ep);
        },
    );
    let r = f.load("ptr", &res);
    f.call("void", "freeaddrinfo", &[("ptr", &r)]);
    let fd = f.load("i32", &fdv);
    let none = f.icmp("slt", "i32", &fd, "0");
    f.if_then(&none, |f| {
        let what = f.select("%listen", "ptr", &what_l, &what_c);
        let e = f.call(V, "jv_errno_err", &[("ptr", &what)]);
        f.ret(V, &e);
    });
    let d = f.cast("sitofp", "i32", &fd, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_socket_connect", &[(V, "x"), (V, "h"), (V, "p")], "");
    let r = f.call(V, "jv_sock_open", &[(V, "%h"), (V, "%p"), ("i1", "false")]);
    f.ret(V, &r);
    m.add(f);
    let mut f = Func::new("internal", V, "jvb_socket_listen", &[(V, "x"), (V, "h"), (V, "p")], "");
    let r = f.call(V, "jv_sock_open", &[(V, "%h"), (V, "%p"), ("i1", "true")]);
    f.ret(V, &r);
    m.add(f);
}

fn ops(m: &mut Module) {
    let what_a = m.cstr("socket_accept");
    let what_s = m.cstr("socket_send");
    let what_r = m.cstr("socket_recv");
    let what_sh = m.cstr("socket_shutdown");
    let what_c = m.cstr("socket_close");
    let send_str = m.cstr("socket_send: data must be a string");
    let recv_max = m.cstr("socket_recv: max must be a positive number");

    let fd_or_err = |f: &mut Func, arg: &str| -> String {
        let fd = f.call("i32", "jv_fd_arg", &[(V, arg)]);
        let bad = f.icmp("slt", "i32", &fd, "0");
        f.if_then(&bad, |f| {
            let e = f.call(V, "jv_fd_err", &[]);
            f.ret(V, &e);
        });
        fd
    };

    let mut f = Func::new("internal", V, "jvb_socket_accept", &[(V, "x"), (V, "s")], "");
    let fd = fd_or_err(&mut f, "%s");
    let c = f.call("i32", "accept", &[("i32", &fd), ("ptr", "null"), ("ptr", "null")]);
    let bad = f.icmp("slt", "i32", &c, "0");
    f.if_then(&bad, |f| {
        let e = f.call(V, "jv_errno_err", &[("ptr", &what_a)]);
        f.ret(V, &e);
    });
    let d = f.cast("sitofp", "i32", &c, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_socket_send", &[(V, "x"), (V, "s"), (V, "data")], "");
    let fd = fd_or_err(&mut f, "%s");
    let iss = f.tag_is("%data", T_STR);
    let nots = f.not1(&iss);
    f.if_then(&nots, |f| {
        let e = f.call(V, "jv_err_c", &[("ptr", &send_str)]);
        f.ret(V, &e);
    });
    let p = f.call("ptr", "jv_str_ptr", &[(V, "%data")]);
    let n = f.call("i64", "jv_str_len", &[(V, "%data")]);
    let done = f.var_init("i64", "0");
    f.while_(
        |f| {
            let k = f.load("i64", &done);
            f.lt(&k, &n)
        },
        |f, _, _| {
            let k = f.load("i64", &done);
            let q = f.off(&p, &k);
            let left = f.sub(&n, &k);
            // MSG_NOSIGNAL: report EPIPE as an error instead of dying on SIGPIPE
            let w = f.call("i64", "send", &[("i32", &fd), ("ptr", &q), ("i64", &left), ("i32", "16384")]);
            let bad = f.icmp("slt", "i64", &w, "0");
            f.if_then(&bad, |f| {
                let e = f.call(V, "jv_errno_err", &[("ptr", &what_s)]);
                f.ret(V, &e);
            });
            let k2 = f.add(&k, &w);
            f.store("i64", &k2, &done);
        },
    );
    let d = f.cast("sitofp", "i64", &n, "double");
    let v = f.mknum(&d);
    f.ret(V, &v);
    m.add(f);

    let mut f = Func::new("internal", V, "jvb_socket_recv", &[(V, "x"), (V, "s"), (V, "max")], "");
    let fd = fd_or_err(&mut f, "%s");
    let isn = f.tag_is("%max", T_NUM);
    let md = f.num_of("%max");
    let pos = f.fcmp("oge", &md, "1.0");
    let ok = f.and1(&isn, &pos);
    let bad = f.not1(&ok);
    f.if_then(&bad, |f| {
        let e = f.call(V, "jv_err_c", &[("ptr", &recv_max)]);
        f.ret(V, &e);
    });
    let mx = f.cast("fptosi", "double", &md, "i64");
    let s = f.call(V, "jv_str_alloc", &[("i64", &mx)]);
    let sp = f.ptr_of(&s);
    let buf = f.off(&sp, "32");
    let r = f.call("i64", "recv", &[("i32", &fd), ("ptr", &buf), ("i64", &mx), ("i32", "0")]);
    let bad = f.icmp("slt", "i64", &r, "0");
    f.if_then(&bad, |f| {
        f.call("void", "jv_release", &[(V, &s)]);
        let e = f.call(V, "jv_errno_err", &[("ptr", &what_r)]);
        f.ret(V, &e);
    });
    f.st_at("i64", &r, &sp, 8);
    let end = f.off(&buf, &r);
    f.store("i8", "0", &end);
    f.ret(V, &s);
    m.add(f);

    for (name, what, shut) in [("jvb_socket_shutdown", &what_sh, true), ("jvb_socket_close", &what_c, false)] {
        let mut f = Func::new("internal", V, name, &[(V, "x"), (V, "s")], "");
        let fd = fd_or_err(&mut f, "%s");
        let rc = if shut {
            f.call("i32", "shutdown", &[("i32", &fd), ("i32", "1")]) // SHUT_WR
        } else {
            f.call("i32", "close", &[("i32", &fd)])
        };
        let bad = f.icmp("slt", "i32", &rc, "0");
        f.if_then(&bad, |f| {
            let e = f.call(V, "jv_errno_err", &[("ptr", what)]);
            f.ret(V, &e);
        });
        f.ret(V, NULL);
        m.add(f);
    }
}
