//! jq → LLVM IR code generation.
//!
//! Filters are compiled in continuation-passing style: generating code for an
//! expression takes a compile-time continuation `k` describing what to do with
//! each output. Generators (`.[]`, `range`, `,`) become inline loops/sequences
//! that run `k`'s code once per output. When a continuation has to be invoked
//! from several places (comma, if, function calls) it is "materialized" into a
//! real LLVM function taking an environment pointer.
//!
//! Every LLVM function owns a stack frame (an array of 16-byte slots); slot 0
//! holds the parent frame pointer (the env parameter). Variables and other
//! state that may be reached from nested functions live in frame slots and are
//! addressed by (function id, slot) — `Loc` — walking the static chain.
//!
//! Errors are values: runtime calls and continuations return a status %V whose
//! tag >= 7 means "abort" (error / try-bypass / break). Each emission point has
//! a `Handler` deciding where aborts go (return, jump to a catch block, label).

use crate::ast::*;
use crate::ir::{Func, Module, V};
use crate::rt::{self, FALSE, NULL, OK, TRUE};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Path-mode marker for a value that is not a path (jq reports errors lazily).
const NONPATH: &str = "{ i64 10, i64 0 }";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loc {
    f: usize,
    slot: usize,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub v: String,
    pub p: Option<String>,
}

impl Item {
    fn val(v: &str) -> Item {
        Item { v: v.to_string(), p: None }
    }
}

#[derive(Clone, Copy)]
struct ItemLoc {
    v: Loc,
    p: Option<Loc>,
}

enum Cleanup {
    Val(String),
    Slot(Loc),
}

enum Handler {
    Ret,
    /// `discard`: the error value is never looked at (`?`, `?//` alternatives)
    Catch { fnid: usize, try_id: usize, depth: usize, block: String, errslot: String, outer: Rc<Handler>, discard: bool },
    Label { fnid: usize, id: Loc, depth: usize, block: String, outer: Rc<Handler> },
}

type ContFn<'a> = Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, &Item) + 'a>;

enum ContKind<'a> {
    Closure(ContFn<'a>),
    Call { name: String, owner: usize },
    Ret { k: Loc, kenv: Loc },
    TryExit { try_id: usize, owner: usize, inner: K<'a> },
}

struct Cont<'a> {
    h: Rc<Handler>,
    kind: ContKind<'a>,
    path: bool,
    cheap: bool,
}

type K<'a> = Rc<Cont<'a>>;

pub struct FnCtx {
    id: usize,
    f: Func,
    nslots: usize,
    cleanups: Vec<Cleanup>,
    handler: Rc<Handler>,
}

struct DefInst<'a> {
    def: &'a FuncDef,
    scope: RefCell<Scope<'a>>,
    definer: usize,
    recursive: bool,
    compiled: RefCell<[Option<String>; 2]>,
    /// direct-style variant (at most one output, value params only)
    direct: RefCell<Option<String>>,
    /// cached "at most one output" analysis result
    aom: std::cell::Cell<Option<bool>>,
}

#[derive(Clone)]
enum FuncBind<'a> {
    Def(Rc<DefInst<'a>>),
    Inline(&'a Expr, Scope<'a>),
    Closure(Loc),
}

#[derive(Clone)]
enum Entry<'a> {
    Var(String, Loc),
    Func(String, usize, FuncBind<'a>),
    Label(String, Loc),
}

struct ScopeNode<'a> {
    e: Entry<'a>,
    next: Scope<'a>,
}

#[derive(Clone, Default)]
pub struct Scope<'a>(Option<Rc<ScopeNode<'a>>>);

impl<'a> Scope<'a> {
    fn push(&self, e: Entry<'a>) -> Scope<'a> {
        Scope(Some(Rc::new(ScopeNode { e, next: self.clone() })))
    }
    fn var(&self, name: &str) -> Option<Loc> {
        let mut s = &self.0;
        while let Some(n) = s {
            if let Entry::Var(v, l) = &n.e {
                if v == name {
                    return Some(*l);
                }
            }
            s = &n.next.0;
        }
        None
    }
    fn label(&self, name: &str) -> Option<Loc> {
        let mut s = &self.0;
        while let Some(n) = s {
            if let Entry::Label(v, l) = &n.e {
                if v == name {
                    return Some(*l);
                }
            }
            s = &n.next.0;
        }
        None
    }
    fn func(&self, name: &str, arity: usize) -> Option<FuncBind<'a>> {
        let mut s = &self.0;
        while let Some(n) = s {
            if let Entry::Func(v, a, b) = &n.e {
                if v == name && *a == arity {
                    return Some(b.clone());
                }
            }
            s = &n.next.0;
        }
        None
    }
}

pub struct Gen<'a> {
    pub m: Module,
    parents: Vec<Option<usize>>,
    consts: HashMap<String, String>,
    nconst: usize,
    nfn: usize,
    ntry: usize,
    uses_input: bool,
    inline_stack: Vec<*const DefInst<'a>>,
    /// variables pre-declared by `?//` alternatives (reused while destructuring)
    alt_vars: Vec<(String, Loc)>,
    /// generating functions for cold definitions (compiled with optnone)
    cold: bool,
    aom_stack: Vec<*const DefInst<'a>>,
    natives: HashMap<(&'static str, usize), &'static str>,
    _p: std::marker::PhantomData<&'a ()>,
}

/// Simple natives: (jq name, arity, runtime function). The runtime function
/// takes (input, args...) and returns an owned value or an error.
pub const NATIVES: &[(&str, usize, &str)] = &[
    ("length", 0, "jv_length"),
    ("keys", 0, "jv_keys"),
    ("keys_unsorted", 0, "jv_keys_unsorted"),
    ("tostring", 0, "jv_tostring"),
    ("tojson", 0, "jv_tojson"),
    ("type", 0, "jv_type"),
    ("sort", 0, "jv_sort"),
];

impl<'a> Gen<'a> {
    pub fn new() -> Self {
        let mut natives = HashMap::new();
        for (n, a, f) in NATIVES {
            natives.insert((*n, *a), *f);
        }
        for (n, a, f) in crate::rt::builtins::NATIVES.iter().chain(crate::rt::net::NATIVES) {
            natives.insert((*n, *a), *f);
        }
        Gen {
            m: Module::new(),
            parents: vec![],
            consts: HashMap::new(),
            nconst: 0,
            nfn: 0,
            ntry: 0,
            uses_input: false,
            inline_stack: vec![],
            alt_vars: vec![],
            cold: false,
            aom_stack: vec![],
            natives,
            _p: std::marker::PhantomData,
        }
    }

    // ------------------------------------------------------------------
    // functions, frames, slots

    fn new_fn(&mut self, parent: Option<usize>, prefix: &str, params: &[(&str, &str)]) -> FnCtx {
        let id = self.parents.len();
        self.parents.push(parent);
        self.nfn += 1;
        let name = format!("{}.{}", prefix, self.nfn);
        // cold code (the jq-written regex compiler) is not worth optimising
        let attrs = if self.cold { "noinline optnone" } else { "" };
        let mut f = Func::new("internal", V, &name, params, attrs);
        f.push_alloca("%frame = alloca [__NSLOTS__ x %V], align 16".into());
        f.store("ptr", "%env", "%frame");
        FnCtx { id, f, nslots: 1, cleanups: vec![], handler: Rc::new(Handler::Ret) }
    }

    fn fn_name(cx: &FnCtx) -> String {
        // extract from header: "define internal %V @name("
        let h = cx_header(&cx.f);
        h
    }

    fn finish_fn(&mut self, mut cx: FnCtx) {
        if !cx.f.terminated {
            cx.f.ret(V, OK);
        }
        let n = cx.nslots;
        let text = cx.f.finish().replace("__NSLOTS__", &n.to_string());
        self.m.funcs.push(text);
    }

    fn slot(&mut self, cx: &mut FnCtx) -> Loc {
        let l = Loc { f: cx.id, slot: cx.nslots };
        cx.nslots += 1;
        l
    }

    fn frame_of(&self, cx: &mut FnCtx, target: usize) -> String {
        let mut p = "%frame".to_string();
        let mut id = cx.id;
        while id != target {
            p = cx.f.load("ptr", &p);
            id = self.parents[id].expect("frame not reachable from here");
        }
        p
    }

    fn slot_ptr(&self, cx: &mut FnCtx, l: Loc) -> String {
        let fp = self.frame_of(cx, l.f);
        if l.slot == 0 {
            fp
        } else {
            cx.f.off(&fp, &(16 * l.slot).to_string())
        }
    }

    fn load_slot(&self, cx: &mut FnCtx, l: Loc, ty: &str) -> String {
        let p = self.slot_ptr(cx, l);
        cx.f.load(ty, &p)
    }

    fn store_slot(&self, cx: &mut FnCtx, l: Loc, ty: &str, v: &str) {
        let p = self.slot_ptr(cx, l);
        cx.f.store(ty, v, &p);
    }

    fn spill_v(&mut self, cx: &mut FnCtx, v: &str) -> Loc {
        let l = self.slot(cx);
        self.store_slot(cx, l, V, v);
        l
    }

    fn spill(&mut self, cx: &mut FnCtx, it: &Item) -> ItemLoc {
        let v = self.spill_v(cx, &it.v);
        let p = it.p.as_ref().map(|p| self.spill_v(cx, p));
        ItemLoc { v, p }
    }

    fn reload(&self, cx: &mut FnCtx, il: ItemLoc) -> Item {
        let v = self.load_slot(cx, il.v, V);
        let p = il.p.map(|p| self.load_slot(cx, p, V));
        Item { v, p }
    }

    // ------------------------------------------------------------------
    // refcounting and cleanups

    fn retain(cx: &mut FnCtx, v: &str) {
        cx.f.call("void", "jv_retain", &[(V, v)]);
    }
    fn release(cx: &mut FnCtx, v: &str) {
        cx.f.call("void", "jv_release", &[(V, v)]);
    }

