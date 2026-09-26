//! `cargo xtask deps` against the metadata fixtures (REQ-SYS-004): the workspace graph passes, and each
//! fixture that adds one forbidden edge fails, naming the edge and its dependency kind.

use std::path::PathBuf;
use std::process::Command;

use xtask::deps::{check, DepKind, Edge, Metadata};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// Runs `xtask deps --metadata <fixture>`; returns (success, stderr).
fn run_deps(name: &str) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["deps", "--metadata"])
        .arg(fixture(name))
        .output()
        .expect("run xtask");
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

fn assert_fails_naming(name: &str, expected: &str) {
    let (ok, stderr) = run_deps(name);
    assert!(!ok, "{name}: xtask deps passed, but the fixture has a forbidden edge");
    assert!(stderr.contains(expected), "{name}: stderr does not name {expected:?}:\n{stderr}");
    assert_eq!(
        stderr.matches("forbidden edge").count(),
        1,
        "{name}: expected exactly one forbidden edge:\n{stderr}"
    );
}

#[test]
fn deps_workspace_fixture_passes() {
    let (ok, stderr) = run_deps("metadata_workspace.json");
    assert!(ok, "the workspace graph fixture fails:\n{stderr}");
}

#[test]
fn deps_forbidden_kernel_to_engine_fails() {
    assert_fails_naming("metadata_kernel_engine.json", "forbidden edge kernel → engine (normal dependency)");
}

#[test]
fn deps_forbidden_engine_to_gui_fails() {
    assert_fails_naming("metadata_engine_gui.json", "forbidden edge engine → gui (normal dependency)");
}

#[test]
fn deps_forbidden_ledger_to_engine_fails() {
    assert_fails_naming("metadata_ledger_engine.json", "forbidden edge ledger → engine (normal dependency)");
}

#[test]
fn deps_forbidden_kernel_to_ledger_normal_fails_naming_kind() {
    assert_fails_naming(
        "metadata_kernel_ledger_normal.json",
        "forbidden edge kernel → ledger (normal dependency): kernel → ledger is a build-dependency only (R-185)",
    );
}

#[test]
fn deps_workspace_fixture_has_kernel_ledger_as_build_dependency() {
    let metadata = Metadata::from_file(&fixture("metadata_workspace.json")).unwrap();
    let edges = metadata.edges().unwrap();
    assert!(edges.contains(&edge("kernel", "ledger", DepKind::Build)));
    assert!(!edges.iter().any(|e| e.from == "ledger"), "ledger must have no workspace dependency");
    // Non-workspace dependencies (xtask's serde, serde_json) are not edges.
    assert!(!edges.iter().any(|e| e.to.starts_with("serde")));
}

#[test]
fn deps_live_workspace_passes() {
    let edges = Metadata::from_cargo().unwrap().edges().unwrap();
    assert_eq!(check(&edges), vec![], "the workspace crate graph has a forbidden edge");
}

fn edge(from: &str, to: &str, kind: DepKind) -> Edge {
    Edge { from: from.to_owned(), to: to.to_owned(), kind }
}

fn forbidden(from: &str, to: &str, kind: DepKind) -> bool {
    !check(&[edge(from, to, kind)]).is_empty()
}

#[test]
fn deps_table_allows_the_crate_map_edges() {
    use DepKind::*;
    for (from, to, kind) in [
        ("kernel", "ledger", Build),
        ("render", "ledger", Normal),
        ("engine", "ledger", Normal),
        ("engine", "kernel", Normal),
        ("engine", "render", Normal),
        ("gui", "engine", Normal),
        ("prin", "engine", Normal),
        ("validation", "ledger", Normal),
        ("validation", "kernel", Normal),
        ("validation", "engine", Normal),
        ("validation", "render", Normal),
        ("engine", "validation", Dev),
        ("xtask", "validation", Dev),
        ("prin", "validation", Dev),
        ("render", "validation", Dev),
        ("kernel", "validation", Dev),
        ("ledger", "validation", Dev),
    ] {
        assert!(!forbidden(from, to, kind), "{from} → {to} ({kind}) should be allowed");
    }
}

#[test]
fn deps_table_forbids_edges_outside_the_crate_map() {
    use DepKind::*;
    for (from, to, kind) in [
        ("kernel", "ledger", Dev),
        ("kernel", "render", Build),
        ("ledger", "kernel", Build),
        ("render", "engine", Normal),
        ("validation", "gui", Normal),
        ("prin", "gui", Normal),
        ("gui", "kernel", Normal),
        ("prin", "kernel", Normal),
        ("engine", "validation", Normal),
        ("engine", "validation", Build),
        ("engine", "xtask", Normal),
        ("xtask", "engine", Normal),
        ("engine", "prin", Normal),
        ("gui", "validation", Dev),
        ("validation", "prin", Normal),
        ("kernel", "validation", Normal),
        ("ledger", "validation", Build),
    ] {
        assert!(forbidden(from, to, kind), "{from} → {to} ({kind}) should be forbidden");
    }
}

