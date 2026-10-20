//! The jq runtime, emitted as LLVM IR. Compiled programs link only libc/libm.
//!
//! Value representation: `%V = { i64 tag, i64 bits }`.
//!   tag 0 null, 1 false, 2 true, 3 number (bits = f64), 4 string, 5 array,
//!   6 object, 7 error, 8 bypass (try-escape wrapper), 9 break (bits = label id),
//!   10 eof (input exhausted; never a user value).
//! Tags 4..=8 are heap objects: bits is a pointer whose first i64 is a refcount.
//!
//! Ownership convention: runtime functions *borrow* their %V arguments and
//! return an *owned* result, unless documented as consuming (`_c` suffix or
//! "owned" in the comment).

pub mod core;
pub mod json;
pub mod ops;
pub mod builtins;
pub mod main;
pub mod regex;
pub mod parse;
pub mod input;

use crate::ir::Module;

pub const NULL: &str = "{ i64 0, i64 0 }";
pub const FALSE: &str = "{ i64 1, i64 0 }";
pub const TRUE: &str = "{ i64 2, i64 0 }";
pub const OK: &str = NULL;

pub const T_NULL: i64 = 0;
pub const T_TRUE: i64 = 2;
pub const T_NUM: i64 = 3;
pub const T_STR: i64 = 4;
pub const T_ARR: i64 = 5;
pub const T_OBJ: i64 = 6;
pub const T_ERR: i64 = 7;
pub const T_BYPASS: i64 = 8;
pub const T_EOF: i64 = 10;

pub fn numconst(x: f64) -> String {
    format!("{{ i64 3, i64 {} }}", x.to_bits() as i64)
}

pub fn emit_runtime(m: &mut Module) {
    declare_libc(m);
    core::emit(m);
    json::emit(m);
    ops::emit(m);
    builtins::emit(m);
    main::emit(m);
    input::emit(m);
}

fn declare_libc(m: &mut Module) {
    let decls = [
        ("malloc", "declare noalias ptr @malloc(i64)"),
        ("calloc", "declare noalias ptr @calloc(i64, i64)"),
        ("realloc", "declare ptr @realloc(ptr, i64)"),
        ("free", "declare void @free(ptr)"),
        ("memcpy", "declare ptr @memcpy(ptr, ptr, i64)"),
        ("memmove", "declare ptr @memmove(ptr, ptr, i64)"),
        ("memcmp", "declare i32 @memcmp(ptr, ptr, i64)"),
        ("memchr", "declare ptr @memchr(ptr, i32, i64)"),
        ("memset", "declare ptr @memset(ptr, i32, i64)"),
        ("strlen", "declare i64 @strlen(ptr)"),
        ("strcmp", "declare i32 @strcmp(ptr, ptr)"),
        ("strtod", "declare double @strtod(ptr, ptr)"),
        ("snprintf", "declare i32 @snprintf(ptr, i64, ptr, ...)"),
        ("fwrite", "declare i64 @fwrite(ptr, i64, i64, ptr)"),
        ("fread", "declare i64 @fread(ptr, i64, i64, ptr)"),
        ("fopen", "declare ptr @fopen(ptr, ptr)"),
        ("fclose", "declare i32 @fclose(ptr)"),
        ("fflush", "declare i32 @fflush(ptr)"),
        ("exit", "declare void @exit(i32) noreturn"),
        ("abort", "declare void @abort() noreturn"),
        ("getenv", "declare ptr @getenv(ptr)"),
        ("isatty", "declare i32 @isatty(i32)"),
        ("__errno_location", "declare ptr @__errno_location()"),
        ("strerror", "declare ptr @strerror(i32)"),
        ("fputs", "declare i32 @fputs(ptr, ptr)"),
        ("write", "declare i64 @write(i32, ptr, i64)"),
        ("stdout", "@stdout = external global ptr"),
        ("stderr", "@stderr = external global ptr"),
        ("stdin", "@stdin = external global ptr"),
        ("environ", "@environ = external global ptr"),
        ("floor", "declare double @floor(double)"),
        ("ceil", "declare double @ceil(double)"),
        ("fmod", "declare double @fmod(double, double)"),
        ("pow", "declare double @pow(double, double)"),
        ("log", "declare double @log(double)"),
        ("trunc", "declare double @trunc(double)"),
    ];
    for (n, d) in decls {
        m.declare(n, d);
    }
}