    fn release_cleanups(&self, cx: &mut FnCtx, from: usize) {
        for i in (from..cx.cleanups.len()).rev() {
            match &cx.cleanups[i] {
                // only reached on error paths: use the out-of-line release
                Cleanup::Val(v) => {
                    let v = v.clone();
                    cx.f.call("void", "jv_release_cold", &[(V, &v)]);
                }
                Cleanup::Slot(l) => {
                    let l = *l;
                    let v = self.load_slot(cx, l, V);
                    cx.f.call("void", "jv_release_cold", &[(V, &v)]);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // error handling

    /// Branch away with abort status `st` according to handler `h`.
    /// `known_err`: st is statically known to be an error (tag 7).
    fn act(&mut self, cx: &mut FnCtx, h: &Rc<Handler>, st: &str, known_err: bool) {
        match &**h {
            Handler::Catch { fnid, try_id, depth, block, errslot, outer, .. } if *fnid == cx.id => {
                let (depth, block, errslot, try_id, outer) = (*depth, block.clone(), errslot.clone(), *try_id, outer.clone());
                let catch = |g: &mut Self, cx: &mut FnCtx| {
                    g.release_cleanups(cx, depth);
                    cx.f.store(V, st, &errslot);
                    cx.f.br(&block);
                };
                if known_err {
                    catch(self, cx);
                    return;
                }
                let t = cx.f.tag(st);
                let iserr = cx.f.eq(&t, "7");
                let (lc, ln) = (cx.f.label("catch"), cx.f.label("nocatch"));
                cx.f.cbr(&iserr, &lc, &ln);
                cx.f.block(&lc);
                catch(self, cx);
                cx.f.block(&ln);
                let isbyp = cx.f.eq(&t, "8");
                let (lb, lo) = (cx.f.label("byp"), cx.f.label("other"));
                cx.f.cbr(&isbyp, &lb, &lo);
                cx.f.block(&lb);
                let id = cx.f.call("i64", "jv_bypass_id", &[(V, st)]);
                let mine = cx.f.eq(&id, &try_id.to_string());
                let (lm, lo2) = (cx.f.label("mine"), cx.f.label("notmine"));
                cx.f.cbr(&mine, &lm, &lo2);
                cx.f.block(&lm);
                let u = cx.f.call(V, "jv_bypass_unwrap", &[(V, st)]);
                self.act(cx, &outer, &u, false);
                cx.f.block(&lo2);
                self.act(cx, &outer, st, false);
                cx.f.block(&lo);
                self.act(cx, &outer, st, false);
            }
            Handler::Label { fnid, id, depth, block, outer } if *fnid == cx.id => {
                let (id, depth, block, outer) = (*id, *depth, block.clone(), outer.clone());
                if known_err {
                    self.act(cx, &outer, st, true);
                    return;
                }
                let t = cx.f.tag(st);
                let isbrk = cx.f.eq(&t, "9");
                let b = cx.f.bits(st);
                let myid = self.load_slot(cx, id, "i64");
                let same = cx.f.eq(&b, &myid);
                let mine = cx.f.and1(&isbrk, &same);
                let (lm, lo) = (cx.f.label("brk"), cx.f.label("nobrk"));
                cx.f.cbr(&mine, &lm, &lo);
                cx.f.block(&lm);
                self.release_cleanups(cx, depth);
                cx.f.br(&block);
                cx.f.block(&lo);
                self.act(cx, &outer, st, false);
            }
            _ => {
                self.release_cleanups(cx, 0);
                cx.f.ret(V, st);
            }
        }
    }

    /// If `st` is an abort status, dispatch it with the current handler.
    fn check(&mut self, cx: &mut FnCtx, st: &str) {
        let t = cx.f.tag(st);
        let abort = cx.f.icmp("uge", "i64", &t, "7");
        let (la, lok) = (cx.f.label("abort"), cx.f.label("ok"));
        cx.f.ins(format!("call i1 @llvm.expect.i1(i1 {}, i1 false)", abort));
        cx.f.cbr(&abort, &la, &lok);
        cx.f.block(&la);
        let h = cx.handler.clone();
        self.act(cx, &h, st, false);
        cx.f.block(&lok);
    }

    /// Check a runtime-op result (owned value or error).
    fn check_err(&mut self, cx: &mut FnCtx, r: &str) {
        let t = cx.f.tag(r);
        let iserr = cx.f.eq(&t, "7");
        let (la, lok) = (cx.f.label("err"), cx.f.label("ok"));
        cx.f.cbr(&iserr, &la, &lok);
        cx.f.block(&la);
        let h = cx.handler.clone();
        self.act(cx, &h, r, true);
        cx.f.block(&lok);
    }

    fn raise(&mut self, cx: &mut FnCtx, e: &str) {
        let h = cx.handler.clone();
        self.act(cx, &h, e, true);
        let l = cx.f.label("after_raise");
        cx.f.block(&l);
    }

    fn raise_msg(&mut self, cx: &mut FnCtx, msg: &str) {
        let s = self.m.cstr(msg);
        let e = cx.f.call(V, "jv_err_c", &[("ptr", &s)]);
        self.raise(cx, &e);
    }

    /// Raise `err` (computed by `mk`) if path `p` is the NONPATH marker.
    /// Is the current handler a `?`-style catch in this function (error value unused)?
    fn discarding(&self, cx: &FnCtx) -> bool {
        matches!(&*cx.handler, Handler::Catch { fnid, discard: true, .. } if *fnid == cx.id)
    }

    fn check_path(&mut self, cx: &mut FnCtx, p: &str, mk: impl FnOnce(&mut Func) -> String) {
        let t = cx.f.tag(p);
        let bad = cx.f.eq(&t, "10");
        let (lb, lo) = (cx.f.label("nonpath"), cx.f.label("path_ok"));
        cx.f.cbr(&bad, &lb, &lo);
        cx.f.block(&lb);
        let e = mk(&mut cx.f);
        let h = cx.handler.clone();
        self.act(cx, &h, &e, true);
        cx.f.block(&lo);
    }

    // ------------------------------------------------------------------
    // continuations

    fn closure(&self, h: &Rc<Handler>, path: bool, f: impl Fn(&mut Gen<'a>, &mut FnCtx, &Item) + 'a) -> K<'a> {
        Rc::new(Cont { h: h.clone(), kind: ContKind::Closure(Rc::new(f)), path, cheap: false })
    }

    fn cheap_closure(&self, h: &Rc<Handler>, path: bool, f: impl Fn(&mut Gen<'a>, &mut FnCtx, &Item) + 'a) -> K<'a> {
        Rc::new(Cont { h: h.clone(), kind: ContKind::Closure(Rc::new(f)), path, cheap: true })
    }

    fn invoke(&mut self, cx: &mut FnCtx, k: &K<'a>, it: &Item) {
        let saved = std::mem::replace(&mut cx.handler, k.h.clone());
        match &k.kind {
            ContKind::Closure(c) => {
                let c = c.clone();
                c(self, cx, it);
            }
            ContKind::Call { name, owner } => {
                let env = self.frame_of(cx, *owner);
                let st = if let Some(p) = &it.p {
                    cx.f.call(V, &format!("@{}", name), &[("ptr", &env), (V, p), (V, &it.v)])
                } else {
                    cx.f.call(V, &format!("@{}", name), &[("ptr", &env), (V, &it.v)])
                };
                self.check(cx, &st);
            }
            ContKind::Ret { k: kl, kenv } => {
                let kf = self.load_slot(cx, *kl, "ptr");
                let ke = self.load_slot(cx, *kenv, "ptr");
                let st = if let Some(p) = &it.p {
                    cx.f.call(V, &kf, &[("ptr", &ke), (V, p), (V, &it.v)])
                } else {
                    cx.f.call(V, &kf, &[("ptr", &ke), (V, &it.v)])
                };
                self.check(cx, &st);
            }
            ContKind::TryExit { try_id, owner, inner } => {
                if cx.id == *owner {
                    let inner = inner.clone();
                    self.invoke(cx, &inner, it);
                } else {
                    let m = self.materialize(cx, inner);
                    let ContKind::Call { name, owner } = &m.kind else { unreachable!() };
                    let env = self.frame_of(cx, *owner);
                    let st = if let Some(p) = &it.p {
                        cx.f.call(V, &format!("@{}", name), &[("ptr", &env), (V, p), (V, &it.v)])
                    } else {
                        cx.f.call(V, &format!("@{}", name), &[("ptr", &env), (V, &it.v)])
                    };
                    let t = cx.f.tag(&st);
                    let abort = cx.f.icmp("uge", "i64", &t, "7");
                    let (la, lok) = (cx.f.label("bypass"), cx.f.label("ok"));
                    cx.f.cbr(&abort, &la, &lok);
                    cx.f.block(&la);
                    self.release_cleanups(cx, 0);
                    let w = cx.f.call(V, "jv_bypass", &[("i64", &try_id.to_string()), (V, &st)]);
                    cx.f.ret(V, &w);
                    cx.f.block(&lok);
                }
            }
        }
        cx.handler = saved;
    }

    /// Turn a continuation into an LLVM function (owned by `cx`'s frame).
    fn materialize(&mut self, cx: &mut FnCtx, k: &K<'a>) -> K<'a> {
        match &k.kind {
            ContKind::Call { .. } => return k.clone(),
            _ => {}
        }
        let params: Vec<(&str, &str)> = if k.path {
            vec![("ptr", "env"), (V, "p"), (V, "x")]
        } else {
            vec![("ptr", "env"), (V, "x")]
        };
        let mut m = self.new_fn(Some(cx.id), "k", &params);
        let it = Item { v: "%x".into(), p: if k.path { Some("%p".into()) } else { None } };
        self.invoke(&mut m, k, &it);
        let name = Self::fn_name(&m);
        self.finish_fn(m);
        Rc::new(Cont { h: k.h.clone(), kind: ContKind::Call { name, owner: cx.id }, path: k.path, cheap: true })
    }

    /// A continuation that may be invoked several times.
    fn share(&mut self, cx: &mut FnCtx, k: &K<'a>) -> K<'a> {
        if k.cheap || matches!(k.kind, ContKind::Call { .. } | ContKind::Ret { .. }) {
            k.clone()
        } else {
            self.materialize(cx, k)
        }
    }

    /// (function pointer, env pointer) to pass as a runtime continuation.
    fn kpair(&mut self, cx: &mut FnCtx, k: &K<'a>) -> (String, String) {
        if let ContKind::Ret { k: kl, kenv } = &k.kind {
            let kf = self.load_slot(cx, *kl, "ptr");
            let ke = self.load_slot(cx, *kenv, "ptr");
            return (kf, ke);
        }
        let m = self.materialize(cx, k);
        let ContKind::Call { name, owner } = &m.kind else { unreachable!() };
        let env = self.frame_of(cx, *owner);
        (format!("@{}", name), env)
    }

    // ------------------------------------------------------------------
    // constants

    fn const_json(&mut self, cx: &mut FnCtx, json: &str) -> String {
        let g = match self.consts.get(json) {
            Some(g) => g.clone(),
            None => {
                self.nconst += 1;
                let g = format!("@const.{}", self.nconst);
                self.m.global(format!("{} = internal global %V zeroinitializer", g));
                self.consts.insert(json.to_string(), g.clone());
                g
            }
        };
        cx.f.load(V, &g)
    }

    fn lit(&mut self, cx: &mut FnCtx, l: &Lit) -> String {
        match l {
            Lit::Null => NULL.into(),
            Lit::True => TRUE.into(),
            Lit::False => FALSE.into(),
            Lit::Num(n) => rt::numconst(*n),
            Lit::NumText(t) => self.const_json(cx, t),
            Lit::Str(s) => {
                let j = json_str(s);
                self.const_json(cx, &j)
            }
        }
    }

    fn str_const(&mut self, cx: &mut FnCtx, s: &str) -> String {
        let j = json_str(s);
        self.const_json(cx, &j)
    }

    // ------------------------------------------------------------------
    // code generation

    /// Evaluate `e` in value mode on input `v`, then run `then` for each output.
    fn each_value(
        &mut self,
        cx: &mut FnCtx,
        e: &'a Expr,
        sc: &Scope<'a>,
        v: &str,
        then: impl Fn(&mut Gen<'a>, &mut FnCtx, &str) + 'a,
    ) {
        let h = cx.handler.clone();
        let k = self.closure(&h, false, move |g, cx, it| then(g, cx, &it.v));
        self.cg(cx, e, sc, &Item::val(v), &k);
    }

    /// Emit an owned result, release it afterwards.
    fn emit_owned(&mut self, cx: &mut FnCtx, k: &K<'a>, r: &str, p: Option<&str>) {
        cx.cleanups.push(Cleanup::Val(r.to_string()));
        self.invoke(cx, k, &Item { v: r.to_string(), p: p.map(|s| s.to_string()) });
        cx.cleanups.pop();
        Self::release(cx, r);
    }

    /// Emit a value-mode result; in path mode this is an error.
    /// In path mode the value is passed on with the NONPATH marker; jq reports
    /// the error only when a path operation or consumer meets it.
    fn emit_value(&mut self, cx: &mut FnCtx, it: &Item, k: &K<'a>, r: &str, owned: bool) {
        if it.p.is_some() {
            if owned {
                cx.cleanups.push(Cleanup::Val(r.to_string()));
            }
            self.invoke(cx, k, &Item { v: r.to_string(), p: Some(NONPATH.to_string()) });
            if owned {
                cx.cleanups.pop();
                Self::release(cx, r);
            }
            return;
        }
        if owned {
            self.emit_owned(cx, k, r, None);
        } else {
            self.invoke(cx, k, &Item::val(r));
        }
    }

    fn cg(&mut self, cx: &mut FnCtx, e: &'a Expr, sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        match e {
            Expr::Identity => self.invoke(cx, k, it),
            Expr::Lit(l) => {
                let v = self.lit(cx, l);
                self.emit_value(cx, it, k, &v, false);
            }
            Expr::Var(name) => {
                let v = self.var(cx, sc, name);
                self.emit_value(cx, it, k, &v, true);
            }
            Expr::Pipe(a, b) => {
                let sc2 = sc.clone();
                let k2 = k.clone();
                let b: &'a Expr = b;
                let h = cx.handler.clone();
                let kk = self.closure(&h, it.p.is_some(), move |g, cx, x| g.cg(cx, b, &sc2, x, &k2));
                self.cg(cx, a, sc, it, &kk);
            }
            Expr::Comma(a, b) => {
                let k2 = self.share(cx, k);
                self.cg(cx, a, sc, it, &k2);
                self.cg(cx, b, sc, it, &k2);
            }
            Expr::Index(t, i) => self.cg_index(cx, t, i, sc, it, k),
            Expr::Slice(t, from, to) => self.cg_slice(cx, t, from.as_deref(), to.as_deref(), sc, it, k),
            Expr::Iterate(t) => {
                let k2 = k.clone();
                let h = cx.handler.clone();
                let kk = self.closure(&h, it.p.is_some(), move |g, cx, b| g.iterate(cx, b, &k2));
                self.cg(cx, t, sc, it, &kk);
            }
            Expr::Neg(x) => {
                let k2 = k.clone();
                let it2 = it.clone();
                let pathmode = it.p.is_some();
                let _ = it2;
                self.each_value(cx, x, sc, &it.v.clone(), move |g, cx, v| {
                    let r = cx.f.call(V, "jv_neg", &[(V, v)]);
                    g.check_err(cx, &r);
                    let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
                    g.emit_value(cx, &fake, &k2, &r, true);
                });
            }
            Expr::Bin(op, a, b) => self.cg_bin(cx, *op, a, b, sc, it, k),
            Expr::And(a, b) | Expr::Or(a, b) => {
                let is_and = matches!(e, Expr::And(..));
                let k2 = self.share(cx, k);
                let ivl = self.spill_v(cx, &it.v);
                let pathmode = it.p.is_some();
                let sc2 = sc.clone();
                let b: &'a Expr = b;
                self.each_value(cx, a, sc, &it.v.clone(), move |g, cx, av| {
                    let t = cx.f.tag(av);
                    let truthy = cx.f.icmp("uge", "i64", &t, "2");
                    let (lshort, lrhs, lend) = (cx.f.label("short"), cx.f.label("rhs"), cx.f.label("andor"));
                    if is_and {
                        cx.f.cbr(&truthy, &lrhs, &lshort);
                    } else {
                        cx.f.cbr(&truthy, &lshort, &lrhs);
                    }
                    cx.f.block(&lshort);
                    let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
                    g.emit_value(cx, &fake, &k2, if is_and { FALSE } else { TRUE }, false);
                    cx.f.br(&lend);
                    cx.f.block(&lrhs);
                    let iv = g.load_slot(cx, ivl, V);
                    let k3 = k2.clone();
                    g.each_value(cx, b, &sc2, &iv, move |g, cx, bv| {
                        let t = cx.f.tag(bv);
                        let truthy = cx.f.icmp("uge", "i64", &t, "2");
                        let r = cx.f.select(&truthy, V, TRUE, FALSE);
                        let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
                        g.emit_value(cx, &fake, &k3, &r, false);
                    });
                    cx.f.br(&lend);
                    cx.f.block(&lend);
                });
            }
            Expr::Alt(a, b) => {
                let k2 = self.share(cx, k);
                let found = self.slot(cx);
                self.store_slot(cx, found, "i64", "0");
                let k3 = k2.clone();
                let h = cx.handler.clone();
                let ka = self.closure(&h, it.p.is_some(), move |g, cx, x| {
                    let t = cx.f.tag(&x.v);
                    let truthy = cx.f.icmp("uge", "i64", &t, "2");
                    let (lt, le) = (cx.f.label("alt_t"), cx.f.label("alt_e"));
                    cx.f.cbr(&truthy, &lt, &le);
                    cx.f.block(&lt);
                    g.store_slot(cx, found, "i64", "1");
                    g.invoke(cx, &k3, x);
                    cx.f.br(&le);
                    cx.f.block(&le);
                });
                self.cg(cx, a, sc, it, &ka);
                let fv = self.load_slot(cx, found, "i64");
                let none = cx.f.eq(&fv, "0");
                let (lb, le) = (cx.f.label("alt_b"), cx.f.label("alt_end"));
                cx.f.cbr(&none, &lb, &le);
                cx.f.block(&lb);
                self.cg(cx, b, sc, it, &k2);
                cx.f.br(&le);
                cx.f.block(&le);
            }
            Expr::If(c, t, els) => {
                let k2 = self.share(cx, k);
                let il = self.spill(cx, it);
                let sc2 = sc.clone();
                let t: &'a Expr = t;
                let els: Option<&'a Expr> = els.as_deref();
                self.each_value(cx, c, sc, &it.v.clone(), move |g, cx, cv| {
                    let tg = cx.f.tag(cv);
                    let truthy = cx.f.icmp("uge", "i64", &tg, "2");
                    let (lt, le, lj) = (cx.f.label("if_t"), cx.f.label("if_e"), cx.f.label("if_j"));
                    cx.f.cbr(&truthy, &lt, &le);
                    cx.f.block(&lt);
                    let it = g.reload(cx, il);
                    g.cg(cx, t, &sc2, &it, &k2);
                    cx.f.br(&lj);
                    cx.f.block(&le);
                    let it = g.reload(cx, il);
                    match els {
                        Some(e) => g.cg(cx, e, &sc2, &it, &k2),
                        None => g.invoke(cx, &k2, &it),
                    }
                    cx.f.br(&lj);
                    cx.f.block(&lj);
                });
            }
            Expr::Array(None) => {
                let a = cx.f.call(V, "jv_arr", &[("i64", "0")]);
                self.emit_value(cx, it, k, &a, true);
            }
            Expr::Array(Some(x)) => {
                let s = self.slot(cx);
                let a = cx.f.call(V, "jv_arr", &[("i64", "4")]);
                self.store_slot(cx, s, V, &a);
                cx.cleanups.push(Cleanup::Slot(s));
                let h = cx.handler.clone();
                let kc = self.cheap_closure(&h, false, move |g, cx, x| {
                    let a = g.load_slot(cx, s, V);
                    let a2 = cx.f.call(V, "jv_arr_push", &[(V, &a), (V, &x.v)]);
                    g.store_slot(cx, s, V, &a2);
                });
                self.cg(cx, x, sc, &Item::val(&it.v), &kc);
                cx.cleanups.pop();
                let a = self.load_slot(cx, s, V);
                self.emit_value(cx, it, k, &a, true);
            }
            Expr::Object(entries) => {
                for (ke, _) in entries {
                    check_const_key(ke);
                }
                let il = self.spill(cx, it);
                self.cg_object(cx, entries, 0, vec![], il, sc, k);
            }
            Expr::Interp(fmt, parts) => {
                let ivl = self.spill_v(cx, &it.v);
                let pathmode = it.p.is_some();
                self.cg_interp(cx, fmt.as_deref(), parts, parts.len(), vec![], ivl, pathmode, sc, k);
            }
            Expr::Format(name) => {
                let n = self.str_const(cx, name);
                let r = cx.f.call(V, "jv_format", &[(V, &n), (V, &it.v)]);
                self.check_err(cx, &r);
                self.emit_value(cx, it, k, &r, true);
            }
            Expr::Try(body, catch) => self.cg_try(cx, body, catch.as_deref(), sc, it, k),
            Expr::Label(name, body) => {
                let idl = self.slot(cx);
                let id = cx.f.call("i64", "jv_new_label", &[]);
                self.store_slot(cx, idl, "i64", &id);
                let done = cx.f.label("label_done");
                let h = Rc::new(Handler::Label {
                    fnid: cx.id,
                    id: idl,
                    depth: cx.cleanups.len(),
                    block: done.clone(),
                    outer: cx.handler.clone(),
                });
                let sc2 = sc.push(Entry::Label(name.clone(), idl));
                let saved = std::mem::replace(&mut cx.handler, h);
                self.cg(cx, body, &sc2, it, k);
                cx.handler = saved;
                cx.f.br(&done);
                cx.f.block(&done);
            }
            Expr::Break(name) => {
                let Some(l) = sc.label(name) else {
                    panic!("$*label-{} is not defined", name);
                };
                let id = self.load_slot(cx, l, "i64");
                let v = cx.f.mkv("9", &id);
                let h = cx.handler.clone();
                self.act(cx, &h, &v, false);
                let l = cx.f.label("after_break");
                cx.f.block(&l);
            }
            Expr::Defs(defs, body) => {
                let mut sc2 = sc.clone();
                for d in defs {
                    let inst = Rc::new(DefInst {
                        def: d,
                        scope: RefCell::new(sc2.clone()),
                        definer: cx.id,
                        recursive: is_recursive(d),
                        compiled: RefCell::new([None, None]),
                direct: RefCell::new(None),
                aom: std::cell::Cell::new(None),
                    });
                    sc2 = sc2.push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst)));
                }
                self.cg(cx, body, &sc2, it, k);
            }
            Expr::Bind(src, pats, body) => self.cg_bind(cx, src, pats, body, sc, it, k),
            Expr::Reduce(src, pat, init, upd) => self.cg_reduce(cx, src, pat, init, upd, sc, it, k),
            Expr::Foreach(src, pat, init, upd, ext) => {
                self.cg_foreach(cx, src, pat, init, upd, ext.as_deref(), sc, it, k)
            }
            Expr::Assign(op, lhs, rhs) => self.cg_assign(cx, *op, lhs, rhs, sc, it, k),
            Expr::Call(name, args) => self.cg_call(cx, name, args, sc, it, k),
        }
    }

    fn var(&mut self, cx: &mut FnCtx, sc: &Scope<'a>, name: &str) -> String {
        if let Some(l) = sc.var(name) {
            let v = self.load_slot(cx, l, V);
            Self::retain(cx, &v);
            return v;
        }
        match name {
            "ENV" => cx.f.call(V, "jv_env", &[]),
            "__prog_args" | "ARGS" => cx.f.call(V, "jv_args", &[]),
            _ => {
                let n = self.str_const(cx, name);
                let r = cx.f.call(V, "jv_named_arg", &[(V, &n)]);
                self.check_err(cx, &r);
                r
            }
        }
    }

    fn cg_index(&mut self, cx: &mut FnCtx, t: &'a Expr, i: &'a Expr, sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let k = k.clone();
        let sc2 = sc.clone();
        let pathmode = it.p.is_some();
        if let Expr::Lit(lit) = i {
            // constant key: no outer generator
            let lit = lit.clone();
            let h = cx.handler.clone();
            let kb = self.closure(&h, pathmode, move |g, cx, b| {
                let key = g.lit(cx, &lit);
                g.index_one(cx, b, &key, &k);
            });
            self.cg(cx, t, sc, it, &kb);
            return;
        }
        let il = self.spill(cx, it);
        self.each_value(cx, i, sc, &it.v.clone(), move |g, cx, key| {
            let kl = g.spill_v(cx, key);
            let it = g.reload(cx, il);
            let k2 = k.clone();
            let h = cx.handler.clone();
            let kb = g.closure(&h, pathmode, move |g, cx, b| {
                let key = g.load_slot(cx, kl, V);
                g.index_one(cx, b, &key, &k2);
            });
            g.cg(cx, t, &sc2, &it, &kb);
        });
    }

    fn index_one(&mut self, cx: &mut FnCtx, b: &Item, key: &str, k: &K<'a>) {
        if let Some(p) = &b.p {
            let (key2, bv) = (key.to_string(), b.v.clone());
            self.check_path(cx, p, |f| f.call(V, "jv_path_access_err", &[(V, &key2), (V, &bv)]));
        }
        let r = cx.f.call(V, "jv_index", &[(V, &b.v), (V, key)]);
        self.check_err(cx, &r);
        match &b.p {
            None => self.emit_owned(cx, k, &r, None),
            Some(p) => {
                cx.cleanups.push(Cleanup::Val(r.clone()));
                Self::retain(cx, p);
                let p2 = cx.f.call(V, "jv_arr_push", &[(V, p), (V, key)]);
                cx.cleanups.push(Cleanup::Val(p2.clone()));
                self.invoke(cx, k, &Item { v: r.clone(), p: Some(p2.clone()) });
                cx.cleanups.pop();
                cx.cleanups.pop();
                Self::release(cx, &p2);
                Self::release(cx, &r);
            }
        }
    }

    fn cg_slice(
        &mut self,
        cx: &mut FnCtx,
        t: &'a Expr,
        from: Option<&'a Expr>,
        to: Option<&'a Expr>,
        sc: &Scope<'a>,
        it: &Item,
        k: &K<'a>,
    ) {
        let il = self.spill(cx, it);
        let pathmode = it.p.is_some();
        let k = k.clone();
        let sc2 = sc.clone();
        let null_e: &'a Expr = &NULL_EXPR;
        let from = from.unwrap_or(null_e);
        let to = to.unwrap_or(null_e);
        self.each_value(cx, to, sc, &it.v.clone(), move |g, cx, tv| {
            let tl = g.spill_v(cx, tv);
            let it = g.reload(cx, il);
            let k2 = k.clone();
            let sc3 = sc2.clone();
            g.each_value(cx, from, &sc2, &it.v, move |g, cx, fv| {
                let fl = g.spill_v(cx, fv);
                let it = g.reload(cx, il);
                let k3 = k2.clone();
                let h = cx.handler.clone();
                let kb = g.closure(&h, pathmode, move |g, cx, b| {
                    let fv = g.load_slot(cx, fl, V);
                    let tv = g.load_slot(cx, tl, V);
                    if let Some(p) = &b.p {
                        let bv = b.v.clone();
                        let (fv2, tv2) = (fv.clone(), tv.clone());
                        g.check_path(cx, p, |f| {
                            let key = f.call(V, "jv_slice_key", &[(V, &fv2), (V, &tv2)]);
                            f.call(V, "jv_path_access_err", &[(V, &key), (V, &bv)])
                        });
                    }
                    let r = cx.f.call(V, "jv_slice", &[(V, &b.v), (V, &fv), (V, &tv)]);
                    g.check_err(cx, &r);
                    match &b.p {
                        None => g.emit_owned(cx, &k3, &r, None),
                        Some(p) => {
                            cx.cleanups.push(Cleanup::Val(r.clone()));
                            let key = cx.f.call(V, "jv_slice_key", &[(V, &fv), (V, &tv)]);
                            Self::retain(cx, p);
                            let p2 = cx.f.call(V, "jv_arr_push", &[(V, p), (V, &key)]);
                            Self::release(cx, &key);
                            cx.cleanups.push(Cleanup::Val(p2.clone()));
                            g.invoke(cx, &k3, &Item { v: r.clone(), p: Some(p2.clone()) });
                            cx.cleanups.pop();
                            cx.cleanups.pop();
                            Self::release(cx, &p2);
                            Self::release(cx, &r);
                        }
                    }
                });
                g.cg(cx, t, &sc3, &it, &kb);
            });
        });
    }

    fn iterate(&mut self, cx: &mut FnCtx, b: &Item, k: &K<'a>) {
        let v = &b.v;
        if let Some(p) = &b.p {
            let bv = v.clone();
            self.check_path(cx, p, |f| f.call(V, "jv_path_iter_err", &[(V, &bv)]));
        }
        let t = cx.f.tag(v);
        let isarr = cx.f.eq(&t, "5");
        let isobj = cx.f.eq(&t, "6");
        let ok = cx.f.or1(&isarr, &isobj);
        let (lbad, lgo) = (cx.f.label("iter_bad"), cx.f.label("iter"));
        cx.f.cbr(&ok, &lgo, &lbad);
        cx.f.block(&lbad);
        // under `?` the message is never seen: skip formatting it
        let e = if self.discarding(cx) { cx.f.load(V, "@g_err_static") } else { cx.f.call(V, "jv_iter_err", &[(V, v)]) };
        let h = cx.handler.clone();
        self.act(cx, &h, &e, true);
        cx.f.block(&lgo);
        let p = cx.f.ptr_of(v);
        let n = cx.f.ld_at("i64", &p, 8);
        let base = cx.f.ld_at("ptr", &p, 24);
        let stride = cx.f.select(&isarr, "i64", "16", "32");
        let offv = cx.f.select(&isarr, "i64", "0", "16");
        let iv = cx.f.var("i64");
        cx.f.store("i64", "0", &iv);
        let (lh, lb, le) = (cx.f.label("it_h"), cx.f.label("it_b"), cx.f.label("it_e"));
        cx.f.br(&lh);
        cx.f.block(&lh);
        let i = cx.f.load("i64", &iv);
        let c = cx.f.lt(&i, &n);
        cx.f.cbr(&c, &lb, &le);
        cx.f.block(&lb);
        let o1 = cx.f.bin("mul", "i64", &i, &stride);
        let o2 = cx.f.add(&o1, &offv);
        let ep = cx.f.off(&base, &o2);
        let elem = cx.f.load(V, &ep);
        match &b.p {
            None => self.invoke(cx, k, &Item::val(&elem)),
            Some(pp) => {
                let key = cx.f.if_val(
                    &isarr,
                    V,
                    |f| {
                        let d = f.cast("sitofp", "i64", &i, "double");
                        f.mknum(&d)
                    },
                    |f| f.call(V, "jv_obj_key_at", &[(V, v), ("i64", &i)]),
                );
                Self::retain(cx, pp);
                let p2 = cx.f.call(V, "jv_arr_push", &[(V, pp), (V, &key)]);
                cx.cleanups.push(Cleanup::Val(p2.clone()));
                self.invoke(cx, k, &Item { v: elem.clone(), p: Some(p2.clone()) });
                cx.cleanups.pop();
                Self::release(cx, &p2);
            }
        }
        let i = cx.f.load("i64", &iv);
        let i1 = cx.f.add(&i, "1");
        cx.f.store("i64", &i1, &iv);
        cx.f.br(&lh);
        cx.f.block(&le);
    }

    fn cg_bin(&mut self, cx: &mut FnCtx, op: BinOp, a: &'a Expr, b: &'a Expr, sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let pathmode = it.p.is_some();
        let k = k.clone();
        let sc2 = sc.clone();
        // fast path: constant rhs needs no spill
        if let Expr::Lit(lit) = b {
            let lit = lit.clone();
            self.each_value(cx, a, sc, &it.v.clone(), move |g, cx, lv| {
                let rv = g.lit(cx, &lit);
                g.binop(cx, op, lv, &rv, pathmode, &k);
            });
            return;
        }
        let ivl = self.spill_v(cx, &it.v);
        self.each_value(cx, b, sc, &it.v.clone(), move |g, cx, rv| {
            let rl = g.spill_v(cx, rv);
            let iv = g.load_slot(cx, ivl, V);
            let k2 = k.clone();
            g.each_value(cx, a, &sc2, &iv, move |g, cx, lv| {
                let rv = g.load_slot(cx, rl, V);
                g.binop(cx, op, lv, &rv, pathmode, &k2);
            });
        });
    }

    fn binop(&mut self, cx: &mut FnCtx, op: BinOp, l: &str, r: &str, pathmode: bool, k: &K<'a>) {
        let lt = cx.f.tag(l);
        let rt_ = cx.f.tag(r);
        let ln = cx.f.eq(&lt, "3");
        let rn = cx.f.eq(&rt_, "3");
        let nn = cx.f.and1(&ln, &rn);
        let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
        // comparisons take the fast path only for plain doubles (literals compare exactly)
        let lr = cx.f.rawtag(l);
        let rr = cx.f.rawtag(r);
        let lp = cx.f.eq(&lr, "3");
        let rp = cx.f.eq(&rr, "3");
        let plain = cx.f.and1(&lp, &rp);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let (fop, rtf) = match op {
                    BinOp::Add => ("fadd", "jv_add"),
                    BinOp::Sub => ("fsub", "jv_sub"),
                    BinOp::Mul => ("fmul", "jv_mul"),
                    BinOp::Div => ("fdiv", "jv_div"),
                    _ => ("", "jv_mod"),
                };
                let fast = if op == BinOp::Div {
                    let rb = cx.f.bits(r);
                    let rz = cx.f.ne(&rb, "0");
                    cx.f.and1(&nn, &rz)
                } else {
                    nn.clone()
                };
                let res = if fop.is_empty() {
                    cx.f.call(V, rtf, &[(V, l), (V, r)])
                } else {
                    cx.f.if_val(
                        &fast,
                        V,
                        |f| {
                            let x = f.num_of(l);
                            let y = f.num_of(r);
                            let z = f.bin(fop, "double", &x, &y);
                            f.mknum(&z)
                        },
                        |f| f.call(V, rtf, &[(V, l), (V, r)]),
                    )
                };
                self.check_err(cx, &res);
                self.emit_value(cx, &fake, k, &res, true);
            }
            BinOp::Eq | BinOp::Ne => {
                let c = cx.f.if_val(
                    &plain,
                    "i1",
                    |f| {
                        let x = f.num_of(l);
                        let y = f.num_of(r);
                        f.fcmp("oeq", &x, &y)
                    },
                    |f| f.call("i1", "jv_eq", &[(V, l), (V, r)]),
                );
                let res = if op == BinOp::Eq {
                    cx.f.select(&c, V, TRUE, FALSE)
                } else {
                    cx.f.select(&c, V, FALSE, TRUE)
                };
                self.emit_value(cx, &fake, k, &res, false);
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let (fp, ip) = match op {
                    BinOp::Lt => ("olt", "slt"),
                    BinOp::Le => ("ole", "sle"),
                    BinOp::Gt => ("ogt", "sgt"),
                    _ => ("oge", "sge"),
                };
                let c = cx.f.if_val(
                    &plain,
                    "i1",
                    |f| {
                        let x = f.num_of(l);
                        let y = f.num_of(r);
                        f.fcmp(fp, &x, &y)
                    },
                    |f| {
                        let c = f.call("i32", "jv_cmp", &[(V, l), (V, r)]);
                        f.icmp(ip, "i32", &c, "0")
                    },
                );
                let res = cx.f.select(&c, V, TRUE, FALSE);
                self.emit_value(cx, &fake, k, &res, false);
            }
        }
    }

    fn cg_object(
        &mut self,
        cx: &mut FnCtx,
        entries: &'a [(Expr, Expr)],
        i: usize,
        done: Vec<(Loc, Loc)>,
        il: ItemLoc,
        sc: &Scope<'a>,
        k: &K<'a>,
    ) {
        if i == entries.len() {
            let mut o = cx.f.call(V, "jv_obj", &[]);
            let ov = cx.f.var(V);
            cx.f.store(V, &o, &ov);
            cx.cleanups.push(Cleanup::Val(String::new()));
            let cidx = cx.cleanups.len() - 1;
            for (kl, vl) in &done {
                let key = self.load_slot(cx, *kl, V);
                let val = self.load_slot(cx, *vl, V);
                let isstr = cx.f.tag_is(&key, rt::T_STR);
                let (lbad, lok) = (cx.f.label("objkey_bad"), cx.f.label("objkey_ok"));
                cx.f.cbr(&isstr, &lok, &lbad);
                cx.f.block(&lbad);
                // release the partial object
                cx.cleanups[cidx] = Cleanup::Val(o.clone());
                let e = cx.f.call(V, "jv_objkey_err", &[(V, &key)]);
                let h = cx.handler.clone();
                self.act(cx, &h, &e, true);
                cx.f.block(&lok);
                o = cx.f.call(V, "jv_obj_set", &[(V, &o), (V, &key), (V, &val)]);
                cx.f.store(V, &o, &ov);
            }
            cx.cleanups.pop();
            let it = self.reload(cx, il);
            self.emit_value(cx, &it, k, &o, true);
            return;
        }
        let (ke, ve) = &entries[i];
        let k = k.clone();
        let sc2 = sc.clone();
        let it = self.reload(cx, il);
        self.each_value(cx, ke, sc, &it.v, move |g, cx, kv| {
            let kl = g.spill_v(cx, kv);
            let it = g.reload(cx, il);
            let k2 = k.clone();
            let sc3 = sc2.clone();
            let done2 = done.clone();
            g.each_value(cx, ve, &sc2, &it.v, move |g, cx, vv| {
                let vl = g.spill_v(cx, vv);
                let mut d = done2.clone();
                d.push((kl, vl));
                g.cg_object(cx, entries, i + 1, d, il, &sc3, &k2);
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn cg_interp(
        &mut self,
        cx: &mut FnCtx,
        fmt: Option<&'a str>,
        parts: &'a [StrPart],
        i: usize,
        done: Vec<Option<Loc>>,
        ivl: Loc,
        pathmode: bool,
        sc: &Scope<'a>,
        k: &K<'a>,
    ) {
        // parts are evaluated last-to-first (later parts form the outer loops)
        if i == 0 {
            let mut s = cx.f.call(V, "jv_str_alloc", &[("i64", "16")]);
            let fmtv = match fmt {
                Some(f) => Some(self.str_const(cx, f)),
                None => None,
            };
            // done is in reverse order
            for (j, slot) in done.iter().rev().enumerate() {
                match (&parts[j], slot) {
                    (StrPart::Lit(text), _) => {
                        let c = self.str_const(cx, text);
                        s = cx.f.call(V, "jv_str_append_v", &[(V, &s), (V, &c)]);
                    }
                    (StrPart::Expr(_), Some(l)) => {
                        let v = self.load_slot(cx, *l, V);
                        let piece = match &fmtv {
                            Some(fv) => cx.f.call(V, "jv_format", &[(V, fv), (V, &v)]),
                            None => cx.f.call(V, "jv_tostring", &[(V, &v)]),
                        };
                        if fmtv.is_some() {
                            let t = cx.f.tag(&piece);
                            let bad = cx.f.eq(&t, "7");
                            let (lb, lo) = (cx.f.label("fmt_err"), cx.f.label("fmt_ok"));
                            cx.f.cbr(&bad, &lb, &lo);
                            cx.f.block(&lb);
                            Self::release(cx, &s);
                            let h = cx.handler.clone();
                            self.act(cx, &h, &piece, true);
                            cx.f.block(&lo);
                        }
                        s = cx.f.call(V, "jv_str_append_v", &[(V, &s), (V, &piece)]);
                        Self::release(cx, &piece);
                    }
                    _ => unreachable!(),
                }
            }
            let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
            self.emit_value(cx, &fake, k, &s, true);
            return;
        }
        match &parts[i - 1] {
            StrPart::Lit(_) => {
                let mut d = done;
                d.push(None);
                self.cg_interp(cx, fmt, parts, i - 1, d, ivl, pathmode, sc, k);
            }
            StrPart::Expr(e) => {
                let k = k.clone();
                let sc2 = sc.clone();
                let iv = self.load_slot(cx, ivl, V);
                self.each_value(cx, e, sc, &iv, move |g, cx, v| {
                    let l = g.spill_v(cx, v);
                    let mut d = done.clone();
                    d.push(Some(l));
                    g.cg_interp(cx, fmt, parts, i - 1, d, ivl, pathmode, &sc2, &k);
                });
            }
        }
    }

    fn cg_try(&mut self, cx: &mut FnCtx, body: &'a Expr, catch: Option<&'a Expr>, sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        self.ntry += 1;
        let try_id = self.ntry;
        let errslot = cx.f.var(V);
        let (lc, lend) = (cx.f.label("catch"), cx.f.label("try_end"));
        let outer = cx.handler.clone();
        let h = Rc::new(Handler::Catch {
            fnid: cx.id,
            try_id,
            depth: cx.cleanups.len(),
            block: lc.clone(),
            errslot: errslot.clone(),
            outer: outer.clone(),
            discard: catch.is_none(),
        });
        let k = if catch.is_some() { self.share(cx, k) } else { k.clone() };
        let kexit = Rc::new(Cont {
            h: k.h.clone(),
            kind: ContKind::TryExit { try_id, owner: cx.id, inner: k.clone() },
            path: k.path,
            cheap: false,
        });
        let saved = std::mem::replace(&mut cx.handler, h);
        self.cg(cx, body, sc, it, &kexit);
        cx.handler = saved;
        cx.f.br(&lend);
        cx.f.block(&lc);
        let e = cx.f.load(V, &errslot);
        let msg = cx.f.call(V, "jv_err_msg", &[(V, &e)]);
        // `error(null)` caught by `?` / try without catch is simply dropped
        match catch {
            Some(c) => {
                Self::retain(cx, &msg);
                Self::release(cx, &e);
                cx.cleanups.push(Cleanup::Val(msg.clone()));
                if it.p.is_some() {
                    self.emit_value(cx, it, &k, &msg, false);
                } else {
                    self.cg(cx, c, sc, &Item::val(&msg), &k);
                }
                cx.cleanups.pop();
                Self::release(cx, &msg);
            }
            None => {
                Self::release(cx, &e);
            }
        }
        cx.f.br(&lend);
        cx.f.block(&lend);
    }

    // ------------------------------------------------------------------
    // variable binding / destructuring

    fn cg_bind(&mut self, cx: &mut FnCtx, src: &'a Expr, pats: &'a [Pattern], body: &'a Expr, sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let il = self.spill(cx, it);
        let k = k.clone();
        let sc2 = sc.clone();
        if pats.len() == 1 {
            self.each_value(cx, src, sc, &it.v.clone(), move |g, cx, sv| {
                let k2 = k.clone();
                let done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a> = Rc::new(move |g, cx, sc3| {
                    let it = g.reload(cx, il);
                    g.cg(cx, body, &sc3, &it, &k2);
                });
                g.bind_pattern(cx, &pats[0], sv, il, sc2.clone(), done);
            });
            return;
        }
        // destructuring alternatives `?//`: all variables exist in every alternative
        let mut names = vec![];
        for p in pats {
            pattern_vars(p, &mut names);
        }
        self.each_value(cx, src, sc, &it.v.clone(), move |g, cx, sv| {
            let svl = g.spill_v(cx, sv);
            let mut sc3 = sc2.clone();
            let mut vars = vec![];
            for n in &names {
                let l = g.slot(cx);
                sc3 = sc3.push(Entry::Var(n.clone(), l));
                vars.push((n.clone(), l));
            }
            g.bind_alternatives(cx, pats, 0, svl, &vars, il, sc3, body, &k);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_alternatives(
        &mut self,
        cx: &mut FnCtx,
        pats: &'a [Pattern],
        i: usize,
        svl: Loc,
        vars: &[(String, Loc)],
        il: ItemLoc,
        sc: Scope<'a>,
        body: &'a Expr,
        k: &K<'a>,
    ) {
        for (_, l) in vars {
            self.store_slot(cx, *l, V, NULL);
        }
        let sv = self.load_slot(cx, svl, V);
        let last = i + 1 == pats.len();
        let k2 = k.clone();
        let done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a> = Rc::new(move |g, cx, sc3| {
            let saved = std::mem::take(&mut g.alt_vars);
            let it = g.reload(cx, il);
            g.cg(cx, body, &sc3, &it, &k2);
            g.alt_vars = saved;
        });
        let prev = std::mem::replace(&mut self.alt_vars, vars.to_vec());
        if last {
            self.bind_pattern(cx, &pats[i], &sv, il, sc, done);
            self.alt_vars = prev;
            return;
        }
        // errors in this alternative (including the body) move on to the next
        let errslot = cx.f.var(V);
        let (lc, lend) = (cx.f.label("alt_catch"), cx.f.label("alt_end"));
        self.ntry += 1;
        let h = Rc::new(Handler::Catch {
            fnid: cx.id,
            try_id: self.ntry,
            depth: cx.cleanups.len(),
            block: lc.clone(),
            errslot: errslot.clone(),
            outer: cx.handler.clone(),
            discard: true,
        });
        let saved = std::mem::replace(&mut cx.handler, h);
        self.bind_pattern(cx, &pats[i], &sv, il, sc.clone(), done);
        self.alt_vars = prev;
        cx.handler = saved;
        cx.f.br(&lend);
        cx.f.block(&lc);
        let e = cx.f.load(V, &errslot);
        Self::release(cx, &e);
        self.bind_alternatives(cx, pats, i + 1, svl, vars, il, sc, body, k);
        cx.f.br(&lend);
        cx.f.block(&lend);
    }

    fn bind_pattern(
        &mut self,
        cx: &mut FnCtx,
        pat: &'a Pattern,
        v: &str,
        il: ItemLoc,
        sc: Scope<'a>,
        done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a>,
    ) {
        match pat {
            Pattern::Var(name) => {
                let l = match self.alt_vars.iter().find(|(n, _)| n == name) {
                    Some((_, l)) => *l,
                    None => self.slot(cx),
                };
                self.store_slot(cx, l, V, v);
                let sc2 = sc.push(Entry::Var(name.clone(), l));
                done(self, cx, sc2);
            }
            Pattern::Array(els) => {
                let vl = self.spill_v(cx, v);
                self.bind_array(cx, els, 0, vl, il, sc, done);
            }
            Pattern::Object(ents) => {
                let vl = self.spill_v(cx, v);
                self.bind_object(cx, ents, 0, vl, il, sc, done);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_array(
        &mut self,
        cx: &mut FnCtx,
        els: &'a [Pattern],
        i: usize,
        vl: Loc,
        il: ItemLoc,
        sc: Scope<'a>,
        done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a>,
    ) {
        if i == els.len() {
            done(self, cx, sc);
            return;
        }
        let v = self.load_slot(cx, vl, V);
        let idx = rt::numconst(i as f64);
        let e = cx.f.call(V, "jv_index", &[(V, &v), (V, &idx)]);
        self.check_err(cx, &e);
        cx.cleanups.push(Cleanup::Val(e.clone()));
        let done2: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a> = Rc::new(move |g, cx, sc2| {
            g.bind_array(cx, els, i + 1, vl, il, sc2, done.clone());
        });
        self.bind_pattern(cx, &els[i], &e, il, sc, done2);
        cx.cleanups.pop();
        Self::release(cx, &e);
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_object(
        &mut self,
        cx: &mut FnCtx,
        ents: &'a [(Expr, Option<String>, Option<Pattern>)],
        i: usize,
        vl: Loc,
        il: ItemLoc,
        sc: Scope<'a>,
        done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a>,
    ) {
        if i == ents.len() {
            done(self, cx, sc);
            return;
        }
        let (ke, var, sub) = &ents[i];
        check_const_key(ke);
        let it = self.reload(cx, il);
        let sc_k = sc.clone();
        self.each_value(cx, ke, &sc_k, &it.v, move |g, cx, key| {
            let isstr = cx.f.tag_is(key, rt::T_STR);
            let (lb, lo) = (cx.f.label("pkey_bad"), cx.f.label("pkey_ok"));
            cx.f.cbr(&isstr, &lo, &lb);
            cx.f.block(&lb);
            let err = cx.f.call(V, "jv_objkey_err", &[(V, key)]);
            g.raise(cx, &err);
            cx.f.br(&lo);
            cx.f.block(&lo);
            let v = g.load_slot(cx, vl, V);
            let e = cx.f.call(V, "jv_index", &[(V, &v), (V, key)]);
            g.check_err(cx, &e);
            cx.cleanups.push(Cleanup::Val(e.clone()));
            let mut sc2 = sc.clone();
            if let Some(name) = var {
                let l = match g.alt_vars.iter().find(|(n, _)| n == name) {
                    Some((_, l)) => *l,
                    None => g.slot(cx),
                };
                g.store_slot(cx, l, V, &e);
                sc2 = sc2.push(Entry::Var(name.clone(), l));
            }
            let done2 = done.clone();
            let next: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a> = Rc::new(move |g, cx, sc3| {
                g.bind_object(cx, ents, i + 1, vl, il, sc3, done2.clone());
            });
            match sub {
                Some(p) => g.bind_pattern(cx, p, &e, il, sc2, next),
                None => next(g, cx, sc2),
            }
            cx.cleanups.pop();
            Self::release(cx, &e);
        });
    }

    // ------------------------------------------------------------------
    // reduce / foreach

    #[allow(clippy::too_many_arguments)]
    fn cg_reduce(
        &mut self,
        cx: &mut FnCtx,
        src: &'a Expr,
        pat: &'a Pattern,
        init: &'a Expr,
        upd: &'a Expr,
        sc: &Scope<'a>,
        it: &Item,
        k: &K<'a>,
    ) {
        let il = self.spill(cx, it);
        let k = k.clone();
        let sc2 = sc.clone();
        let pathmode = it.p.is_some();
        self.each_value(cx, init, sc, &it.v.clone(), move |g, cx, iv| {
            let st = g.slot(cx);
            Self::retain(cx, iv);
            g.store_slot(cx, st, V, iv);
            cx.cleanups.push(Cleanup::Slot(st));
            let it = g.reload(cx, il);
            let sc3 = sc2.clone();
            g.each_value(cx, src, &sc2, &it.v, move |g, cx, sv| {
                let done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a> = Rc::new(move |g, cx, sc4| {
                    g.reduce_step(cx, upd, &sc4, st);
                });
                g.bind_pattern(cx, pat, sv, il, sc3.clone(), done);
            });
            cx.cleanups.pop();
            let v = g.load_slot(cx, st, V);
            let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
            g.emit_value(cx, &fake, &k, &v, true);
        });
    }


    /// In-place `PATH op= E` on a uniquely owned reduce accumulator in slot `st`.
    #[allow(clippy::too_many_arguments)]
    fn reduce_assign(&mut self, cx: &mut FnCtx, op: AssignOp, keys: Vec<&'a Expr>, rhs: &'a Expr, sc: &Scope<'a>, st: Loc) {
        // no output from the key/rhs expressions means the update produced nothing: state := null
        let produced = self.slot(cx);
        self.store_slot(cx, produced, "i64", "0");
        let rhs_first = !matches!(op, AssignOp::Update);
        self.reduce_assign_keys(cx, op, keys, 0, vec![], rhs, rhs_first, sc, st, produced);
        let p = self.load_slot(cx, produced, "i64");
        let none = cx.f.eq(&p, "0");
        let (ln, le) = (cx.f.label("ra_none"), cx.f.label("ra_end"));
        cx.f.cbr(&none, &ln, &le);
        cx.f.block(&ln);
        let s = self.load_slot(cx, st, V);
        Self::release(cx, &s);
        self.store_slot(cx, st, V, NULL);
        cx.f.br(&le);
        cx.f.block(&le);
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_assign_keys(
        &mut self,
        cx: &mut FnCtx,
        op: AssignOp,
        keys: Vec<&'a Expr>,
        i: usize,
        done: Vec<Loc>,
        rhs: &'a Expr,
        rhs_first: bool,
        sc: &Scope<'a>,
        st: Loc,
        produced: Loc,
    ) {
        let cur = self.load_slot(cx, st, V);
        if rhs_first {
            // evaluate the right-hand side (against the current state) before the keys
            let sc2 = sc.clone();
            self.each_value(cx, rhs, sc, &cur, move |g, cx, rv| {
                let rl = g.spill_v(cx, rv);
                g.reduce_assign_keys2(cx, op, keys.clone(), i, done.clone(), Some(rl), rhs, &sc2, st, produced);
            });
        } else {
            self.reduce_assign_keys2(cx, op, keys, i, done, None, rhs, sc, st, produced);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_assign_keys2(
        &mut self,
        cx: &mut FnCtx,
        op: AssignOp,
        keys: Vec<&'a Expr>,
        i: usize,
        done: Vec<Loc>,
        rl: Option<Loc>,
        rhs: &'a Expr,
        sc: &Scope<'a>,
        st: Loc,
        produced: Loc,
    ) {
        if i < keys.len() {
            let cur = self.load_slot(cx, st, V);
            let k = keys[i];
            let sc2 = sc.clone();
            self.each_value(cx, k, sc, &cur, move |g, cx, kv| {
                let kl = g.spill_v(cx, kv);
                let mut d = done.clone();
                d.push(kl);
                g.reduce_assign_keys2(cx, op, keys.clone(), i + 1, d, rl, rhs, &sc2, st, produced);
            });
            return;
        }
        // build the path and update the accumulator in place
        let mut path = cx.f.call(V, "jv_arr", &[("i64", &done.len().max(1).to_string())]);
        for l in &done {
            let kv = self.load_slot(cx, *l, V);
            path = cx.f.call(V, "jv_arr_push", &[(V, &path), (V, &kv)]);
        }
        cx.cleanups.push(Cleanup::Val(path.clone()));
        let s = self.load_slot(cx, st, V);
        match op {
            AssignOp::Update => {
                let old = cx.f.call(V, "jv_getpath", &[(V, &s), (V, &path)]);
                self.check_err(cx, &old);
                cx.cleanups.push(Cleanup::Val(old.clone()));
                // first output of rhs replaces the value; none deletes the path
                let out = self.slot(cx);
                self.store_slot(cx, out, V, "{ i64 10, i64 0 }");
                let idl = self.slot(cx);
                let id = cx.f.call("i64", "jv_new_label", &[]);
                self.store_slot(cx, idl, "i64", &id);
                let done_l = cx.f.label("ra_upd_done");
                let hl = Rc::new(Handler::Label {
                    fnid: cx.id,
                    id: idl,
                    depth: cx.cleanups.len(),
                    block: done_l.clone(),
                    outer: cx.handler.clone(),
                });
                let saved = std::mem::replace(&mut cx.handler, hl.clone());
                let ku = self.closure(&hl, false, move |g, cx, u| {
                    Self::retain(cx, &u.v);
                    g.store_slot(cx, out, V, &u.v);
                    let id = g.load_slot(cx, idl, "i64");
                    let b = cx.f.mkv("9", &id);
                    let h = cx.handler.clone();
                    g.act(cx, &h, &b, false);
                    let l = cx.f.label("after_break");
                    cx.f.block(&l);
                });
                self.cg(cx, rhs, sc, &Item::val(&old), &ku);
                cx.handler = saved;
                cx.f.br(&done_l);
                cx.f.block(&done_l);
                cx.cleanups.pop();
                Self::release(cx, &old);
                let o = self.load_slot(cx, out, V);
                let got = cx.f.tag(&o);
                let none = cx.f.eq(&got, "10");
                let s = self.load_slot(cx, st, V);
                self.store_slot(cx, st, V, NULL);
                let r = cx.f.if_val(
                    &none,
                    V,
                    |f| {
                        let ps = f.call(V, "jv_arr", &[("i64", "1")]);
                        let ps = f.call(V, "jv_arr_push", &[(V, &ps), (V, &path)]);
                        let r = f.call(V, "jv_delpaths_c", &[(V, &s), (V, &ps)]);
                        f.call("void", "jv_release", &[(V, &ps)]);
                        r
                    },
                    |f| {
                        let r = f.call(V, "jv_setpath_c", &[(V, &s), (V, &path), (V, &o)]);
                        f.call("void", "jv_release", &[(V, &o)]);
                        r
                    },
                );
                self.check_err(cx, &r);
                self.store_slot(cx, st, V, &r);
            }
            _ => {
                let rv = self.load_slot(cx, rl.unwrap(), V);
                let nv = match op {
                    AssignOp::Set => {
                        Self::retain(cx, &rv);
                        rv
                    }
                    AssignOp::Alt => {
                        let old = cx.f.call(V, "jv_getpath", &[(V, &s), (V, &path)]);
                        self.check_err(cx, &old);
                        let t = cx.f.tag(&old);
                        let truthy = cx.f.icmp("uge", "i64", &t, "2");
                        let r = cx.f.select(&truthy, V, &old, &rv);
                        Self::retain(cx, &r);
                        Self::release(cx, &old);
                        r
                    }
                    AssignOp::Arith(bop) => {
                        let old = cx.f.call(V, "jv_getpath", &[(V, &s), (V, &path)]);
                        self.check_err(cx, &old);
                        cx.cleanups.push(Cleanup::Val(old.clone()));
                        let f = match bop {
                            BinOp::Add => "jv_add",
                            BinOp::Sub => "jv_sub",
                            BinOp::Mul => "jv_mul",
                            BinOp::Div => "jv_div",
                            _ => "jv_mod",
                        };
                        let r = cx.f.call(V, f, &[(V, &old), (V, &rv)]);
                        self.check_err(cx, &r);
                        cx.cleanups.pop();
                        Self::release(cx, &old);
                        r
                    }
                    AssignOp::Update => unreachable!(),
                };
                cx.cleanups.push(Cleanup::Val(nv.clone()));
                let s = self.load_slot(cx, st, V);
                self.store_slot(cx, st, V, NULL);
                let r = cx.f.call(V, "jv_setpath_c", &[(V, &s), (V, &path), (V, &nv)]);
                self.check_err(cx, &r);
                cx.cleanups.pop();
                Self::release(cx, &nv);
                self.store_slot(cx, st, V, &r);
            }
        }
        cx.cleanups.pop();
        Self::release(cx, &path);
        self.store_slot(cx, produced, "i64", "1");
    }

    fn reduce_step(&mut self, cx: &mut FnCtx, upd: &'a Expr, sc: &Scope<'a>, st: Loc) {
        // Fast path: `. + E` with single-valued E updates the accumulator in place.
        if let Expr::Bin(BinOp::Add, l, r) = upd {
            if matches!(**l, Expr::Identity) && single_valued(r) {
                let cur = self.load_slot(cx, st, V);
                self.each_value(cx, r, sc, &cur, move |g, cx, rv| {
                    let s = g.load_slot(cx, st, V);
                    g.store_slot(cx, st, V, NULL);
                    let n = cx.f.call(V, "jv_add_c2", &[(V, &s), (V, rv)]);
                    g.check_err(cx, &n);
                    g.store_slot(cx, st, V, &n);
                });
                return;
            }
        }
        // Fast path: `PATH op= E` on a simple path (`.a`, `.[$k]`, `.a[$k]`, keys not
        // depending on `.`): the accumulator is uniquely owned, so update it in place.
        if let Expr::Assign(op, lhs, rhs) = upd {
            if let Some(keys) = simple_path(lhs) {
                let keys_ok = keys.iter().all(|k| !refs_dot(k)) && {
                    let ks: Vec<&'a Expr> = keys.clone();
                    ks.iter().all(|k| self.aom(k, sc))
                };
                let rhs_ok = matches!(op, AssignOp::Update) || self.aom(rhs, sc);
                if keys_ok && rhs_ok {
                    self.reduce_assign(cx, *op, keys, rhs, sc, st);
                    return;
                }
            }
        }
        let last = self.slot(cx);
        self.store_slot(cx, last, V, "{ i64 10, i64 0 }");
        cx.cleanups.push(Cleanup::Slot(last));
        let cur = self.load_slot(cx, st, V);
        let h = cx.handler.clone();
        let ku = self.closure(&h, false, move |g, cx, u| {
            let l = g.load_slot(cx, last, V);
            Self::release(cx, &l);
            Self::retain(cx, &u.v);
            g.store_slot(cx, last, V, &u.v);
        });
        self.cg(cx, upd, sc, &Item::val(&cur), &ku);
        cx.cleanups.pop();
        let l = self.load_slot(cx, last, V);
        let old = self.load_slot(cx, st, V);
        Self::release(cx, &old);
        let none = cx.f.tag_is(&l, rt::T_EOF);
        let nv = cx.f.select(&none, V, NULL, &l);
        self.store_slot(cx, st, V, &nv);
    }

    #[allow(clippy::too_many_arguments)]
    fn cg_foreach(
        &mut self,
        cx: &mut FnCtx,
        src: &'a Expr,
        pat: &'a Pattern,
        init: &'a Expr,
        upd: &'a Expr,
        ext: Option<&'a Expr>,
        sc: &Scope<'a>,
        it: &Item,
        k: &K<'a>,
    ) {
        let il = self.spill(cx, it);
        let k = self.share(cx, k);
        let sc2 = sc.clone();
        let pathmode = it.p.is_some();
        self.each_value(cx, init, sc, &it.v.clone(), move |g, cx, iv| {
            let st = g.slot(cx);
            Self::retain(cx, iv);
            g.store_slot(cx, st, V, iv);
            cx.cleanups.push(Cleanup::Slot(st));
            let it = g.reload(cx, il);
            let sc3 = sc2.clone();
            let k2 = k.clone();
            g.each_value(cx, src, &sc2, &it.v, move |g, cx, sv| {
                let k3 = k2.clone();
                let done: Rc<dyn Fn(&mut Gen<'a>, &mut FnCtx, Scope<'a>) + 'a> = Rc::new(move |g, cx, sc4| {
                    // move the state into `base`; outputs of the update become the new state
                    let base = g.slot(cx);
                    let s = g.load_slot(cx, st, V);
                    g.store_slot(cx, base, V, &s);
                    g.store_slot(cx, st, V, "{ i64 10, i64 0 }");
                    cx.cleanups.push(Cleanup::Slot(base));
                    let k4 = k3.clone();
                    let sc5 = sc4.clone();
                    let h = cx.handler.clone();
                    let ku = g.closure(&h, false, move |g, cx, u| {
                        let old = g.load_slot(cx, st, V);
                        Self::release(cx, &old);
                        Self::retain(cx, &u.v);
                        g.store_slot(cx, st, V, &u.v);
                        let fake = Item { v: u.v.clone(), p: if pathmode { Some(String::new()) } else { None } };
                        match ext {
                            Some(x) => {
                                let k5 = k4.clone();
                                g.each_value(cx, x, &sc5, &u.v, move |g, cx, xv| {
                                    let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
                                    g.emit_value(cx, &fake, &k5, xv, false);
                                });
                            }
                            None => g.emit_value(cx, &fake, &k4, &u.v, false),
                        }
                    });
                    let bv = g.load_slot(cx, base, V);
                    g.cg(cx, upd, &sc4, &Item::val(&bv), &ku);
                    cx.cleanups.pop();
                    let s = g.load_slot(cx, st, V);
                    let none = cx.f.tag_is(&s, rt::T_EOF);
                    let bv = g.load_slot(cx, base, V);
                    let (ln, ly, le) = (cx.f.label("fe_none"), cx.f.label("fe_some"), cx.f.label("fe_end"));
                    cx.f.cbr(&none, &ln, &ly);
                    cx.f.block(&ln);
                    g.store_slot(cx, st, V, &bv);
                    cx.f.br(&le);
                    cx.f.block(&ly);
                    Self::release(cx, &bv);
                    cx.f.br(&le);
                    cx.f.block(&le);
                });
                g.bind_pattern(cx, pat, sv, il, sc3.clone(), done);
            });
            cx.cleanups.pop();
            let v = g.load_slot(cx, st, V);
            Self::release(cx, &v);
        });
    }

    // ------------------------------------------------------------------
    // assignment operators

    fn cg_assign(&mut self, cx: &mut FnCtx, op: AssignOp, lhs: &'a Expr, rhs: &'a Expr, sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let pathmode = it.p.is_some();
        let il = self.spill(cx, it);
        let k = k.clone();
        let sc2 = sc.clone();
        match op {
            AssignOp::Update => {
                let k = self.share(cx, &k);
                self.modify(cx, lhs, rhs, None, sc, il, pathmode, &k);
            }
            AssignOp::Set | AssignOp::Arith(_) | AssignOp::Alt => {
                self.each_value(cx, rhs, sc, &it.v.clone(), move |g, cx, rv| {
                    let rl = g.spill_v(cx, rv);
                    g.modify(cx, lhs, rhs, Some((op, rl)), &sc2, il, pathmode, &k);
                });
            }
        }
    }

    /// Walk paths of `lhs` over the input, updating a copy of it.
    /// `with = None`: `lhs |= rhs` (rhs run on the old value; empty deletes).
    /// `with = Some((op, $v))`: set / arithmetic / alternative assignment with value $v.
    #[allow(clippy::too_many_arguments)]
    fn modify(
        &mut self,
        cx: &mut FnCtx,
        lhs: &'a Expr,
        rhs: &'a Expr,
        with: Option<(AssignOp, Loc)>,
        sc: &Scope<'a>,
        il: ItemLoc,
        pathmode: bool,
        k: &K<'a>,
    ) {
        let it = self.reload(cx, il);
        let st = self.slot(cx);
        Self::retain(cx, &it.v);
        self.store_slot(cx, st, V, &it.v);
        cx.cleanups.push(Cleanup::Slot(st));
        let dels = self.slot(cx);
        if with.is_none() {
            let a = cx.f.call(V, "jv_arr", &[("i64", "0")]);
            self.store_slot(cx, dels, V, &a);
            cx.cleanups.push(Cleanup::Slot(dels));
        }
        let root = self.const_json(cx, "[]");
        let sc2 = sc.clone();
        let h = cx.handler.clone();
        let kp = self.closure(&h, true, move |g, cx, x| {
            let p = x.p.clone().unwrap();
            let xv = x.v.clone();
            g.check_path(cx, &p, |f| f.call(V, "jv_path_err", &[(V, &xv)]));
            let s = g.load_slot(cx, st, V);
            let old = cx.f.call(V, "jv_getpath", &[(V, &s), (V, &p)]);
            g.check_err(cx, &old);
            cx.cleanups.push(Cleanup::Val(old.clone()));
            match with {
                None => {
                    // first output of rhs replaces; no output deletes
                    let got = g.slot(cx);
                    g.store_slot(cx, got, "i64", "0");
                    let idl = g.slot(cx);
                    let id = cx.f.call("i64", "jv_new_label", &[]);
                    g.store_slot(cx, idl, "i64", &id);
                    let pl = g.spill_v(cx, &p);
                    let done = cx.f.label("upd_done");
                    let hl = Rc::new(Handler::Label {
                        fnid: cx.id,
                        id: idl,
                        depth: cx.cleanups.len(),
                        block: done.clone(),
                        outer: cx.handler.clone(),
                    });
                    let saved = std::mem::replace(&mut cx.handler, hl.clone());
                    let ku = g.closure(&hl, false, move |g, cx, u| {
                        let s = g.load_slot(cx, st, V);
                        g.store_slot(cx, st, V, NULL);
                        let p = g.load_slot(cx, pl, V);
                        let s2 = cx.f.call(V, "jv_setpath_c", &[(V, &s), (V, &p), (V, &u.v)]);
                        g.check_err(cx, &s2);
                        g.store_slot(cx, st, V, &s2);
                        g.store_slot(cx, got, "i64", "1");
                        let id = g.load_slot(cx, idl, "i64");
                        let b = cx.f.mkv("9", &id);
                        let h = cx.handler.clone();
                        g.act(cx, &h, &b, false);
                        let l = cx.f.label("after_break");
                        cx.f.block(&l);
                    });
                    g.cg(cx, rhs, &sc2, &Item::val(&old), &ku);
                    cx.handler = saved;
                    cx.f.br(&done);
                    cx.f.block(&done);
                    let gv = g.load_slot(cx, got, "i64");
                    let none = cx.f.eq(&gv, "0");
                    let (ld, le) = (cx.f.label("upd_del"), cx.f.label("upd_end"));
                    cx.f.cbr(&none, &ld, &le);
                    cx.f.block(&ld);
                    let d = g.load_slot(cx, dels, V);
                    let p = g.load_slot(cx, pl, V);
                    let d2 = cx.f.call(V, "jv_arr_push", &[(V, &d), (V, &p)]);
                    g.store_slot(cx, dels, V, &d2);
                    cx.f.br(&le);
                    cx.f.block(&le);
                }
                Some((op, rl)) => {
                    let rv = g.load_slot(cx, rl, V);
                    let nv = match op {
                        AssignOp::Set => {
                            Self::retain(cx, &rv);
                            rv
                        }
                        AssignOp::Alt => {
                            let t = cx.f.tag(&old);
                            let truthy = cx.f.icmp("uge", "i64", &t, "2");
                            let r = cx.f.select(&truthy, V, &old, &rv);
                            Self::retain(cx, &r);
                            r
                        }
                        AssignOp::Arith(bop) => {
                            let f = match bop {
                                BinOp::Add => "jv_add",
                                BinOp::Sub => "jv_sub",
                                BinOp::Mul => "jv_mul",
                                BinOp::Div => "jv_div",
                                _ => "jv_mod",
                            };
                            let r = cx.f.call(V, f, &[(V, &old), (V, &rv)]);
                            g.check_err(cx, &r);
                            r
                        }
                        AssignOp::Update => unreachable!(),
                    };
                    cx.cleanups.push(Cleanup::Val(nv.clone()));
                    let s = g.load_slot(cx, st, V);
                    g.store_slot(cx, st, V, NULL);
                    let s2 = cx.f.call(V, "jv_setpath_c", &[(V, &s), (V, &p), (V, &nv)]);
                    g.check_err(cx, &s2);
                    g.store_slot(cx, st, V, &s2);
                    cx.cleanups.pop();
                    Self::release(cx, &nv);
                }
            }
            cx.cleanups.pop();
            Self::release(cx, &old);
        });
        self.cg(cx, lhs, sc, &Item { v: it.v.clone(), p: Some(root) }, &kp);
        if with.is_none() {
            cx.cleanups.pop();
            let s = self.load_slot(cx, st, V);
            self.store_slot(cx, st, V, NULL);
            let d = self.load_slot(cx, dels, V);
            let s2 = cx.f.call(V, "jv_delpaths_c", &[(V, &s), (V, &d)]);
            Self::release(cx, &d);
            self.check_err(cx, &s2);
            self.store_slot(cx, st, V, &s2);
        }
        cx.cleanups.pop();
        let s = self.load_slot(cx, st, V);
        let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
        self.emit_value(cx, &fake, k, &s, true);
    }

    // ------------------------------------------------------------------
    // function calls

    fn cg_call(&mut self, cx: &mut FnCtx, name: &str, args: &'a [Expr], sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        match sc.func(name, args.len()) {
            Some(FuncBind::Inline(e, esc)) => self.cg(cx, e, &esc, it, k),
            Some(FuncBind::Closure(l)) => {
                let cp = self.load_slot(cx, l, "ptr");
                let off = if it.p.is_some() { 8 } else { 0 };
                let code = cx.f.ld_at("ptr", &cp, off);
                let env = cx.f.ld_at("ptr", &cp, 16);
                let (kf, ke) = self.kpair(cx, k);
                let st = match &it.p {
                    Some(p) => cx.f.call(V, &code, &[("ptr", &env), (V, p), (V, &it.v), ("ptr", &kf), ("ptr", &ke)]),
                    None => cx.f.call(V, &code, &[("ptr", &env), (V, &it.v), ("ptr", &kf), ("ptr", &ke)]),
                };
                self.check(cx, &st);
            }
            Some(FuncBind::Def(inst)) => {
                let ptr = Rc::as_ptr(&inst);
                if !inst.recursive && !self.inline_stack.contains(&ptr) && inlinable(inst.def) {
                    self.inline_stack.push(ptr);
                    self.inline_def(cx, &inst, args, sc, it, k);
                    self.inline_stack.pop();
                } else if it.p.is_none() && inst.def.params.iter().all(|p| matches!(p, Param::Value(_))) && self.def_aom(&inst) {
                    let il = self.spill(cx, it);
                    self.direct_args(cx, &inst, args, 0, vec![], il, sc, k);
                } else {
                    self.call_def(cx, &inst, args, sc, it, k);
                }
            }
            None => self.cg_builtin(cx, name, args, sc, it, k),
        }
    }

    fn inline_def(&mut self, cx: &mut FnCtx, inst: &Rc<DefInst<'a>>, args: &'a [Expr], sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let d = inst.def;
        let mut bsc = inst.scope.borrow().push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst.clone())));
        let mut vparams = vec![];
        for (p, a) in d.params.iter().zip(args) {
            match p {
                Param::Filter(n) => bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Inline(a, sc.clone()))),
                Param::Value(n) => {
                    bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Inline(a, sc.clone())));
                    vparams.push((n.clone(), a));
                }
            }
        }
        if vparams.is_empty() {
            self.cg(cx, &d.body, &bsc, it, k);
            return;
        }
        let il = self.spill(cx, it);
        self.bind_values(cx, vparams, 0, sc.clone(), bsc, il, &d.body, k);
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_values(
        &mut self,
        cx: &mut FnCtx,
        vparams: Vec<(String, &'a Expr)>,
        i: usize,
        argsc: Scope<'a>,
        bsc: Scope<'a>,
        il: ItemLoc,
        body: &'a Expr,
        k: &K<'a>,
    ) {
        let it = self.reload(cx, il);
        if i == vparams.len() {
            self.cg(cx, body, &bsc, &it, k);
            return;
        }
        let (n, a) = vparams[i].clone();
        let k = k.clone();
        let argsc2 = argsc.clone();
        self.each_value(cx, a, &argsc, &it.v, move |g, cx, v| {
            let l = g.spill_v(cx, v);
            let bsc2 = bsc.push(Entry::Var(n.clone(), l));
            g.bind_values(cx, vparams.clone(), i + 1, argsc2.clone(), bsc2, il, body, &k);
        });
    }

    /// Build a closure struct {code_v, code_p, env} for a filter argument.
    fn make_closure(&mut self, cx: &mut FnCtx, a: &'a Expr, sc: &Scope<'a>) -> String {
        if let Expr::Call(n, args) = a {
            if args.is_empty() {
                if let Some(FuncBind::Closure(l)) = sc.func(n, 0) {
                    return self.load_slot(cx, l, "ptr");
                }
            }
        }
        let mut names = vec![];
        for path in [false, true] {
            let params: Vec<(&str, &str)> = if path {
                vec![("ptr", "env"), (V, "p"), (V, "x"), ("ptr", "k"), ("ptr", "kenv")]
            } else {
                vec![("ptr", "env"), (V, "x"), ("ptr", "k"), ("ptr", "kenv")]
            };
            let mut c = self.new_fn(Some(cx.id), "arg", &params);
            let kl = self.slot(&mut c);
            let kel = self.slot(&mut c);
            self.store_slot(&mut c, kl, "ptr", "%k");
            self.store_slot(&mut c, kel, "ptr", "%kenv");
            let ret = Rc::new(Cont { h: Rc::new(Handler::Ret), kind: ContKind::Ret { k: kl, kenv: kel }, path, cheap: true });
            let it = Item { v: "%x".into(), p: if path { Some("%p".into()) } else { None } };
            self.cg(&mut c, a, sc, &it, &ret);
            names.push(Self::fn_name(&c));
            self.finish_fn(c);
        }
        let s = cx.f.array_var("i8", 24);
        cx.f.st_at("ptr", &format!("@{}", names[0]), &s, 0);
        cx.f.st_at("ptr", &format!("@{}", names[1]), &s, 8);
        cx.f.st_at("ptr", "%frame", &s, 16);
        s
    }

    fn call_def(&mut self, cx: &mut FnCtx, inst: &Rc<DefInst<'a>>, args: &'a [Expr], sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let path = it.p.is_some();
        let fname = self.compile_def(inst, path);
        let mut cl = vec![];
        for a in args {
            cl.push(self.make_closure(cx, a, sc));
        }
        let env = self.frame_of(cx, inst.definer);
        let (kf, ke) = self.kpair(cx, k);
        let mut cargs: Vec<(&str, &str)> = vec![("ptr", &env)];
        if let Some(p) = &it.p {
            cargs.push((V, p));
        }
        cargs.push((V, &it.v));
        cargs.push(("ptr", &kf));
        cargs.push(("ptr", &ke));
        for c in &cl {
            cargs.push(("ptr", c));
        }
        let st = cx.f.call(V, &format!("@{}", fname), &cargs);
        self.check(cx, &st);
    }


    // ------------------------------------------------------------------
    // direct-style calls for functions with at most one output

    /// Does `e` produce at most one output (or abort)? Optimistic for recursion.
    fn aom(&mut self, e: &'a Expr, sc: &Scope<'a>) -> bool {
        match e {
            Expr::Identity | Expr::Lit(_) | Expr::Var(_) | Expr::Format(_) | Expr::Break(_) => true,
            Expr::Interp(_, parts) => parts.iter().all(|p| match p {
                StrPart::Lit(_) => true,
                StrPart::Expr(x) => self.aom(x, sc),
            }),
            Expr::Index(t, i) => self.aom(t, sc) && self.aom(i, sc),
            Expr::Slice(t, a, b) => {
                self.aom(t, sc) && a.as_deref().map_or(true, |x| self.aom(x, sc)) && b.as_deref().map_or(true, |x| self.aom(x, sc))
            }
            Expr::Iterate(_) | Expr::Comma(..) | Expr::Foreach(..) => false,
            Expr::Try(b, c) => self.aom(b, sc) && c.as_deref().map_or(true, |x| self.aom(x, sc)),
            Expr::Pipe(a, b) | Expr::Bin(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) | Expr::Alt(a, b) => {
                self.aom(a, sc) && self.aom(b, sc)
            }
            Expr::Neg(x) | Expr::Label(_, x) => self.aom(x, sc),
            Expr::Assign(op, _, rhs) => matches!(op, AssignOp::Update) || self.aom(rhs, sc),
            Expr::If(c, t, e) => self.aom(c, sc) && self.aom(t, sc) && e.as_deref().map_or(true, |x| self.aom(x, sc)),
            Expr::Array(_) => true,
            Expr::Object(ents) => ents.iter().all(|(k, v)| self.aom(k, sc) && self.aom(v, sc)),
            Expr::Bind(src, _, body) => self.aom(src, sc) && self.aom(body, sc),
            Expr::Reduce(_, _, init, _) => self.aom(init, sc),
            Expr::Defs(defs, body) => {
                let mut sc2 = sc.clone();
                for d in defs {
                    let inst = Rc::new(DefInst {
                        def: d,
                        scope: RefCell::new(sc2.clone()),
                        definer: usize::MAX,
                        recursive: is_recursive(d),
                        compiled: RefCell::new([None, None]),
                        direct: RefCell::new(None),
                        aom: std::cell::Cell::new(None),
                    });
                    sc2 = sc2.push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst)));
                }
                self.aom(body, &sc2)
            }
            Expr::Call(name, args) => match sc.func(name, args.len()) {
                Some(FuncBind::Inline(x, xsc)) => self.aom(x, &xsc),
                Some(FuncBind::Closure(_)) => false,
                Some(FuncBind::Def(inst)) => {
                    let ptr = Rc::as_ptr(&inst);
                    if self.aom_stack.contains(&ptr) {
                        return true;
                    }
                    let d = inst.def;
                    let value_only = d.params.iter().all(|p| matches!(p, Param::Value(_)));
                    if value_only {
                        if let Some(v) = inst.aom.get() {
                            return v && args.iter().all(|a| self.aom(a, sc));
                        }
                    }
                    let mut bsc = inst.scope.borrow().push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst.clone())));
                    for (p, a) in d.params.iter().zip(args) {
                        let n = match p {
                            Param::Filter(n) | Param::Value(n) => n,
                        };
                        let bound: &'a Expr = if value_only { leak_var(n) } else { a };
                        let bsc_arg = if value_only { bsc.clone() } else { sc.clone() };
                        bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Inline(bound, bsc_arg)));
                    }
                    self.aom_stack.push(ptr);
                    let r = self.aom(&d.body, &bsc);
                    self.aom_stack.pop();
                    if value_only {
                        inst.aom.set(Some(r));
                    }
                    let vargs = d.params.iter().zip(args).all(|(p, a)| matches!(p, Param::Filter(_)) || self.aom(a, sc));
                    r && vargs
                }
                None => match (name.as_str(), args.len()) {
                    ("empty", 0) | ("error", 0) | ("not", 0) | ("input", 0) | ("first", 1) | ("last", 1) => true,
                    ("error", 1) | ("getpath", 1) | ("path", 1) => self.aom(&args[0], sc),
                    (n, a) => {
                        (self.natives.contains_key(&(n, a)) || math1(n, a).is_some()) && args.iter().all(|x| self.aom(x, sc))
                    }
                },
            },
        }
    }