/// R-187, which closes RQ-129: kernel and ledger may take validation as a dev-dependency, never as a normal or
/// build dependency; gui never depends on validation; validation never depends on prin.
#[test]
fn deps_applies_r_187_to_the_validation_edges() {
    use DepKind::*;
    for from in ["kernel", "ledger"] {
        // Allowed: the dev-dependency. It is also the control for the two failing kinds below.
        assert_eq!(check(&[edge(from, "validation", Dev)]), vec![], "{from} → validation (dev) is allowed (R-187)");
        for kind in [Normal, Build] {
            let violations = check(&[edge(from, "validation", kind)]);
            assert_eq!(violations.len(), 1, "{from} → validation ({kind}) should be forbidden");
            assert!(violations[0].rule.contains("dev-dependency"), "{}", violations[0].rule);
        }
    }
    // gui → validation fails in every kind. Control: the same dev edge from engine passes.
    assert!(!forbidden("engine", "validation", Dev));
    for kind in [Normal, Dev, Build] {
        let violations = check(&[edge("gui", "validation", kind)]);
        assert_eq!(violations.len(), 1, "gui → validation ({kind}) should be forbidden");
        assert!(violations[0].rule.contains("R-187"), "{}", violations[0].rule);
    }
    // validation → prin fails in every kind. Control: validation → engine passes.
    assert!(!forbidden("validation", "engine", Normal));
    for kind in [Normal, Dev, Build] {
        let violations = check(&[edge("validation", "prin", kind)]);
        assert_eq!(violations.len(), 1, "validation → prin ({kind}) should be forbidden");
        assert!(violations[0].rule.contains("R-187"), "{}", violations[0].rule);
    }
}

fn metadata_json(dep: &str) -> Metadata {
    let doc = format!(
        r#"{{"workspace_members": ["path+file:///ws/crates/engine#0.1.0", "path+file:///ws/crates/gui#0.1.0"],
            "packages": [
              {{"name": "engine", "id": "path+file:///ws/crates/engine#0.1.0",
                "manifest_path": "/ws/crates/engine/Cargo.toml", "dependencies": [{dep}]}},
              {{"name": "gui", "id": "path+file:///ws/crates/gui#0.1.0",
                "manifest_path": "/ws/crates/gui/Cargo.toml", "dependencies": []}}]}}"#
    );
    Metadata::from_json(doc.as_bytes()).unwrap()
}

/// REQ-SYS-004 constrains workspace edges. A registry package, or a path package outside the workspace, that
/// shares a member's name is not one.
#[test]
fn deps_a_dependency_that_only_shares_a_member_name_is_not_an_edge() {
    let registry =
        r#"{"name": "gui", "kind": null, "source": "registry+https://github.com/rust-lang/crates.io-index"}"#;
    assert_eq!(metadata_json(registry).edges().unwrap(), vec![]);
    let elsewhere = r#"{"name": "gui", "kind": null, "source": null, "path": "/elsewhere/gui"}"#;
    assert_eq!(metadata_json(elsewhere).edges().unwrap(), vec![]);
    // Control: the same dependency as a path dependency on the member is an edge, and a forbidden one.
    let member = r#"{"name": "gui", "kind": null, "source": null, "path": "/ws/crates/gui"}"#;
    let edges = metadata_json(member).edges().unwrap();
    assert_eq!(edges, vec![edge("engine", "gui", DepKind::Normal)]);
    assert_eq!(check(&edges).len(), 1);
}

