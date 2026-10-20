//! jq-llvm — compile jq filters to native executables via LLVM.

mod ast;
mod codegen;
mod ir;
mod parser;
mod rt;

use std::path::PathBuf;
use std::process::{exit, Command};

const USAGE: &str = "\
usage: jq-llvm [options] <filter>
       jq-llvm [options] -f <file.jq>

Compiles a jq program to a native executable. The executable accepts the
usual jq options (-n -r -c -s -e -S -R --arg --argjson --tab --indent ...)
and reads JSON from files or stdin.

options:
  -o <path>       output executable (default: ./a.out)
  -f <file>       read the filter from a file
  --emit-llvm     write textual LLVM IR to the output path instead
  -O0..-O3        optimisation level passed to clang (default -O2)
  -L <dir>        search directory for `include \"name\";` (also the -f file's
                  directory and the current directory)
  --clang <path>  clang to use (default: $JQC_CLANG or `clang` on PATH)
  --run ...       compile to a temporary file and run it; remaining
                  arguments are passed to the program
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out: Option<String> = None;
    let mut filter: Option<String> = None;
    let mut emit_llvm = false;
    let mut opt = "-O2".to_string();
    let mut clang = std::env::var("JQC_CLANG").unwrap_or_else(|_| "clang".into());
    let mut run: Option<Vec<String>> = None;
    let mut lib_dirs: Vec<PathBuf> = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                print!("{}", USAGE);
                return;
            }
            "-o" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "-f" => {
                i += 1;
                let path = args.get(i).unwrap_or_else(|| die("-f needs a file"));
                filter = Some(std::fs::read_to_string(path).unwrap_or_else(|e| die(&format!("{}: {}", path, e))));
                if let Some(dir) = std::path::Path::new(path).parent() {
                    lib_dirs.insert(0, dir.to_path_buf());
                }
            }
            "-L" => {
                i += 1;
                lib_dirs.push(args.get(i).unwrap_or_else(|| die("-L needs a directory")).into());
            }
            s if s.starts_with("-L") && s.len() > 2 => lib_dirs.push(s[2..].into()),
            "--emit-llvm" => emit_llvm = true,
            "--clang" => {
                i += 1;
                clang = args.get(i).cloned().unwrap_or_else(|| die("--clang needs a path"));
            }
            "--run" => {
                run = Some(args[i + 1..].to_vec());
                break;
            }
            s if s.starts_with("-O") && s.len() == 3 => opt = s.to_string(),
            s => {
                if filter.is_some() {
                    die(&format!("unexpected argument: {}", s));
                }
                filter = Some(s.to_string());
            }
        }
        i += 1;
    }
    let Some(filter) = filter else {
        eprint!("{}", USAGE);
        exit(2);
    };

    lib_dirs.push(".".into());
    let ir = compile(&filter, &lib_dirs).unwrap_or_else(|e| {
        eprintln!("jq-llvm: error: {}", e);
        exit(3);
    });

    if emit_llvm {
        let path = out.unwrap_or_else(|| "out.ll".into());
        std::fs::write(&path, ir).unwrap_or_else(|e| die(&format!("{}: {}", path, e)));
        return;
    }

    let tmpdir = std::env::temp_dir().join(format!("jq-llvm-{}", std::process::id()));
    std::fs::create_dir_all(&tmpdir).unwrap_or_else(|e| die(&e.to_string()));
    let ll = tmpdir.join("prog.ll");
    std::fs::write(&ll, ir).unwrap_or_else(|e| die(&e.to_string()));
    let exe: PathBuf = match (&out, &run) {
        (Some(o), _) => o.into(),
        (None, Some(_)) => tmpdir.join("prog"),
        (None, None) => "a.out".into(),
    };
    let status = Command::new(&clang)
        .arg(&opt)
        .arg("-w")
        .arg(&ll)
        .arg("-o")
        .arg(&exe)
        .arg("-lm")
        .arg("-lpthread")
        .status()
        .unwrap_or_else(|e| die(&format!("failed to run {}: {}", clang, e)));
    if !status.success() {
        die("clang failed");
    }
    if let Some(rargs) = run {
        let st = Command::new(&exe).args(&rargs).status().unwrap_or_else(|e| die(&e.to_string()));
        let _ = std::fs::remove_dir_all(&tmpdir);
        exit(st.code().unwrap_or(1));
    }
    let _ = std::fs::remove_dir_all(&tmpdir);
}

fn die(msg: &str) -> ! {
    eprintln!("jq-llvm: {}", msg);
    exit(2)
}

pub fn compile(filter: &str, lib_dirs: &[PathBuf]) -> Result<String, String> {
    let (includes, mut prog) = parser::parse_program(filter)?;
    // `include "path";` makes a library's definitions visible to the program
    let mut defs = vec![];
    let mut seen = std::collections::HashSet::new();
    for inc in &includes {
        load_library(inc, lib_dirs, &mut defs, &mut seen)?;
    }
    if !defs.is_empty() {
        prog = ast::Expr::Defs(defs, Box::new(prog));
    }
    let prelude = parser::parse_defs(concat!(include_str!("prelude.jq"), include_str!("regex.jq"))).map_err(|e| format!("prelude: {}", e))?;
    let prog: &'static ast::Expr = Box::leak(Box::new(prog));
    let prelude: &'static [ast::FuncDef] = Box::leak(prelude.into_boxed_slice());
    let mut g = codegen::Gen::new();
    rt::emit_runtime(&mut g.m);
    // codegen reports undefined functions/labels by panicking; turn that into an error
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        g.compile_program(prog, prelude);
        g
    }));
    match res {
        Ok(mut g) => {
            g.m.declare("llvm.expect.i1", "declare i1 @llvm.expect.i1(i1, i1)");
            Ok(g.m.render(""))
        }
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "internal compiler error".into());
            Err(msg)
        }
    }
}

/// Find `<name>.jq` (or `<name>`) in the search path and append its definitions
/// (after those of the libraries it includes itself).
fn load_library(
    name: &str,
    lib_dirs: &[PathBuf],
    defs: &mut Vec<ast::FuncDef>,
    seen: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), String> {
    let candidates = |d: &PathBuf| [d.join(format!("{}.jq", name)), d.join(name)];
    let path = lib_dirs
        .iter()
        .flat_map(candidates)
        .find(|p| p.is_file())
        .ok_or_else(|| format!("module not found: {}", name))?;
    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
    if !seen.insert(canon) {
        return Ok(());
    }
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let (includes, lib) = parser::parse_library(&src).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut dirs = vec![path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| ".".into())];
    dirs.extend_from_slice(lib_dirs);
    for inc in &includes {
        load_library(inc, &dirs, defs, seen)?;
    }
    defs.extend(lib);
    Ok(())
}