    fn def_aom(&mut self, inst: &Rc<DefInst<'a>>) -> bool {
        if let Some(v) = inst.aom.get() {
            return v;
        }
        let d = inst.def;
        let mut bsc = inst.scope.borrow().push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst.clone())));
        for p in &d.params {
            let n = match p {
                Param::Filter(n) | Param::Value(n) => n,
            };
            bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Inline(leak_var(n), bsc.clone())));
        }
        let ptr = Rc::as_ptr(inst);
        self.aom_stack.push(ptr);
        let r = self.aom(&d.body, &bsc);
        self.aom_stack.pop();
        inst.aom.set(Some(r));
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn direct_args(
        &mut self,
        cx: &mut FnCtx,
        inst: &Rc<DefInst<'a>>,
        args: &'a [Expr],
        i: usize,
        done: Vec<Loc>,
        il: ItemLoc,
        sc: &Scope<'a>,
        k: &K<'a>,
    ) {
        let it = self.reload(cx, il);
        if i == args.len() {
            let fname = self.compile_direct(inst);
            let env = self.frame_of(cx, inst.definer);
            let mut vals = vec![];
            for l in &done {
                vals.push(self.load_slot(cx, *l, V));
            }
            let mut cargs: Vec<(&str, &str)> = vec![("ptr", &env), (V, &it.v)];
            for v in &vals {
                cargs.push((V, v));
            }
            let r = cx.f.call(V, &format!("@{}", fname), &cargs);
            let t = cx.f.tag(&r);
            let empty = cx.f.eq(&t, "10");
            let abort = cx.f.icmp("uge", "i64", &t, "7");
            let (le, lnot, la, lv, lend) =
                (cx.f.label("d_empty"), cx.f.label("d_notempty"), cx.f.label("d_abort"), cx.f.label("d_val"), cx.f.label("d_end"));
            cx.f.cbr(&empty, &le, &lnot);
            cx.f.block(&le);
            cx.f.br(&lend);
            cx.f.block(&lnot);
            cx.f.cbr(&abort, &la, &lv);
            cx.f.block(&la);
            let h = cx.handler.clone();
            self.act(cx, &h, &r, false);
            cx.f.block(&lv);
            self.emit_owned(cx, k, &r, None);
            cx.f.br(&lend);
            cx.f.block(&lend);
            return;
        }
        let k = k.clone();
        let sc2 = sc.clone();
        let inst2 = inst.clone();
        self.each_value(cx, &args[i], sc, &it.v, move |g, cx, v| {
            let l = g.spill_v(cx, v);
            let mut d = done.clone();
            d.push(l);
            g.direct_args(cx, &inst2, args, i + 1, d, il, &sc2, &k);
        });
    }

    /// Compile `def f($a; ...)` as `%V f(env, input, a, ...)` returning its
    /// single output (owned), an abort status, or tag 10 for "no output".
    fn compile_direct(&mut self, inst: &Rc<DefInst<'a>>) -> String {
        if let Some(n) = inst.direct.borrow().as_ref() {
            return n.clone();
        }
        let d = inst.def;
        let pnames: Vec<String> = (0..d.params.len()).map(|i| format!("a{}", i)).collect();
        let was_cold = self.cold;
        self.cold |= is_cold_def(&d.name);
        let mut params: Vec<(&str, &str)> = vec![("ptr", "env"), (V, "x")];
        for n in &pnames {
            params.push((V, n));
        }
        let mut c = self.new_fn(Some(inst.definer), &format!("direct.{}", sanitize(&d.name)), &params);
        let name = Self::fn_name(&c);
        *inst.direct.borrow_mut() = Some(name.clone());
        let mut bsc = inst.scope.borrow().push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst.clone())));
        for (i, p) in d.params.iter().enumerate() {
            let n = match p {
                Param::Filter(n) | Param::Value(n) => n,
            };
            let l = self.slot(&mut c);
            self.store_slot(&mut c, l, V, &format!("%{}", pnames[i]));
            bsc = bsc.push(Entry::Var(n.clone(), l));
            bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Inline(leak_var(n), bsc.clone())));
        }
        let res = self.slot(&mut c);
        self.store_slot(&mut c, res, V, "{ i64 10, i64 0 }");
        c.cleanups.push(Cleanup::Slot(res));
        let h = c.handler.clone();
        let k = self.cheap_closure(&h, false, move |g, cx, x| {
            let old = g.load_slot(cx, res, V);
            Self::release(cx, &old);
            Self::retain(cx, &x.v);
            g.store_slot(cx, res, V, &x.v);
        });
        self.cg(&mut c, &d.body, &bsc, &Item::val("%x"), &k);
        c.cleanups.pop();
        let r = self.load_slot(&mut c, res, V);
        c.f.ret(V, &r);
        self.finish_fn(c);
        self.cold = was_cold;
        name
    }

    fn compile_def(&mut self, inst: &Rc<DefInst<'a>>, path: bool) -> String {
        let idx = path as usize;
        if let Some(n) = &inst.compiled.borrow()[idx] {
            return n.clone();
        }
        let d = inst.def;
        let pnames: Vec<String> = (0..d.params.len()).map(|i| format!("c{}", i)).collect();
        let was_cold = self.cold;
        self.cold |= is_cold_def(&d.name);
        let mut params: Vec<(&str, &str)> = vec![("ptr", "env")];
        if path {
            params.push((V, "p"));
        }
        params.push((V, "x"));
        params.push(("ptr", "k"));
        params.push(("ptr", "kenv"));
        for n in &pnames {
            params.push(("ptr", n));
        }
        let mut c = self.new_fn(Some(inst.definer), &format!("def.{}", sanitize(&d.name)), &params);
        let name = Self::fn_name(&c);
        inst.compiled.borrow_mut()[idx] = Some(name.clone());
        let kl = self.slot(&mut c);
        let kel = self.slot(&mut c);
        self.store_slot(&mut c, kl, "ptr", "%k");
        self.store_slot(&mut c, kel, "ptr", "%kenv");
        let mut bsc = inst.scope.borrow().push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst.clone())));
        let mut vparams = vec![];
        for (i, p) in d.params.iter().enumerate() {
            let l = self.slot(&mut c);
            self.store_slot(&mut c, l, "ptr", &format!("%{}", pnames[i]));
            match p {
                Param::Filter(n) => bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Closure(l))),
                Param::Value(n) => {
                    bsc = bsc.push(Entry::Func(n.clone(), 0, FuncBind::Closure(l)));
                    vparams.push(n.clone());
                }
            }
        }
        let ret = Rc::new(Cont { h: Rc::new(Handler::Ret), kind: ContKind::Ret { k: kl, kenv: kel }, path, cheap: true });
        let it = Item { v: "%x".into(), p: if path { Some("%p".into()) } else { None } };
        if vparams.is_empty() {
            self.cg(&mut c, &d.body, &bsc, &it, &ret);
        } else {
            // `$x` params: evaluate the closure (value mode) and bind each output
            let il = self.spill(&mut c, &it);
            let calls: &'a [Expr] = leak_calls(&vparams);
            let pairs: Vec<(String, &'a Expr)> = vparams.iter().cloned().zip(calls.iter()).collect();
            self.bind_values(&mut c, pairs, 0, bsc.clone(), bsc, il, &d.body, &ret);
        }
        self.finish_fn(c);
        self.cold = was_cold;
        name
    }

    // ------------------------------------------------------------------
    // builtins

    fn cg_builtin(&mut self, cx: &mut FnCtx, name: &str, args: &'a [Expr], sc: &Scope<'a>, it: &Item, k: &K<'a>) {
        let pathmode = it.p.is_some();
        match (name, args.len()) {
            ("empty", 0) => {}
            ("error", 0) => {
                let e = cx.f.call(V, "jv_err", &[(V, &it.v)]);
                self.raise(cx, &e);
            }
            ("error", 1) => {
                self.each_value(cx, &args[0], sc, &it.v.clone(), move |g, cx, m| {
                    let e = cx.f.call(V, "jv_err", &[(V, m)]);
                    g.raise(cx, &e);
                });
            }
            ("not", 0) => {
                let t = cx.f.tag(&it.v);
                let truthy = cx.f.icmp("uge", "i64", &t, "2");
                let r = cx.f.select(&truthy, V, FALSE, TRUE);
                self.emit_value(cx, it, k, &r, false);
            }
            ("path", 1) => {
                let root = self.const_json(cx, "[]");
                let k2 = k.clone();
                let h = cx.handler.clone();
                let kp = self.closure(&h, true, move |g, cx, x| {
                    let xv = x.v.clone();
                    g.check_path(cx, x.p.as_ref().unwrap(), |f| f.call(V, "jv_path_err", &[(V, &xv)]));
                    let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
                    g.emit_value(cx, &fake, &k2, x.p.as_ref().unwrap(), false);
                });
                self.cg(cx, &args[0], sc, &Item { v: it.v.clone(), p: Some(root) }, &kp);
            }
            ("getpath", 1) => {
                let il = self.spill(cx, it);
                let k2 = k.clone();
                self.each_value(cx, &args[0], sc, &it.v.clone(), move |g, cx, pv| {
                    let it = g.reload(cx, il);
                    let r = cx.f.call(V, "jv_getpath", &[(V, &it.v), (V, pv)]);
                    g.check_err(cx, &r);
                    match &it.p {
                        None => g.emit_owned(cx, &k2, &r, None),
                        Some(p) => {
                            cx.cleanups.push(Cleanup::Val(r.clone()));
                            let p2 = cx.f.call(V, "jv_add", &[(V, p), (V, pv)]);
                            g.check_err(cx, &p2);
                            cx.cleanups.push(Cleanup::Val(p2.clone()));
                            g.invoke(cx, &k2, &Item { v: r.clone(), p: Some(p2.clone()) });
                            cx.cleanups.pop();
                            cx.cleanups.pop();
                            Self::release(cx, &p2);
                            Self::release(cx, &r);
                        }
                    }
                });
            }
            ("range", 1) | ("range", 2) | ("range", 3) => {
                let ivl = self.spill_v(cx, &it.v);
                let k2 = k.clone();
                let n = args.len();
                let zero: &'a Expr = &ZERO_EXPR;
                let one: &'a Expr = &ONE_EXPR;
                let (fe, te, be): (&'a Expr, &'a Expr, &'a Expr) = match n {
                    1 => (zero, &args[0], one),
                    2 => (&args[0], &args[1], one),
                    _ => (&args[0], &args[1], &args[2]),
                };
                let sc2 = sc.clone();
                self.each_value(cx, fe, sc, &it.v.clone(), move |g, cx, from| {
                    let fl = g.spill_v(cx, from);
                    let iv = g.load_slot(cx, ivl, V);
                    let k3 = k2.clone();
                    let sc3 = sc2.clone();
                    g.each_value(cx, te, &sc2, &iv, move |g, cx, upto| {
                        let tl = g.spill_v(cx, upto);
                        let iv = g.load_slot(cx, ivl, V);
                        let k4 = k3.clone();
                        g.each_value(cx, be, &sc3, &iv, move |g, cx, by| {
                            let from = g.load_slot(cx, fl, V);
                            let upto = g.load_slot(cx, tl, V);
                            g.range_loop(cx, &from, &upto, by, pathmode, &k4);
                        });
                    });
                });
            }
            ("limit", 2) | ("first", 1) => {
                let (ne, fe): (Option<&'a Expr>, &'a Expr) =
                    if name == "first" { (None, &args[0]) } else { (Some(&args[0]), &args[1]) };
                let il = self.spill(cx, it);
                let k2 = k.clone();
                let sc2 = sc.clone();
                let body = move |g: &mut Gen<'a>, cx: &mut FnCtx, n: &str| {
                    let isnum = cx.f.tag_is(n, rt::T_NUM);
                    let (lbad, lok) = (cx.f.label("lim_bad"), cx.f.label("lim_ok"));
                    cx.f.cbr(&isnum, &lok, &lbad);
                    cx.f.block(&lbad);
                    g.raise_msg(cx, "Invalid limit: must be a number");
                    cx.f.br(&lok);
                    cx.f.block(&lok);
                    let nd = cx.f.num_of(n);
                    let neg = cx.f.fcmp("olt", &nd, "0.0");
                    let (lneg, lpos) = (cx.f.label("lim_neg"), cx.f.label("lim_pos"));
                    cx.f.cbr(&neg, &lneg, &lpos);
                    cx.f.block(&lneg);
                    g.raise_msg(cx, "limit doesn't support negative count");
                    cx.f.br(&lpos);
                    cx.f.block(&lpos);
                    let nl = g.spill_v(cx, n);
                    let pos = cx.f.fcmp("ogt", &nd, "0.0");
                    let (lrun, lend) = (cx.f.label("lim_run"), cx.f.label("lim_end"));
                    cx.f.cbr(&pos, &lrun, &lend);
                    cx.f.block(&lrun);
                    let cnt = g.slot(cx);
                    g.store_slot(cx, cnt, "i64", "0");
                    let idl = g.slot(cx);
                    let id = cx.f.call("i64", "jv_new_label", &[]);
                    g.store_slot(cx, idl, "i64", &id);
                    let done = cx.f.label("lim_done");
                    let hl = Rc::new(Handler::Label {
                        fnid: cx.id,
                        id: idl,
                        depth: cx.cleanups.len(),
                        block: done.clone(),
                        outer: cx.handler.clone(),
                    });
                    let saved = std::mem::replace(&mut cx.handler, hl.clone());
                    let k3 = k2.clone();
                    let kl = g.closure(&hl, pathmode, move |g, cx, x| {
                        g.invoke(cx, &k3, x);
                        let c = g.load_slot(cx, cnt, "i64");
                        let c1 = cx.f.add(&c, "1");
                        g.store_slot(cx, cnt, "i64", &c1);
                        let n = g.load_slot(cx, nl, V);
                        let nd = cx.f.num_of(&n);
                        let cd = cx.f.cast("sitofp", "i64", &c1, "double");
                        let stop = cx.f.fcmp("oge", &cd, &nd);
                        let (ls, lc) = (cx.f.label("lim_stop"), cx.f.label("lim_cont"));
                        cx.f.cbr(&stop, &ls, &lc);
                        cx.f.block(&ls);
                        let id = g.load_slot(cx, idl, "i64");
                        let b = cx.f.mkv("9", &id);
                        let h = cx.handler.clone();
                        g.act(cx, &h, &b, false);
                        cx.f.block(&lc);
                    });
                    let it = g.reload(cx, il);
                    g.cg(cx, fe, &sc2, &it, &kl);
                    cx.handler = saved;
                    cx.f.br(&done);
                    cx.f.block(&done);
                    cx.f.br(&lend);
                    cx.f.block(&lend);
                };
                match ne {
                    None => body(self, cx, &rt::numconst(1.0)),
                    Some(ne) => self.each_value(cx, ne, sc, &it.v.clone(), body),
                }
            }
            ("last", 1) => {
                // jq 1.8: native, yields nothing for an empty generator, not path-transparent
                let found = self.slot(cx);
                self.store_slot(cx, found, V, "{ i64 10, i64 0 }");
                cx.cleanups.push(Cleanup::Slot(found));
                let h = cx.handler.clone();
                let kl = self.cheap_closure(&h, false, move |g, cx, x| {
                    let old = g.load_slot(cx, found, V);
                    Self::release(cx, &old);
                    Self::retain(cx, &x.v);
                    g.store_slot(cx, found, V, &x.v);
                });
                self.cg(cx, &args[0], sc, &Item::val(&it.v), &kl);
                cx.cleanups.pop();
                let v = self.load_slot(cx, found, V);
                let none = cx.f.tag_is(&v, rt::T_EOF);
                let (ln, ly, le) = (cx.f.label("last_none"), cx.f.label("last_some"), cx.f.label("last_end"));
                cx.f.cbr(&none, &ln, &ly);
                cx.f.block(&ly);
                self.emit_value(cx, it, k, &v, true);
                cx.f.br(&le);
                cx.f.block(&ln);
                cx.f.br(&le);
                cx.f.block(&le);
            }
            ("input", 0) => {
                self.uses_input = true;
                let r = cx.f.call(V, "jv_input", &[]);
                self.check_err(cx, &r);
                self.emit_value(cx, it, k, &r, true);
            }
            ("inputs", 0) => {
                self.uses_input = true;
                let (lh, lb, le) = (cx.f.label("inp_h"), cx.f.label("inp_b"), cx.f.label("inp_e"));
                cx.f.br(&lh);
                cx.f.block(&lh);
                let r = cx.f.call(V, "jv_next_input", &[]);
                let eof = cx.f.tag_is(&r, rt::T_EOF);
                cx.f.cbr(&eof, &le, &lb);
                cx.f.block(&lb);
                self.check_err(cx, &r);
                self.emit_value(cx, it, k, &r, true);
                cx.f.br(&lh);
                cx.f.block(&le);
            }
            ("$__loc__", 0) => unreachable!(),
            _ => {
                if let Some(rtf) = self.natives.get(&(name, args.len())).copied() {
                    let il = self.spill(cx, it);
                    self.native_args(cx, rtf, args, 0, vec![], il, sc, k);
                    return;
                }
                if let Some(fname) = math1(name, args.len()) {
                    let r = cx.f.call(V, "jv_math1", &[(V, &it.v), ("ptr", &format!("@{}", fname))]);
                    self.check_err(cx, &r);
                    self.emit_value(cx, it, k, &r, true);
                    return;
                }
                panic!("{}/{} is not defined", name, args.len());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn native_args(
        &mut self,
        cx: &mut FnCtx,
        rtf: &'static str,
        args: &'a [Expr],
        i: usize,
        done: Vec<Loc>,
        il: ItemLoc,
        sc: &Scope<'a>,
        k: &K<'a>,
    ) {
        let it = self.reload(cx, il);
        if i == args.len() {
            let mut vals = vec![];
            for l in &done {
                vals.push(self.load_slot(cx, *l, V));
            }
            let mut cargs: Vec<(&str, &str)> = vec![(V, &it.v)];
            for v in &vals {
                cargs.push((V, v));
            }
            let r = cx.f.call(V, rtf, &cargs);
            self.check_err(cx, &r);
            self.emit_value(cx, &it, k, &r, true);
            return;
        }
        let k = k.clone();
        let sc2 = sc.clone();
        self.each_value(cx, &args[i], sc, &it.v, move |g, cx, v| {
            let l = g.spill_v(cx, v);
            let mut d = done.clone();
            d.push(l);
            g.native_args(cx, rtf, args, i + 1, d, il, &sc2, &k);
        });
    }

    fn range_loop(&mut self, cx: &mut FnCtx, from: &str, upto: &str, by: &str, pathmode: bool, k: &K<'a>) {
        let a = cx.f.tag_is(from, rt::T_NUM);
        let b = cx.f.tag_is(upto, rt::T_NUM);
        let c = cx.f.tag_is(by, rt::T_NUM);
        let ab = cx.f.and1(&a, &b);
        let ok = cx.f.and1(&ab, &c);
        let (lbad, lok) = (cx.f.label("rng_bad"), cx.f.label("rng_ok"));
        cx.f.cbr(&ok, &lok, &lbad);
        cx.f.block(&lbad);
        self.raise_msg(cx, "Range bounds must be numeric");
        cx.f.br(&lok);
        cx.f.block(&lok);
        let fd = cx.f.num_of(from);
        let td = cx.f.num_of(upto);
        let bd = cx.f.num_of(by);
        let xv = cx.f.var("double");
        cx.f.store("double", &fd, &xv);
        let (lh, lb, le) = (cx.f.label("rng_h"), cx.f.label("rng_b"), cx.f.label("rng_e"));
        cx.f.br(&lh);
        cx.f.block(&lh);
        let x = cx.f.load("double", &xv);
        let up = cx.f.fcmp("ogt", &bd, "0.0");
        let c1 = cx.f.fcmp("olt", &x, &td);
        let c2 = cx.f.fcmp("ogt", &x, &td);
        let down = cx.f.fcmp("olt", &bd, "0.0");
        let g1 = cx.f.and1(&up, &c1);
        let g2 = cx.f.and1(&down, &c2);
        let go = cx.f.or1(&g1, &g2);
        cx.f.cbr(&go, &lb, &le);
        cx.f.block(&lb);
        let v = cx.f.mknum(&x);
        let fake = Item { v: String::new(), p: if pathmode { Some(String::new()) } else { None } };
        self.emit_value(cx, &fake, k, &v, false);
        let x2 = cx.f.load("double", &xv);
        let x3 = cx.f.bin("fadd", "double", &x2, &bd);
        cx.f.store("double", &x3, &xv);
        cx.f.br(&lh);
        cx.f.block(&le);
    }

    // ------------------------------------------------------------------
    // entry point

    pub fn compile_program(&mut self, prog: &'a Expr, prelude: &'a [FuncDef]) {
        let mut root = self.new_fn(None, "jq_main", &[("ptr", "env"), (V, "x"), ("ptr", "k"), ("ptr", "kenv")]);
        let kl = self.slot(&mut root);
        let kel = self.slot(&mut root);
        self.store_slot(&mut root, kl, "ptr", "%k");
        self.store_slot(&mut root, kel, "ptr", "%kenv");
        let mut sc = Scope::default();
        let mut insts = vec![];
        for d in prelude {
            let inst = Rc::new(DefInst {
                def: d,
                scope: RefCell::new(Scope::default()),
                definer: root.id,
                recursive: is_recursive(d),
                compiled: RefCell::new([None, None]),
                direct: RefCell::new(None),
                aom: std::cell::Cell::new(None),
            });
            insts.push(inst.clone());
            sc = sc.push(Entry::Func(d.name.clone(), d.params.len(), FuncBind::Def(inst)));
        }
        // builtins are mutually visible
        for inst in &insts {
            *inst.scope.borrow_mut() = sc.clone();
        }
        let ret = Rc::new(Cont { h: Rc::new(Handler::Ret), kind: ContKind::Ret { k: kl, kenv: kel }, path: false, cheap: true });
        self.cg(&mut root, prog, &sc, &Item::val("%x"), &ret);
        let name = Self::fn_name(&root);
        self.finish_fn(root);

        // wrapper with the fixed name the runtime calls
        let mut f = Func::new("internal", V, "jq_program", &[("ptr", "env"), (V, "x"), ("ptr", "k"), ("ptr", "kenv")], "");
        let r = f.call(V, &format!("@{}", name), &[("ptr", "%env"), (V, "%x"), ("ptr", "%k"), ("ptr", "%kenv")]);
        f.ret(V, &r);
        self.m.add(f);

        // constants
        let mut f = Func::new("internal", "void", "jq_init", &[], "");
        let mut consts: Vec<(&String, &String)> = self.consts.iter().collect();
        consts.sort_by(|a, b| a.1.cmp(b.1));
        for (json, g) in consts {
            let s = self.m.cstr(json.as_bytes());
            let sv = f.call(V, "jv_str", &[("ptr", &s), ("i64", &json.len().to_string())]);
            let v = f.call(V, "jv_fromjson", &[(V, &sv)]);
            f.call("void", "jv_release", &[(V, &sv)]);
            f.call("void", "jv_immortal", &[(V, &v)]);
            f.store(V, &v, g);
        }
        f.ret_void();
        self.m.add(f);
        self.m.global(format!("@g_uses_input = internal constant i64 {}", self.uses_input as i64));
    }
}

fn cx_header(f: &Func) -> String {
    f.name().to_string()
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

static NULL_EXPR: Expr = Expr::Lit(Lit::Null);
static ZERO_EXPR: Expr = Expr::Lit(Lit::Num(0.0));
static ONE_EXPR: Expr = Expr::Lit(Lit::Num(1.0));

/// Calls `name` for each value param name (leaked: lives for the compilation).
fn leak_calls(names: &[String]) -> &'static [Expr] {
    let v: Vec<Expr> = names.iter().map(|n| Expr::Call(n.clone(), vec![])).collect();
    Box::leak(v.into_boxed_slice())
}

fn pattern_vars(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Var(n) => {
            if !out.contains(n) {
                out.push(n.clone())
            }
        }
        Pattern::Array(els) => els.iter().for_each(|e| pattern_vars(e, out)),
        Pattern::Object(ents) => {
            for (_, v, sub) in ents {
                if let Some(n) = v {
                    if !out.contains(n) {
                        out.push(n.clone());
                    }
                }
                if let Some(s) = sub {
                    pattern_vars(s, out);
                }
            }
        }
    }
}