/// A synthetic workspace on disk for R-187's source condition: the §7.1 graph among ledger, kernel and
/// validation, plus `extra_dev` from `crate_name` (a raw dependency entry, e.g. a dev-dependency on
/// validation), with `files` (paths relative to that crate) written under it. Returns the metadata file.
fn source_workspace(case: &str, crate_name: &str, extra_dev: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("deps_r187").join(case);
    let _ = std::fs::remove_dir_all(&root);
    for name in ["ledger", "kernel", "validation"] {
        let src = root.join("crates").join(name).join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "").unwrap();
    }
    for (rel, text) in files {
        let path = root.join("crates").join(crate_name).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let ws = root.display().to_string();
    let dep = |to: &str, kind: &str| {
        format!(r#"{{"name": "{to}", "kind": {kind}, "source": null, "path": "{ws}/crates/{to}"}}"#)
    };
    let deps_of = |name: &str| {
        let mut deps: Vec<String> = match name {
            "kernel" => vec![dep("ledger", r#""build""#)],
            "validation" => vec![dep("ledger", "null"), dep("kernel", "null")],
            _ => vec![],
        };
        if name == crate_name && !extra_dev.is_empty() {
            deps.push(extra_dev.replace("{ws}", &ws));
        }
        deps.join(", ")
    };
    let package = |name: &str| {
        format!(
            r#"{{"name": "{name}", "id": "path+file://{ws}/crates/{name}#0.1.0",
                "manifest_path": "{ws}/crates/{name}/Cargo.toml", "dependencies": [{}]}}"#,
            deps_of(name)
        )
    };
    let doc = format!(
        r#"{{"workspace_members": ["path+file://{ws}/crates/ledger#0.1.0", "path+file://{ws}/crates/kernel#0.1.0",
             "path+file://{ws}/crates/validation#0.1.0"],
            "packages": [{}, {}, {}]}}"#,
        package("ledger"),
        package("kernel"),
        package("validation")
    );
    let path = root.join("metadata.json");
    std::fs::write(&path, doc).unwrap();
    path
}

fn run_deps_path(path: &std::path::Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["deps", "--metadata"])
        .arg(path)
        .output()
        .expect("run xtask");
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

const DEV_ON_VALIDATION: &str =
    r#"{"name": "validation", "kind": "dev", "source": null, "path": "{ws}/crates/validation"}"#;
const UNIT_TEST: &str = "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    use validation::Harness;\n}\n";
const INTEGRATION_TEST: &str = "use validation::Harness;\n\n#[test]\nfn t() {}\n";

/// R-187: in kernel and ledger, a test that uses validation is an integration test (tests/), not a unit test
/// in src/. A unit test in src/ fails `xtask deps`, naming the crate, file and line; the control, the same
/// use in tests/, passes.
#[test]
fn deps_validation_use_in_kernel_or_ledger_src_fails_and_in_tests_passes() {
    for name in ["kernel", "ledger"] {
        let unit = source_workspace(
            &format!("{name}_unit"),
            name,
            DEV_ON_VALIDATION,
            &[("src/lib.rs", UNIT_TEST), ("tests/controls.rs", INTEGRATION_TEST)],
        );
        let (ok, stderr) = run_deps_path(&unit);
        assert!(!ok, "{name}: a unit test in src/ that uses validation passes xtask deps");
        let expected = format!("forbidden use of validation in {name} src/ at ");
        assert!(stderr.contains(&expected), "{name}: stderr does not name the use:\n{stderr}");
        assert!(stderr.contains("src/lib.rs:5"), "{name}: stderr does not name the file and line:\n{stderr}");
        assert_eq!(stderr.matches("xtask deps: forbidden").count(), 1, "{name}: expected one violation:\n{stderr}");

        // Control: the same use as an integration test only.
        let integration = source_workspace(
            &format!("{name}_integration"),
            name,
            DEV_ON_VALIDATION,
            &[("src/lib.rs", "pub fn f() {}\n"), ("tests/controls.rs", INTEGRATION_TEST)],
        );
        let (ok, stderr) = run_deps_path(&integration);
        assert!(ok, "{name}: an integration test that uses validation fails xtask deps:\n{stderr}");
    }
}

/// A module file deeper under src/, and a renamed dev-dependency, are scanned too.
#[test]
fn deps_validation_use_is_found_in_submodules_and_under_a_rename() {
    let renamed = r#"{"name": "validation", "rename": "harness", "kind": "dev", "source": null,
                      "path": "{ws}/crates/validation"}"#;
    let metadata = source_workspace(
        "kernel_renamed",
        "kernel",
        renamed,
        &[("src/chart/mod.rs", "#[cfg(test)]\nfn t() { harness::run(); }\n")],
    );
    let (ok, stderr) = run_deps_path(&metadata);
    assert!(!ok, "a use of the renamed validation in src/chart/mod.rs passes xtask deps");
    assert!(stderr.contains("chart/mod.rs:2"), "{stderr}");
    // Under the rename, the crate's own name still counts (a local `mod validation` included).
    let metadata = source_workspace(
        "kernel_renamed_own_name",
        "kernel",
        renamed,
        &[("src/chart/mod.rs", "mod validation { pub fn run() {} }\nfn t() { validation::run(); }\n")],
    );
    let (ok, stderr) = run_deps_path(&metadata);
    assert!(!ok && stderr.contains("chart/mod.rs:1"), "{stderr}");
    // Control: the same module without either name passes.
    let metadata = source_workspace(
        "kernel_renamed_control",
        "kernel",
        renamed,
        &[("src/chart/mod.rs", "mod checks { pub fn run() {} }\nfn t() { checks::run(); }\n")],
    );
    let (ok, stderr) = run_deps_path(&metadata);
    assert!(ok, "{stderr}");
}

