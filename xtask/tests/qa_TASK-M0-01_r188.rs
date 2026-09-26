//! QA tests for TASK-M0-01, round 5 layouts, brought in line with R-189 (`decisions.md` § "R-189 — kernel and ledger
//! `src/` use neither `#[path]` nor `include!`"), which replaces R-188 item 2. Written from REQ-SYS-004's verify
//! detail ("in kernel and ledger src/, #[path] (including under cfg_attr) and include! are forbidden, and cargo
//! xtask deps fails on any occurrence, naming the file and line (R-189)") and R-189's text ("The R-187 scan then
//! covers the `.rs` files under `src/` only, and no longer follows `#[path]` or `include!` into other files") — not
//! from the implementation.
//!
//! Each layout that round 5 used to require be *followed* now must fail at the `include!` or `#[path]` itself,
//! naming the file under `src/` and the line, whether the loaded file uses `validation` or not. The control for
//! each (R-176) is the same layout with the `include!` / `#[path]` removed: the file outside `src/`, even one that
//! uses `validation`, is then compiled by nothing and must not be scanned, so the check passes.
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

/// Asserts that `with` fails naming each of `at` (`"<file under crates/>:<line>"`) as a forbidden `what`
/// (`"include!"` or `"#[path]"`), citing R-189, and never reports the file outside src/ it would load; and that
/// `without` (the same layout with the forbidden item removed) passes.
fn fails_at_and_control(
    case: &str,
    krate: &str,
    with: &[(&str, &str)],
    without: &[(&str, &str)],
    what: &str,
    at: &[&str],
    not_named: &str,
) {
    let (meta, _) = workspace(case, krate, with);
    let (ok, text) = run(&meta);
    assert!(!ok, "{case}: a {what} in {krate} src/ passes xtask deps (R-189):\n{text}");
    for site in at {
        let expected = format!("forbidden {what} in {krate} src/ at ");
        let line = text.lines().find(|l| l.contains(&expected) && l.contains(site));
        assert!(line.is_some(), "{case}: the failure does not name {what} at {site}:\n{text}");
        assert!(line.unwrap().contains("R-189"), "{case}: the failure does not cite R-189:\n{text}");
    }
    assert!(
        !text.contains(not_named),
        "{case}: the scan followed the {what} into {not_named}, which R-189 says it no longer does:\n{text}"
    );
    let (meta, _) = workspace(&format!("{case}_control"), krate, without);
    let (ok, text) = run(&meta);
    assert!(ok, "{case}: control, the same layout without the {what}, fails:\n{text}");
}

/// R-189 applies to ledger as to kernel: an `include!` of a file outside src/ fails at the `include!`, whether or
/// not the included file uses validation.
#[test]
fn qa_ledger_include_of_a_file_outside_src_fails() {
    let lib = ("ledger/src/lib.rs", "pub fn f() {}\ninclude!(\"../gen/t.rs\");\n");
    let bare = ("ledger/src/lib.rs", "pub fn f() {}\n");
    for (tag, t) in [("use", USE_IT), ("plain", PLAIN)] {
        fails_at_and_control(
            &format!("ledger_outside_{tag}"),
            "ledger",
            &[lib, ("ledger/gen/t.rs", t)],
            &[bare, ("ledger/gen/t.rs", t)],
            "include!",
            &["ledger/src/lib.rs:2"],
            "gen/t.rs",
        );
    }
}

/// An `include!` of a file without the `.rs` extension fails at the `include!`.
#[test]
fn qa_include_of_a_file_without_rs_extension_fails() {
    let lib = ("kernel/src/lib.rs", "include!(\"../gen/tests.in\");\n");
    let bare = ("kernel/src/lib.rs", "\n");
    for (tag, t) in [("use", USE_IT), ("plain", PLAIN)] {
        fails_at_and_control(
            &format!("ext_{tag}"),
            "kernel",
            &[lib, ("kernel/gen/tests.in", t)],
            &[bare, ("kernel/gen/tests.in", t)],
            "include!",
            &["kernel/src/lib.rs:1"],
            "gen/tests.in",
        );
    }
}

/// An `include!` of a file that itself declares modules fails at the `include!`; nothing in the included file
/// or its modules is scanned.
#[test]
fn qa_include_of_a_file_declaring_modules_fails() {
    let lib = ("kernel/src/lib.rs", "pub fn f() {}\n\ninclude!(\"../gen/deep/a.rs\");\n");
    let bare = ("kernel/src/lib.rs", "pub fn f() {}\n");
    let a = ("kernel/gen/deep/a.rs", "mod b;\n#[cfg(test)]\nmod tests {\n    fn t() { validation::run(); }\n}\n");
    let b = ("kernel/gen/deep/b.rs", USE_IT);
    fails_at_and_control("mod_in_included", "kernel", &[lib, a, b], &[bare, a, b], "include!", &["kernel/src/lib.rs:3"], "gen/deep");
}