/// Expressions that produce exactly one output (or an error).
fn single_valued(e: &Expr) -> bool {
    match e {
        Expr::Identity | Expr::Lit(_) | Expr::Var(_) | Expr::Format(_) => true,
        Expr::Array(_) => true,
        Expr::Object(ents) => ents.iter().all(|(k, v)| single_valued(k) && single_valued(v)),
        Expr::Index(t, i) => single_valued(t) && single_valued(i),
        Expr::Slice(t, a, b) => {
            single_valued(t) && a.as_ref().map_or(true, |x| single_valued(x)) && b.as_ref().map_or(true, |x| single_valued(x))
        }
        Expr::Neg(x) => single_valued(x),
        Expr::Bin(_, a, b) => single_valued(a) && single_valued(b),
        Expr::Pipe(a, b) => single_valued(a) && single_valued(b),
        Expr::Interp(_, parts) => parts.iter().all(|p| match p {
            StrPart::Lit(_) => true,
            StrPart::Expr(e) => single_valued(e),
        }),
        Expr::Call(n, args) if args.is_empty() => matches!(
            n.as_str(),
            "length" | "keys" | "keys_unsorted" | "tostring" | "tojson" | "type" | "not" | "ascii_downcase" | "ascii_upcase"
        ),
        _ => false,
    }
}