/// The live workspace must have its sources and targets to check: a kernel with a validation dependency whose src/
/// is missing is an error, not a silent pass, and so is metadata without kernel's and ledger's targets. Control: the
/// same metadata read as a fixture skips the scan.
#[test]
fn deps_missing_sources_are_an_error_for_the_live_workspace() {
    let path = source_workspace("kernel_no_src", "kernel", DEV_ON_VALIDATION, &[]);
    let err = Metadata::from_file(&path).unwrap().source_violations(true).unwrap_err();
    assert!(err.contains("no library or binary target"), "{err}");
    with_lib_target(&path, "kernel", "src/lib.rs");
    with_lib_target(&path, "ledger", "src/lib.rs");
    std::fs::remove_dir_all(path.parent().unwrap().join("crates/kernel/src")).unwrap();
    let metadata = Metadata::from_file(&path).unwrap();
    let err = metadata.source_violations(true).unwrap_err();
    assert!(err.contains("kernel: cannot find its src/"), "{err}");
    assert_eq!(metadata.source_violations(false).unwrap(), vec![]);
}

/// Any identifier named `validation` in kernel or ledger src/, outside comments and literals, fails, naming the
/// line: a use of the crate after a comparison, a qualified path, a C raw string, in a macro or an attribute, and a
/// local item with the name (a deliberate over-approximation: without name resolution `validation::x` may be a
/// local module or the crate). Controls: the same source under tests/ passes, and so does the source without the
/// identifier.
#[test]
fn deps_any_validation_identifier_in_src_fails() {
    let cases = [
        ("char_lt", "#[cfg(test)]\nfn t(c: char, d: u8) -> bool {\n    'a' < c && d > ::validation::LIMIT\n}\n"),
        ("qualified", "fn t() -> u8 {\n\n    <u8 as Tr>::validation::X\n}\n"),
        (
            "c_raw_string",
            "pub const C: &core::ffi::CStr = cr#\"a\"b\"#;\n#[cfg(test)]\nfn t() { validation::run(); }\n",
        ),
        ("macro", "#[cfg(test)]\nfn t() {\n    assert!(validation::ok());\n}\n"),
        ("attribute", "#[cfg(test)]\n\n#[validation::harness]\nfn t() {}\n"),
        ("local_mod", "mod checks {}\n\nmod validation { pub fn run() {} }\n"),
    ];
    for (case, src) in cases {
        let metadata = source_workspace(&format!("id_{case}"), "kernel", DEV_ON_VALIDATION, &[("src/lib.rs", src)]);
        let (ok, stderr) = run_deps_path(&metadata);
        assert!(!ok, "{case}: an identifier named validation in src/ passes xtask deps");
        assert!(stderr.contains("src/lib.rs:3"), "{case}: stderr does not name the line:\n{stderr}");
        let metadata = source_workspace(
            &format!("id_{case}_tests"),
            "kernel",
            DEV_ON_VALIDATION,
            &[("tests/controls.rs", src)],
        );
        let (ok, stderr) = run_deps_path(&metadata);
        assert!(ok, "{case}: control, the same source under tests/, fails:\n{stderr}");
        let plain = src.replace("validation", "checks");
        let metadata =
            source_workspace(&format!("id_{case}_plain"), "kernel", DEV_ON_VALIDATION, &[("src/lib.rs", &plain)]);
        let (ok, stderr) = run_deps_path(&metadata);
        assert!(ok, "{case}: control, the source without the identifier, fails:\n{stderr}");
    }
}

/// A source under kernel src/ that does not lex fails `xtask deps`, naming the file: it is never passed unscanned.
/// Control: the same file, lexing, passes.
#[test]
fn deps_a_src_file_that_does_not_lex_fails() {
    let metadata = source_workspace(
        "lex_error",
        "kernel",
        DEV_ON_VALIDATION,
        &[("src/broken.rs", "fn f() { \"unterminated }\n")],
    );
    let (ok, stderr) = run_deps_path(&metadata);
    assert!(!ok && stderr.contains("src/broken.rs: cannot lex it"), "{stderr}");
    let metadata = source_workspace(
        "lex_error_control",
        "kernel",
        DEV_ON_VALIDATION,
        &[("src/broken.rs", "fn f() { \"terminated\"; }\n")],
    );
    let (ok, stderr) = run_deps_path(&metadata);
    assert!(ok, "{stderr}");
}

