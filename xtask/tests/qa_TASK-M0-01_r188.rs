//! QA tests for TASK-M0-01, round 5: R-188, written from REQ-SYS-004's verify detail ("in kernel and ledger no
//! source under src/ uses validation (R-187), counting every file loaded through #[path] or include!, and an
//! include! path that is not a string literal, or does not resolve, fails (R-188)") and `decisions.md` § "R-188"
//! item 2 ("`cargo xtask deps` follows `include!` string paths as it follows `#[path]`, and scans the file; a path
//! it can't resolve ... fails the check") — not from the implementation.
//!
//! "Loaded" means loaded by rustc. Where a case depends on how rustc resolves a path, the test first compiles the
//! crate's `src/lib.rs` with `rustc` and asserts rustc reads the file the case names (a premise check), so the
//! expectation is rustc's, not the checker's. Each failing case has a control (R-176): the same layout with the use
//! of `validation` removed passes.
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("qa_TASK-M0-01_r188").join(case);
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
const PLAIN: &str = "#[cfg(test)]\nfn t() {}\n";

/// Asserts `with` (a layout whose file `named` uses validation) fails naming `named`, and `without` passes.
fn fails_and_control(case: &str, krate: &str, with: &[(&str, &str)], without: &[(&str, &str)], named: &str) {
    let (meta, _) = workspace(case, krate, with);
    let (ok, text) = run(&meta);
    assert!(!ok, "{case}: a use of validation in a file rustc loads through include! or #[path] passes xtask deps:\n{text}");
    assert!(text.contains(named), "{case}: the failure does not name {named}:\n{text}");
    let (meta, _) = workspace(&format!("{case}_control"), krate, without);
    let (ok, text) = run(&meta);
    assert!(ok, "{case}: control, the same layout without the use, fails:\n{text}");
}

/// R-188 applies to ledger as to kernel: a use in a file outside src/ that ledger's src/lib.rs includes fails.
#[test]
fn qa_ledger_include_of_a_file_outside_src_is_scanned() {
    let lib = ("ledger/src/lib.rs", "include!(\"../gen/t.rs\");\n");
    let (_, crates) = workspace("ledger_premise", "ledger", &[lib, ("ledger/gen/t.rs", USE_IT)]);
    rustc_loads(&crates, "ledger", "gen/t.rs");
    fails_and_control(
        "ledger_outside",
        "ledger",
        &[lib, ("ledger/gen/t.rs", USE_IT)],
        &[lib, ("ledger/gen/t.rs", PLAIN)],
        "gen/t.rs",
    );
}

/// An included file need not end in `.rs`: rustc parses whatever file the literal names.
#[test]
fn qa_include_of_a_file_without_rs_extension_is_scanned() {
    let lib = ("kernel/src/lib.rs", "include!(\"../gen/tests.in\");\n");
    let (_, crates) = workspace("ext_premise", "kernel", &[lib, ("kernel/gen/tests.in", USE_IT)]);
    rustc_loads(&crates, "kernel", "gen/tests.in");
    fails_and_control(
        "ext",
        "kernel",
        &[lib, ("kernel/gen/tests.in", USE_IT)],
        &[lib, ("kernel/gen/tests.in", PLAIN)],
        "gen/tests.in",
    );
}

/// A use inside an inline `mod tests { … }` of the included file, and a `mod` the included file declares (resolved
/// by rustc relative to the included file), are both scanned.
#[test]
fn qa_include_modules_inside_the_included_file_are_scanned() {
    let lib = ("kernel/src/lib.rs", "include!(\"../gen/deep/a.rs\");\n");
    let a = ("kernel/gen/deep/a.rs", "mod b;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n");
    let (_, crates) = workspace("mod_premise", "kernel", &[lib, a, ("kernel/gen/deep/b.rs", USE_IT)]);
    rustc_loads(&crates, "kernel", "gen/deep/b.rs");
    fails_and_control(
        "mod_in_included",
        "kernel",
        &[lib, a, ("kernel/gen/deep/b.rs", USE_IT)],
        &[lib, a, ("kernel/gen/deep/b.rs", PLAIN)],
        "gen/deep/b.rs",
    );
    let inline = ("kernel/gen/deep/a.rs", "#[cfg(test)]\nmod tests {\n    fn t() { validation::run(); }\n}\n");
    let inline_plain = ("kernel/gen/deep/a.rs", "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n");
    fails_and_control("inline_in_included", "kernel", &[lib, inline], &[lib, inline_plain], "gen/deep/a.rs");
}