/// Does a definition (syntactically) call itself?
fn is_recursive(d: &FuncDef) -> bool {
    let arity = d.params.len();
    for p in &d.params {
        let n = match p {
            Param::Filter(n) | Param::Value(n) => n,
        };
        if *n == d.name && arity == 0 {
            return false;
        }
    }
    calls(&d.body, &d.name, arity)
}

fn calls(e: &Expr, name: &str, arity: usize) -> bool {
    let c = |x: &Expr| calls(x, name, arity);
    match e {
        Expr::Call(n, args) => (n == name && args.len() == arity) || args.iter().any(c),
        Expr::Defs(defs, body) => {
            for d in defs {
                if d.name == name && d.params.len() == arity {
                    // shadowed from here on
                    return false;
                }
                let shadow = d.params.iter().any(|p| matches!(p, Param::Filter(n) | Param::Value(n) if n == name) && arity == 0);
                if !shadow && c(&d.body) {
                    return true;
                }
            }
            c(body)
        }
        Expr::Identity | Expr::Lit(_) | Expr::Var(_) | Expr::Format(_) | Expr::Break(_) => false,
        Expr::Interp(_, parts) => parts.iter().any(|p| matches!(p, StrPart::Expr(x) if c(x))),
        Expr::Index(a, b) | Expr::Pipe(a, b) | Expr::Comma(a, b) | Expr::Bin(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) | Expr::Alt(a, b) | Expr::Assign(_, a, b) => c(a) || c(b),
        Expr::Slice(t, a, b) => c(t) || a.as_ref().map_or(false, |x| c(x)) || b.as_ref().map_or(false, |x| c(x)),
        Expr::Iterate(a) | Expr::Neg(a) | Expr::Label(_, a) => c(a),
        Expr::Try(a, b) => c(a) || b.as_ref().map_or(false, |x| c(x)),
        Expr::If(a, b, d) => c(a) || c(b) || d.as_ref().map_or(false, |x| c(x)),
        Expr::Array(a) => a.as_ref().map_or(false, |x| c(x)),
        Expr::Object(ents) => ents.iter().any(|(k, v)| c(k) || c(v)),
        Expr::Bind(s, pats, b) => c(s) || c(b) || pats.iter().any(|p| pat_calls(p, name, arity)),
        Expr::Reduce(s, p, i, u) => c(s) || c(i) || c(u) || pat_calls(p, name, arity),
        Expr::Foreach(s, p, i, u, x) => c(s) || c(i) || c(u) || x.as_ref().map_or(false, |x| c(x)) || pat_calls(p, name, arity),
    }
}