/// Sets `crate_name`'s targets in the metadata at `path` to one `lib` target at `src_path` (relative to the crate).
fn with_lib_target(path: &std::path::Path, crate_name: &str, src_path: &str) {
    let mut doc: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let dir = path.parent().unwrap().join("crates").join(crate_name);
    for package in doc["packages"].as_array_mut().unwrap() {
        if package["name"] == crate_name {
            let src_path = dir.join(src_path).display().to_string();
            package["targets"] = serde_json::json!([{ "kind": ["lib"], "src_path": src_path }]);
        }
    }
    std::fs::write(path, serde_json::to_vec(&doc).unwrap()).unwrap();
}

/// A kernel or ledger library target outside src/ (`[lib] path = "lib/lib.rs"`) fails, naming the crate: its unit
/// tests would escape the scan of src/. Control: the same crate with its library at src/lib.rs passes.
#[test]
fn deps_a_kernel_or_ledger_lib_outside_src_fails() {
    for name in ["kernel", "ledger"] {
        let metadata = source_workspace(&format!("lib_{name}"), name, DEV_ON_VALIDATION, &[("lib/lib.rs", "")]);
        with_lib_target(&metadata, name, "lib/lib.rs");
        let (ok, stderr) = run_deps_path(&metadata);
        assert!(!ok && stderr.contains(&format!("{name}: its lib target is at ")), "{name}: {stderr}");
        let metadata = source_workspace(&format!("lib_{name}_control"), name, DEV_ON_VALIDATION, &[]);
        with_lib_target(&metadata, name, "src/lib.rs");
        let (ok, stderr) = run_deps_path(&metadata);
        assert!(ok, "{name}: control, the library at src/lib.rs, fails:\n{stderr}");
    }
}

/// Runs `xtask deps` on a synthetic workspace whose `crate_name` has `files`, with or without a dev-dependency on
/// validation; returns (success, stderr).
fn run_source(case: &str, crate_name: &str, dev_on_validation: bool, files: &[(&str, &str)]) -> (bool, String) {
    let dev = if dev_on_validation { DEV_ON_VALIDATION } else { "" };
    run_deps_path(&source_workspace(case, crate_name, dev, files))
}

/// R-189: a `#[path]` attribute in kernel or ledger src/ fails `xtask deps`, naming the file and line and citing
/// R-189, with or without a dependency on validation: as an outer or inner attribute, and inside a `macro_rules!`
/// body. Control: the same file without the attribute (the module at its default place) passes.
#[test]
fn deps_a_path_attribute_in_kernel_or_ledger_src_fails() {
    let cases = [
        ("outer", "pub fn f() {}\n#[path = \"../elsewhere/t.rs\"]\nmod t;\n", "pub fn f() {}\n\nmod t;\n"),
        (
            "macro",
            "macro_rules! m {\n    () => {\n        #[path = \"../elsewhere/t.rs\"] mod t;\n    };\n}\n",
            "macro_rules! m {\n    () => {\n        mod t;\n    };\n}\n",
        ),
    ];
    for name in ["kernel", "ledger"] {
        for dev in [false, true] {
            for (case, with, without) in cases {
                let line = if case == "outer" { 2 } else { 3 };
                let files = [("src/lib.rs", with), ("src/t.rs", ""), ("elsewhere/t.rs", "")];
                let (ok, stderr) = run_source(&format!("path_{name}_{case}_{dev}"), name, dev, &files);
                let expected = format!("forbidden #[path] in {name} src/ at ");
                assert!(!ok, "{name} {case} (dev {dev}): a #[path] in src/ passes:\n{stderr}");
                assert!(stderr.contains(&expected), "{name} {case}: {stderr}");
                assert!(stderr.contains(&format!("src/lib.rs:{line}")), "{name} {case}: no file and line:\n{stderr}");
                assert!(stderr.contains("R-189"), "{name} {case}: does not cite R-189:\n{stderr}");
                // Control: the same file without the attribute.
                let files = [("src/lib.rs", without), ("src/t.rs", ""), ("elsewhere/t.rs", "")];
                let (ok, stderr) = run_source(&format!("path_{name}_{case}_{dev}_control"), name, dev, &files);
                assert!(ok, "{name} {case} (dev {dev}): control, without the attribute, fails:\n{stderr}");
            }
        }
    }
}

