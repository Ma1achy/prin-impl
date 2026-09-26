//! QA tests for TASK-M0-01, R-189 (`decisions.md` § "R-189 — kernel and ledger `src/` use neither `#[path]` nor
//! `include!`"), written from REQ-SYS-004's verify detail ("in kernel and ledger src/, #[path] (including under
//! cfg_attr) and include! are forbidden, and cargo xtask deps fails on any occurrence, naming the file and line
//! (R-189)") and R-189's purpose ("The R-187 scan then covers the `.rs` files under `src/` only"): the rule holds
//! only if no file outside `src/` joins kernel or ledger through `include!` or `#[path]`, however they are spelled.
//! Not written from the implementation.
//!
//! Where a case depends on rustc, a premise check compiles the crate with rustc and shows it loads the file outside
//! `src/` through the spelling under test, so the expectation is rustc's. Each failing case has a control (R-176):
//! the same shape with the forbidden macro or attribute swapped for an allowed one passes. Scope controls: R-189
//! binds kernel and ledger `src/` only, so `include!` in engine `src/` and in kernel `tests/` pass.
#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const CRATES: [&str; 8] = ["kernel", "ledger", "engine", "render", "gui", "validation", "prin", "xtask"];

/// The §7.1 graph as the stub crates realise it: (from, to, kind) with kind `None` normal.
const BASELINE: [(&str, &str, Option<&str>); 11] = [
    ("kernel", "ledger", Some("build")),
    ("render", "ledger", None),
    ("engine", "ledger", None),
    ("engine", "kernel", None),
    ("engine", "render", None),
    ("gui", "engine", None),
    ("prin", "engine", None),
    ("validation", "ledger", None),
    ("validation", "kernel", None),
    ("validation", "engine", None),
    ("validation", "render", None),
];