/// An `include!` whose literal names a directory, not a file, does not resolve to a file: it fails.
#[test]
fn qa_include_of_a_directory_fails() {
    let lib = ("kernel/src/lib.rs", "pub fn f() {}\ninclude!(\"../gen\");\n");
    let (meta, _) = workspace("dir", "kernel", &[lib, ("kernel/gen/t.rs", PLAIN)]);
    let (ok, text) = run(&meta);
    assert!(!ok, "an include! of a directory passes xtask deps:\n{text}");
    assert!(text.contains("src/lib.rs"), "the failure does not name the including file:\n{text}");
    let good = ("kernel/src/lib.rs", "pub fn f() {}\ninclude!(\"../gen/t.rs\");\n");
    let (meta, _) = workspace("dir_control", "kernel", &[good, ("kernel/gen/t.rs", PLAIN)]);
    let (ok, text) = run(&meta);
    assert!(ok, "control, the include! naming the file, fails:\n{text}");
}

/// An `include!` whose path is a macro variable is not a string literal at the invocation the checker sees: it
/// fails (R-188: "an include! path that is not a string literal ... fails").
#[test]
fn qa_include_through_a_macro_variable_fails() {
    let lib = (
        "kernel/src/lib.rs",
        "macro_rules! inc {\n    ($p:literal) => { include!($p); };\n}\ninc!(\"../gen/t.rs\");\n",
    );
    let (_, crates) = workspace("macro_var_premise", "kernel", &[lib, ("kernel/gen/t.rs", USE_IT)]);
    rustc_loads(&crates, "kernel", "gen/t.rs");
    let (meta, _) = workspace("macro_var", "kernel", &[lib, ("kernel/gen/t.rs", PLAIN)]);
    let (ok, text) = run(&meta);
    assert!(!ok, "an include! whose path is a macro variable passes xtask deps:\n{text}");
    let direct = ("kernel/src/lib.rs", "include!(\"../gen/t.rs\");\n");
    let (meta, _) = workspace("macro_var_control", "kernel", &[direct, ("kernel/gen/t.rs", PLAIN)]);
    let (ok, text) = run(&meta);
    assert!(ok, "control, the same include! with its literal, fails:\n{text}");
}

/// rustc resolves an `include!` expanded from a `macro_rules!` relative to the file the macro is invoked in, not
/// the file that defines it (the premise check shows it). Here the macro is defined in `src/m.rs` and invoked in
/// `src/a/y.rs`, so rustc loads `kernel/gen/t.rs`, which uses validation. A file also exists at the path read
/// from the definition's file (`crates/gen/t.rs`, clean), so a checker reading the wrong file finds a file and
/// no use. The use in the file rustc loads must fail the check.
#[test]
fn qa_include_in_a_macro_resolves_at_the_call_site() {
    let files = |t: &'static str| {
        vec![
            ("kernel/src/lib.rs", "#[macro_use]\nmod m;\nmod a;\n"),
            ("kernel/src/m.rs", "macro_rules! inc {\n    () => { include!(\"../../gen/t.rs\"); };\n}\n"),
            ("kernel/src/a/mod.rs", "mod y;\n"),
            ("kernel/src/a/y.rs", "inc!();\n"),
            ("kernel/gen/t.rs", t),
            ("gen/t.rs", PLAIN),
        ]
    };
    let (_, crates) = workspace("macro_site_premise", "kernel", &files(USE_IT));
    rustc_loads(&crates, "kernel", "src/a/../../gen/t.rs");
    fails_and_control("macro_site", "kernel", &files(USE_IT), &files(PLAIN), "kernel/gen/t.rs");
}

/// The same for `#[path]` (R-188: include! is followed "as it follows #[path]"): a `#[path]` module expanded from a
/// `macro_rules!` is resolved by rustc at the call site. The macro is defined in `src/m.rs` and invoked in
/// `src/a/b/y.rs`, so rustc loads `kernel/gen/t.rs` (premise check); a clean file exists at `crates/gen/t.rs`,
/// one of the paths the literal names read from the definition's file.
#[test]
fn qa_path_attribute_in_a_macro_resolves_at_the_call_site() {
    let files = |t: &'static str| {
        vec![
            ("kernel/src/lib.rs", "#[macro_use]\nmod m;\nmod a;\n"),
            ("kernel/src/m.rs", "macro_rules! pm {\n    () => { #[path = \"../../../gen/t.rs\"] mod t; };\n}\n"),
            ("kernel/src/a/mod.rs", "mod b;\n"),
            ("kernel/src/a/b/mod.rs", "mod y;\n"),
            ("kernel/src/a/b/y.rs", "pm!();\n"),
            ("kernel/gen/t.rs", t),
            ("gen/t.rs", PLAIN),
        ]
    };
    let (_, crates) = workspace("macro_path_premise", "kernel", &files(USE_IT));
    rustc_loads(&crates, "kernel", "src/a/b/../../../gen/t.rs");
    fails_and_control("macro_path", "kernel", &files(USE_IT), &files(PLAIN), "kernel/gen/t.rs");
}