/// R-189: a `path` under `cfg_attr` in kernel src/ fails, naming the file and line. Control: the same file with the
/// `cfg_attr` carrying another attribute passes.
#[test]
fn deps_a_cfg_attr_path_in_kernel_src_fails() {
    let with = "pub fn f() {}\n\n#[cfg_attr(test, path = \"../elsewhere/t.rs\")]\nmod t;\n";
    let files = [("src/lib.rs", with), ("src/t.rs", ""), ("elsewhere/t.rs", "")];
    let (ok, stderr) = run_source("cfg_attr_path", "kernel", true, &files);
    assert!(!ok && stderr.contains("forbidden #[path] in kernel src/ at "), "{stderr}");
    assert!(stderr.contains("src/lib.rs:3") && stderr.contains("R-189"), "{stderr}");
    let without = "pub fn f() {}\n\n#[cfg_attr(test, allow(dead_code))]\nmod t;\n";
    let files = [("src/lib.rs", without), ("src/t.rs", "")];
    let (ok, stderr) = run_source("cfg_attr_path_control", "kernel", true, &files);
    assert!(ok, "control, cfg_attr without path, fails:\n{stderr}");
}

/// R-189: an `include!` in kernel or ledger src/ fails, naming the file and line and citing R-189, with or without a
/// dependency on validation: bare, `std::`- and `core::`-qualified, with any delimiter, and inside a `macro_rules!`
/// body. Controls: the same file without the macro passes, and so does `include_str!`/`include_bytes!` in its place.
#[test]
fn deps_an_include_in_kernel_or_ledger_src_fails() {
    let cases = [
        ("bare", "pub fn f() {}\n\ninclude!(\"t.rs\");\n"),
        ("std", "pub fn f() {}\n\nstd::include!(\"t.rs\");\n"),
        ("core", "pub fn f() {}\n\ncore::include! { concat!(env!(\"OUT_DIR\"), \"/t.rs\") }\n"),
        ("macro", "macro_rules! m {\n    () => {\n        include!(\"t.rs\");\n    };\n}\n"),
    ];
    for name in ["kernel", "ledger"] {
        for dev in [false, true] {
            for (case, with) in cases {
                let files = [("src/lib.rs", with), ("src/t.rs", "")];
                let (ok, stderr) = run_source(&format!("include_{name}_{case}_{dev}"), name, dev, &files);
                assert!(!ok, "{name} {case} (dev {dev}): an include! in src/ passes:\n{stderr}");
                assert!(stderr.contains(&format!("forbidden include! in {name} src/ at ")), "{name} {case}: {stderr}");
                assert!(stderr.contains("src/lib.rs:3"), "{name} {case}: no file and line:\n{stderr}");
                assert!(stderr.contains("R-189"), "{name} {case}: does not cite R-189:\n{stderr}");
                // Controls: the same file without the macro, and with include_str!/include_bytes! in its place.
                let without = with.replace("include!", "stringify!");
                let as_str = with.replace("include!", "include_str!");
                let as_bytes = with.replace("include!", "include_bytes!");
                for (tag, text) in [("without", without), ("str", as_str), ("bytes", as_bytes)] {
                    let files = [("src/lib.rs", text.as_str()), ("src/t.rs", "")];
                    let (ok, stderr) =
                        run_source(&format!("include_{name}_{case}_{dev}_{tag}"), name, dev, &files);
                    assert!(ok, "{name} {case} (dev {dev}): control {tag} fails:\n{stderr}");
                }
            }
        }
    }
}

/// Runs `xtask deps` on a synthetic workspace whose `crate_name` has `src/lib.rs` = `lib` (plus an empty `src/t.rs`
/// and a file outside src/ that a forbidden item could load), without a dependency on validation.
fn run_lib(case: &str, crate_name: &str, lib: &str) -> (bool, String) {
    let files = [("src/lib.rs", lib), ("src/t.rs", ""), ("gen/t.rs", "pub fn t() {}\n")];
    run_source(case, crate_name, false, &files)
}

/// Asserts `stderr` names a forbidden `what` in `crate_name` src/ at `src/lib.rs:<line>`, citing R-190.
fn assert_names(case: &str, stderr: &str, what: &str, crate_name: &str, line: usize) {
    let named = stderr.lines().any(|l| {
        l.contains(&format!("forbidden {what} in {crate_name} src/ at "))
            && l.contains(&format!("src/lib.rs:{line}:"))
            && l.contains("R-190")
    });
    assert!(named, "{case}: no failure names {what} at src/lib.rs:{line}, citing R-190:\n{stderr}");
}