fn pat_calls(p: &Pattern, name: &str, arity: usize) -> bool {
    match p {
        Pattern::Var(_) => false,
        Pattern::Array(els) => els.iter().any(|e| pat_calls(e, name, arity)),
        Pattern::Object(ents) => ents.iter().any(|(k, _, s)| calls(k, name, arity) || s.as_ref().map_or(false, |s| pat_calls(s, name, arity))),
    }
}

/// One-argument libm functions exposed as jq builtins.
fn math1(name: &str, arity: usize) -> Option<&'static str> {
    if arity != 0 {
        return None;
    }
    const M: &[&str] = &[
        "floor", "sqrt", "ceil", "round", "trunc", "fabs", "exp", "exp2", "exp10", "log", "log2", "log10", "log1p", "expm1",
        "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "cbrt",
        "significand", "logb", "gamma", "lgamma", "tgamma", "lgamma_r", "rint", "nearbyint", "j0", "j1", "y0", "y1",
    ];
    M.iter().find(|m| **m == name).copied()
}

/// jq rejects constant non-string object keys at compile time.
fn check_const_key(e: &Expr) {
    let bad = match e {
        Expr::Lit(Lit::Str(_)) => None,
        Expr::Lit(Lit::Null) => Some("null (null)".to_string()),
        Expr::Lit(Lit::True) => Some("boolean (true)".to_string()),
        Expr::Lit(Lit::False) => Some("boolean (false)".to_string()),
        Expr::Lit(Lit::Num(n)) => Some(format!("number ({})", n)),
        Expr::Lit(Lit::NumText(t)) => Some(format!("number ({})", t)),
        _ => None,
    };
    if let Some(desc) = bad {
        panic!("Cannot use {} as object key", desc);
    }
}