/// Builds the workspace under a fresh directory; `krate` takes a dev-dependency on `validation` (allowed by
/// R-187). `files` are paths relative to `crates/` (so `kernel/src/lib.rs`, or `gen/t.rs` outside every crate).
/// Returns (the metadata file, the `crates/` directory).
fn workspace(case: &str, krate: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("qa_TASK-M0-01_r189").join(case);
    let _ = std::fs::remove_dir_all(&root);
    let crates = root.join("crates");
    let dir = |name: &str| crates.join(name);
    for name in CRATES {
        std::fs::create_dir_all(dir(name).join("src")).unwrap();
        std::fs::write(dir(name).join("src/lib.rs"), "pub fn stub() {}\n").unwrap();
    }
    for (rel, text) in files {
        let path = crates.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let id = |name: &str| format!("path+file://{}#0.1.0", dir(name).display());
    let dep = |to: &str, kind: Option<&str>| {
        json!({
            "name": to, "source": null, "req": "*", "kind": kind, "rename": null, "optional": false,
            "uses_default_features": true, "features": [], "target": null, "registry": null,
            "path": dir(to).display().to_string()
        })
    };
    let packages: Vec<Value> = CRATES
        .iter()
        .map(|name| {
            let mut deps: Vec<Value> = BASELINE.iter().filter(|e| e.0 == *name).map(|e| dep(e.1, e.2)).collect();
            if *name == krate {
                deps.push(dep("validation", Some("dev")));
            }
            let lib = json!({
                "name": name, "kind": ["lib"], "crate_types": ["lib"], "required-features": [],
                "src_path": dir(name).join("src/lib.rs").display().to_string(), "edition": "2021",
                "doctest": true, "test": true, "doc": true
            });
            json!({
                "name": name, "version": "0.1.0", "id": id(name), "license": null, "source": null,
                "dependencies": deps, "targets": [lib], "features": {},
                "manifest_path": dir(name).join("Cargo.toml").display().to_string(), "edition": "2021",
                "metadata": null, "publish": [], "authors": []
            })
        })
        .collect();
    let members: Vec<String> = CRATES.iter().map(|n| id(n)).collect();
    let doc = json!({
        "packages": packages, "workspace_members": members, "workspace_default_members": members,
        "resolve": null, "target_directory": root.join("target").display().to_string(), "version": 1,
        "workspace_root": root.display().to_string(), "metadata": null
    });
    let path = root.join("metadata.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    (path, crates)
}

/// Runs `xtask deps --metadata <file>`; returns (success, stdout + stderr).
fn run(path: &Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["deps", "--metadata"])
        .arg(path)
        .output()
        .expect("run xtask");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// Premise check: compiling `<crates>/<krate>/src/lib.rs` with rustc (as a test build, no `validation` extern
/// given) reports the unresolved `validation` in the file `expected` (a path suffix as rustc prints it), so rustc
/// loads that file into the crate.
fn rustc_loads(crates: &Path, krate: &str, expected: &str) {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let out = Command::new(rustc)
        .current_dir(crates.join(krate))
        .args(["--edition", "2021", "--crate-type", "lib", "--test", "--emit", "metadata", "-o"])
        .arg(crates.join(krate).join("qa_out.rmeta"))
        .arg("src/lib.rs")
        .output()
        .expect("run rustc");
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && text.contains("validation") && text.contains(expected),
        "premise: rustc does not load {expected} with its use of validation:\n{text}"
    );
}

const USE_IT: &str = "#[cfg(test)]\nfn t() { validation::run(); }\n";

/// Asserts `with` fails, with a line of output naming `file` (a path suffix under `crates/`) followed by `:<line>`
/// and citing R-189; and `without` passes.
fn fails_naming_and_control(case: &str, krate: &str, with: &[(&str, &str)], without: &[(&str, &str)], file: &str) {
    let (meta, _) = workspace(case, krate, with);
    let (ok, text) = run(&meta);
    assert!(!ok, "{case}: {krate} src/ loads a file outside src/ through include! or #[path], and xtask deps passes (R-189):\n{text}");
    let named = text.lines().any(|l| {
        l.contains("R-189")
            && l.find(&format!("{file}:")).is_some_and(|i| {
                l[i + file.len() + 1..].starts_with(|c: char| c.is_ascii_digit())
            })
    });
    assert!(named, "{case}: no failure names {file} and a line, citing R-189:\n{text}");
    let (meta, _) = workspace(&format!("{case}_control"), krate, without);
    let (ok, text) = run(&meta);
    assert!(ok, "{case}: control fails:\n{text}");
}

/// `include!` imported under another name (`use std::include as inc;`, `use core::{include as i2};`) is still
/// `include!`: rustc loads the file (premise), so it must fail. Control: `stringify!` imported the same way.
#[test]
fn qa_include_imported_under_another_name_fails() {
    let cases = [
        ("alias_std", "use std::include as inc;\ninc!(\"../gen/t.rs\");\n"),
        ("alias_core_group", "use core::{include as i2};\ni2! { \"../gen/t.rs\" }\n"),
    ];
    for (case, lib) in cases {
        let files = [("kernel/src/lib.rs", lib), ("kernel/gen/t.rs", USE_IT)];
        let (_, crates) = workspace(&format!("{case}_premise"), "kernel", &files);
        rustc_loads(&crates, "kernel", "gen/t.rs");
        let control = lib.replace("include", "stringify");
        fails_naming_and_control(
            case,
            "kernel",
            &files,
            &[("kernel/src/lib.rs", control.as_str()), ("kernel/gen/t.rs", USE_IT)],
            "kernel/src/lib.rs",
        );
    }
}

/// `include` passed to a `macro_rules!` as an `ident` fragment and invoked as `$i!(…)`: rustc loads the file
/// (premise), so it must fail. Control: `stringify` passed the same way.
#[test]
fn qa_include_passed_as_a_macro_ident_fails() {
    let lib = "macro_rules! m {\n    ($i:ident) => { $i!(\"../gen/t.rs\"); };\n}\nm!(include);\n";
    let files = [("kernel/src/lib.rs", lib), ("kernel/gen/t.rs", USE_IT)];
    let (_, crates) = workspace("ident_premise", "kernel", &files);
    rustc_loads(&crates, "kernel", "gen/t.rs");
    let control = lib.replace("m!(include)", "m!(stringify)");
    fails_naming_and_control(
        "ident",
        "kernel",
        &files,
        &[("kernel/src/lib.rs", control.as_str()), ("kernel/gen/t.rs", USE_IT)],
        "kernel/src/lib.rs",
    );
}

/// `path = "…"` passed to a `macro_rules!` as a `meta` fragment and expanded as `#[$a] mod t;` is a `#[path]`
/// attribute: rustc loads the file (premise), so it must fail. Control: `allow(dead_code)` passed the same way.
#[test]
fn qa_path_passed_as_a_meta_fragment_fails() {
    let lib = "macro_rules! m {\n    ($a:meta) => { #[$a] mod t; };\n}\nm!(path = \"../gen/t.rs\");\n";
    let files = [("ledger/src/lib.rs", lib), ("ledger/src/t.rs", ""), ("ledger/gen/t.rs", USE_IT)];
    let (_, crates) = workspace("meta_premise", "ledger", &files);
    rustc_loads(&crates, "ledger", "gen/t.rs");
    let control = lib.replace("path = \"../gen/t.rs\"", "allow(dead_code)");
    fails_naming_and_control(
        "meta",
        "ledger",
        &files,
        &[("ledger/src/lib.rs", control.as_str()), ("ledger/src/t.rs", ""), ("ledger/gen/t.rs", USE_IT)],
        "ledger/src/lib.rs",
    );
}

/// R-189 names `cfg_attr` explicitly; ledger as well as kernel. Control: the `cfg_attr` carrying another attribute.
#[test]
fn qa_ledger_cfg_attr_path_fails() {
    let lib = "pub fn f() {}\n#[cfg_attr(test, path = \"../gen/t.rs\")]\nmod t;\n";
    let files = [("ledger/src/lib.rs", lib), ("ledger/src/t.rs", ""), ("ledger/gen/t.rs", USE_IT)];
    let control = lib.replace("path = \"../gen/t.rs\"", "allow(dead_code)");
    fails_naming_and_control(
        "ledger_cfg_attr",
        "ledger",
        &files,
        &[("ledger/src/lib.rs", control.as_str()), ("ledger/src/t.rs", ""), ("ledger/gen/t.rs", USE_IT)],
        "ledger/src/lib.rs",
    );
}

/// The failure names the file the `include!` is in when that is a nested module file, not the crate root, and its
/// line. Control: the module without the `include!`.
#[test]
fn qa_include_in_a_nested_module_file_names_that_file_and_line() {
    let files = |b: &'static str| {
        vec![("kernel/src/lib.rs", "mod a;\n"), ("kernel/src/a/mod.rs", "mod b;\n"), ("kernel/src/a/b.rs", b)]
    };
    let with = "pub fn f() {}\n\ninclude!(\"x.rs\");\n";
    let (meta, _) = workspace("nested", "kernel", &[files(with), vec![("kernel/src/a/x.rs", "")]].concat());
    let (ok, text) = run(&meta);
    assert!(!ok, "an include! in kernel src/a/b.rs passes:\n{text}");
    assert!(
        text.lines().any(|l| l.contains("kernel/src/a/b.rs:3") && l.contains("include!") && l.contains("R-189")),
        "the failure does not name kernel/src/a/b.rs:3:\n{text}"
    );
    let (meta, _) = workspace("nested_control", "kernel", &[files("pub fn f() {}\n"), vec![("kernel/src/a/x.rs", "")]].concat());
    let (ok, text) = run(&meta);
    assert!(ok, "control, the module without the include!, fails:\n{text}");
}