const OUT_DIR_FORM: &str = "pub fn f() {}\n\ninclude!(concat!(env!(\"OUT_DIR\"), \"/layout.rs\"));\n";

/// R-190: kernel src/ may hold the item-level `include!(concat!(env!("OUT_DIR"), "/<name>.rs"))`, at the file's top
/// level, after an attribute, and in a `mod` body; ledger src/ may not. Control for the kernel passes: the same
/// file in ledger fails, naming the file and line.
#[test]
fn deps_the_out_dir_include_passes_in_kernel_src_and_fails_in_ledger_src() {
    let forms = [
        ("top", OUT_DIR_FORM, 3),
        ("attr", "pub fn f() {}\n#[allow(dead_code)]\ninclude!(concat!(env!(\"OUT_DIR\"), \"/layout.rs\"));\n", 3),
        ("mod", "pub mod gen {\n    include!(concat!(env!(\"OUT_DIR\"), \"/layout.rs\"));\n}\n", 2),
    ];
    for (case, lib, line) in forms {
        for dev in [false, true] {
            let files = [("src/lib.rs", lib)];
            let (ok, stderr) = run_source(&format!("out_dir_kernel_{case}_{dev}"), "kernel", dev, &files);
            assert!(ok, "{case} (dev {dev}): the OUT_DIR include in kernel src/ fails:\n{stderr}");
            // Control: the same file in ledger src/ fails.
            let (ok, stderr) = run_source(&format!("out_dir_ledger_{case}_{dev}"), "ledger", dev, &files);
            assert!(!ok, "{case} (dev {dev}): the OUT_DIR include in ledger src/ passes:\n{stderr}");
            assert_names(case, &stderr, "include!", "ledger", line);
        }
    }
}

/// R-190: only the exact item-level form passes in kernel src/. Inside a `macro_rules!` body, path-qualified
/// (`std::include!`), with `..` in the literal, in a function body, with another directory than `OUT_DIR`, with a
/// subdirectory, braces or a trailing comma, it fails, naming the file and line. Control: `OUT_DIR_FORM` passes.
#[test]
fn deps_any_other_out_dir_include_in_kernel_src_fails() {
    let form = "include!(concat!(env!(\"OUT_DIR\"), \"/layout.rs\"));";
    let cases = [
        ("macro", format!("macro_rules! m {{\n    () => {{\n        {form}\n    }};\n}}\nm!();\n"), 3),
        ("std", format!("pub fn f() {{}}\n\nstd::{form}\n"), 3),
        ("dotdot", OUT_DIR_FORM.replace("/layout.rs", "/../../../gen/t.rs"), 3),
        ("fn_body", format!("pub fn f() {{\n    {form}\n}}\n"), 2),
        ("other_var", OUT_DIR_FORM.replace("OUT_DIR", "CARGO_MANIFEST_DIR"), 3),
        ("subdir", OUT_DIR_FORM.replace("/layout.rs", "/a/layout.rs"), 3),
        ("braces", "pub fn f() {}\n\ninclude! { concat!(env!(\"OUT_DIR\"), \"/layout.rs\") }\n".to_owned(), 3),
        ("trailing_comma", OUT_DIR_FORM.replace("\"/layout.rs\")", "\"/layout.rs\",)"), 3),
        ("expression", "pub fn f() {}\n\nconst X: u8 = include!(concat!(env!(\"OUT_DIR\"), \"/n.rs\"));\n".to_owned(), 3),
    ];
    for (case, lib, line) in cases {
        let (ok, stderr) = run_lib(&format!("out_dir_other_{case}"), "kernel", &lib);
        assert!(!ok, "{case}: this include! in kernel src/ passes:\n{lib}\n{stderr}");
        assert_names(case, &stderr, "include!", "kernel", line);
    }
    let (ok, stderr) = run_lib("out_dir_other_control", "kernel", OUT_DIR_FORM);
    assert!(ok, "control, the OUT_DIR form, fails:\n{stderr}");
}