/// `$name` as an expression (leaked: lives for the compilation).
fn leak_var(n: &str) -> &'static Expr {
    Box::leak(Box::new(Expr::Var(n.to_string())))
}

/// Keys of a simple path `.k1[k2]...` (index chain rooted at `.`).
fn simple_path(e: &Expr) -> Option<Vec<&Expr>> {
    match e {
        Expr::Identity => Some(vec![]),
        Expr::Index(base, key) => {
            let mut ks = simple_path(base)?;
            ks.push(key);
            Some(ks)
        }
        _ => None,
    }
}

/// Could evaluating `e` read its input `.`?
fn refs_dot(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Var(_) => false,
        Expr::Pipe(a, _) => refs_dot(a),
        Expr::Index(t, k) => refs_dot(t) || refs_dot(k),
        Expr::Bin(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) | Expr::Alt(a, b) => refs_dot(a) || refs_dot(b),
        Expr::Neg(x) => refs_dot(x),
        Expr::Interp(_, parts) => parts.iter().any(|p| matches!(p, StrPart::Expr(x) if refs_dot(x))),
        Expr::Array(x) => x.as_deref().map_or(false, refs_dot),
        Expr::Object(ents) => ents.iter().any(|(k, v)| refs_dot(k) || refs_dot(v)),
        _ => true,
    }
}