/// Scope: R-189 binds kernel and ledger `src/` only. `include!` and `#[path]` in engine `src/`, and `include!` in
/// kernel's `tests/`, pass. (Their counterparts in kernel `src/` fail: see the tests above and the round 5 file.)
#[test]
fn qa_include_and_path_outside_kernel_and_ledger_src_pass() {
    let engine = [
        ("engine/src/lib.rs", "include!(\"../gen/t.rs\");\n#[path = \"../gen/u.rs\"]\nmod u;\n"),
        ("engine/gen/t.rs", "pub fn t() {}\n"),
        ("engine/gen/u.rs", "pub fn u() {}\n"),
    ];
    let (meta, _) = workspace("scope_engine", "kernel", &engine);
    let (ok, text) = run(&meta);
    assert!(ok, "include!/#[path] in engine src/ fails xtask deps, which R-189 does not ask:\n{text}");
    let tests = [("kernel/tests/it.rs", "include!(\"../gen/t.rs\");\n"), ("kernel/gen/t.rs", "fn t() {}\n")];
    let (meta, _) = workspace("scope_kernel_tests", "kernel", &tests);
    let (ok, text) = run(&meta);
    assert!(ok, "include! in kernel tests/ fails xtask deps, which R-189 does not ask:\n{text}");
    // Negative control for this test's harness: the same include! moved to kernel src/ fails.
    let (meta, _) = workspace("scope_kernel_src", "kernel", &[("kernel/src/lib.rs", tests[0].1), tests[1]]);
    let (ok, text) = run(&meta);
    assert!(!ok, "include! in kernel src/ passes:\n{text}");
}