/// An `include!` naming a directory fails at the `include!` (as every `include!` does).
#[test]
fn qa_include_of_a_directory_fails() {
    let lib = ("kernel/src/lib.rs", "pub fn f() {}\ninclude!(\"../gen\");\n");
    let bare = ("kernel/src/lib.rs", "pub fn f() {}\n");
    fails_at_and_control(
        "dir",
        "kernel",
        &[lib, ("kernel/gen/t.rs", PLAIN)],
        &[bare, ("kernel/gen/t.rs", PLAIN)],
        "include!",
        &["kernel/src/lib.rs:2"],
        "gen/t.rs",
    );
}

/// An `include!` inside a `macro_rules!` body whose path is a macro variable fails at the `include!`. The premise
/// check shows rustc does load the file through it.
#[test]
fn qa_include_through_a_macro_variable_fails() {
    let lib = (
        "kernel/src/lib.rs",
        "macro_rules! inc {\n    ($p:literal) => { include!($p); };\n}\ninc!(\"../gen/t.rs\");\n",
    );
    let (_, crates) = workspace("macro_var_premise", "kernel", &[lib, ("kernel/gen/t.rs", USE_IT)]);
    rustc_loads(&crates, "kernel", "gen/t.rs");
    let bare = ("kernel/src/lib.rs", "macro_rules! inc {\n    ($p:literal) => { stringify!($p); };\n}\n");
    fails_at_and_control(
        "macro_var",
        "kernel",
        &[lib, ("kernel/gen/t.rs", PLAIN)],
        &[bare, ("kernel/gen/t.rs", USE_IT)],
        "include!",
        &["kernel/src/lib.rs:2"],
        "gen/t.rs",
    );
}

/// An `include!` in a `macro_rules!` defined in one file and invoked in another (rustc resolves it at the call
/// site, the premise check shows) fails at the `include!` in the defining file.
#[test]
fn qa_include_in_a_macro_invoked_elsewhere_fails() {
    let files = |m: &'static str, t: &'static str| {
        vec![
            ("kernel/src/lib.rs", "#[macro_use]\nmod m;\nmod a;\n"),
            ("kernel/src/m.rs", m),
            ("kernel/src/a/mod.rs", "mod y;\n"),
            ("kernel/src/a/y.rs", "inc!();\n"),
            ("kernel/gen/t.rs", t),
            ("gen/t.rs", PLAIN),
        ]
    };
    let with = "macro_rules! inc {\n    () => { include!(\"../../gen/t.rs\"); };\n}\n";
    let without = "macro_rules! inc {\n    () => {};\n}\n";
    let (_, crates) = workspace("macro_site_premise", "kernel", &files(with, USE_IT));
    rustc_loads(&crates, "kernel", "src/a/../../gen/t.rs");
    fails_at_and_control(
        "macro_site",
        "kernel",
        &files(with, USE_IT),
        &files(without, USE_IT),
        "include!",
        &["kernel/src/m.rs:2"],
        "gen/t.rs",
    );
}

/// The same for `#[path]` inside a `macro_rules!` body: it fails at the attribute in the defining file.
#[test]
fn qa_path_attribute_in_a_macro_invoked_elsewhere_fails() {
    let files = |m: &'static str, t: &'static str| {
        vec![
            ("kernel/src/lib.rs", "#[macro_use]\nmod m;\nmod a;\n"),
            ("kernel/src/m.rs", m),
            ("kernel/src/a/mod.rs", "mod b;\n"),
            ("kernel/src/a/b/mod.rs", "mod y;\n"),
            ("kernel/src/a/b/y.rs", "pm!();\n"),
            ("kernel/gen/t.rs", t),
            ("gen/t.rs", PLAIN),
        ]
    };
    let with = "macro_rules! pm {\n    () => { #[path = \"../../../gen/t.rs\"] mod t; };\n}\n";
    let without = "macro_rules! pm {\n    () => {};\n}\n";
    let (_, crates) = workspace("macro_path_premise", "kernel", &files(with, USE_IT));
    rustc_loads(&crates, "kernel", "src/a/b/../../../gen/t.rs");
    fails_at_and_control(
        "macro_path",
        "kernel",
        &files(with, USE_IT),
        &files(without, USE_IT),
        "#[path]",
        &["kernel/src/m.rs:2"],
        "gen/t.rs",
    );
}