/// Inline only small definitions; large ones (e.g. the regex compiler) are
/// compiled once as functions instead of being copied into every call site.
fn inlinable(d: &FuncDef) -> bool {
    const LIMIT: usize = 60;
    expr_size(&d.body, LIMIT) < LIMIT
}

/// Number of AST nodes in `e`, counting at most up to `cap`.
fn expr_size(e: &Expr, cap: usize) -> usize {
    let mut n = 0usize;
    let mut stack = vec![e];
    while let Some(e) = stack.pop() {
        n += 1;
        if n >= cap {
            return n;
        }
        match e {
            Expr::Identity | Expr::Lit(_) | Expr::Var(_) | Expr::Format(_) | Expr::Break(_) => {}
            Expr::Interp(_, parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        stack.push(x);
                    }
                }
            }
            Expr::Index(a, b) | Expr::Pipe(a, b) | Expr::Comma(a, b) | Expr::Bin(_, a, b) | Expr::And(a, b)
            | Expr::Or(a, b) | Expr::Alt(a, b) | Expr::Assign(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Slice(t, a, b) => {
                stack.push(t);
                stack.extend(a.as_deref());
                stack.extend(b.as_deref());
            }
            Expr::Iterate(a) | Expr::Neg(a) | Expr::Label(_, a) => stack.push(a),
            Expr::Try(a, b) => {
                stack.push(a);
                stack.extend(b.as_deref());
            }
            Expr::If(a, b, c) => {
                stack.push(a);
                stack.push(b);
                stack.extend(c.as_deref());
            }
            Expr::Array(a) => stack.extend(a.as_deref()),
            Expr::Object(ents) => {
                for (k, v) in ents {
                    stack.push(k);
                    stack.push(v);
                }
            }
            Expr::Bind(s, _, b) => {
                stack.push(s);
                stack.push(b);
            }
            Expr::Reduce(s, _, i, u) => {
                stack.push(s);
                stack.push(i);
                stack.push(u);
            }
            Expr::Foreach(s, _, i, u, x) => {
                stack.push(s);
                stack.push(i);
                stack.push(u);
                stack.extend(x.as_deref());
            }
            Expr::Defs(defs, b) => {
                for d in defs {
                    stack.push(&d.body);
                }
                stack.push(b);
            }
            Expr::Call(_, args) => stack.extend(args.iter()),
        }
    }
    n
}

/// Definitions whose code runs rarely (the regex compiler runs once per
/// distinct pattern) and is large: compiled without optimisation.
fn is_cold_def(name: &str) -> bool {
    name.starts_with("_re_") && name != "_re_prog"
}