/// R-190: a macro named `env` or `concat` defined or aliased in kernel src/ would take the builtin's place in the
/// OUT_DIR form, so it fails when that form is used, naming its own line. Control: without the OUT_DIR include the
/// same definition passes, and so does the OUT_DIR form with a macro of another name.
#[test]
fn deps_a_shadowed_env_or_concat_with_the_out_dir_include_fails() {
    let shadows = [
        ("env", "macro_rules! env {\n    ($x:literal) => { \"../gen\" };\n}\n"),
        ("concat", "macro_rules! concat {\n    ($($x:tt)*) => { \"../gen/t.rs\" };\n}\n"),
        ("alias", "use core::stringify as env;\n"),
    ];
    for (case, shadow) in shadows {
        let lib = format!("{shadow}{OUT_DIR_FORM}");
        let (ok, stderr) = run_lib(&format!("shadow_{case}"), "kernel", &lib);
        assert!(!ok, "{case}: a shadowed builtin with the OUT_DIR include passes:\n{stderr}");
        assert_names(case, &stderr, "macro named concat or env", "kernel", 1);
        let (ok, stderr) = run_lib(&format!("shadow_{case}_control"), "kernel", shadow);
        assert!(ok, "{case}: control, the definition without the OUT_DIR include, fails:\n{stderr}");
    }
    let other = format!("macro_rules! gen {{\n    () => {{}};\n}}\n{OUT_DIR_FORM}");
    let (ok, stderr) = run_lib("shadow_other_control", "kernel", &other);
    assert!(ok, "control, a macro of another name with the OUT_DIR include, fails:\n{stderr}");
}

/// R-190: any other `include` identifier fails in kernel and ledger src/: an alias (`use std::include as inc`) and
/// `include` passed to a macro (`m!(include)`), naming the file and line. Controls: `stringify` in its place passes.
#[test]
fn deps_an_aliased_or_passed_include_fails() {
    let cases = [
        ("alias", "use std::include as inc;\ninc!(\"../gen/t.rs\");\n", 1),
        ("passed", "macro_rules! m {\n    ($i:ident) => { $i!(\"../gen/t.rs\"); };\n}\nm!(include);\n", 4),
    ];
    for name in ["kernel", "ledger"] {
        for (case, lib, line) in cases {
            let (ok, stderr) = run_lib(&format!("include_ident_{name}_{case}"), name, lib);
            assert!(!ok, "{name} {case}: passes:\n{stderr}");
            assert_names(case, &stderr, "include!", name, line);
            let control = lib.replace("include", "stringify");
            let (ok, stderr) = run_lib(&format!("include_ident_{name}_{case}_control"), name, &control);
            assert!(ok, "{name} {case}: control with stringify fails:\n{stderr}");
        }
    }
}

/// R-190: an attribute holding a macro variable fails in kernel and ledger src/ (`#[$a]`, `#[cfg_attr(test, $a)]`,
/// `# $a`), naming the file and line. Control: a `#[cfg(test)]` attribute in the same macro body (no `$`) passes.
#[test]
fn deps_an_attribute_with_a_macro_variable_fails() {
    let cases = [
        ("meta", "macro_rules! m {\n    ($a:meta) => {\n        #[$a] mod t;\n    };\n}\n"),
        ("cfg_attr", "macro_rules! m {\n    ($a:meta) => {\n        #[cfg_attr(test, $a)] mod t;\n    };\n}\n"),
        ("tt", "macro_rules! m {\n    ($a:tt) => {\n        # $a mod t;\n    };\n}\n"),
    ];
    for name in ["kernel", "ledger"] {
        for (case, lib) in cases {
            let (ok, stderr) = run_lib(&format!("macro_var_attr_{name}_{case}"), name, lib);
            assert!(!ok, "{name} {case}: passes:\n{stderr}");
            assert_names(case, &stderr, "attribute with a macro variable", name, 3);
        }
        let control = "macro_rules! m {\n    () => {\n        #[cfg(test)] mod t;\n    };\n}\n";
        let (ok, stderr) = run_lib(&format!("macro_var_attr_{name}_control"), name, control);
        assert!(ok, "{name}: control, #[cfg(test)] in a macro body, fails:\n{stderr}");
    }
}

/// R-190: `path` outside an attribute is an ordinary identifier (`let path = 1;`), and passes in kernel and ledger
/// src/. Control: the same `path = …` as an attribute fails.
#[test]
fn deps_a_path_binding_passes() {
    for name in ["kernel", "ledger"] {
        let lib = "pub fn f() -> u8 {\n    let path = 1;\n    path\n}\n";
        let (ok, stderr) = run_lib(&format!("path_binding_{name}"), name, lib);
        assert!(ok, "{name}: `let path = 1;` fails:\n{stderr}");
        let control = "pub fn f() {}\n#[path = \"../gen/t.rs\"]\nmod t;\n";
        let (ok, stderr) = run_lib(&format!("path_binding_{name}_control"), name, control);
        assert!(!ok, "{name}: control, #[path], passes:\n{stderr}");
        assert_names("path", &stderr, "#[path]", name, 2);
    }
}
