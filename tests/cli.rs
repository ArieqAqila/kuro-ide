//! Command-line behaviour of the real binary.

use std::process::Command;

fn run(args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_kuro")).args(args).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.success())
}

#[test]
fn version_names_the_commit_so_a_stale_install_is_obvious() {
    let (out, ok) = run(&["--version"]);
    assert!(ok);
    let line = out.trim();
    // "kuro 0.1.0 (775d078)" or "(775d078-dirty)", or "(unknown)" outside a git checkout
    assert!(line.starts_with(&format!("kuro {} (", env!("CARGO_PKG_VERSION"))), "{line:?}");
    let rev = line.rsplit('(').next().unwrap().trim_end_matches(')');
    assert!(rev == "unknown" || rev.trim_end_matches("-dirty").chars().all(|c| c.is_ascii_hexdigit()), "{line:?}");
}

#[test]
fn help_and_unknown_options() {
    let (out, ok) = run(&["--help"]);
    assert!(ok && out.contains("USAGE") && out.contains("--config"));
    let o = Command::new(env!("CARGO_BIN_EXE_kuro")).arg("--bogus").output().unwrap();
    assert!(!o.status.success(), "unknown options are an error");
    assert!(String::from_utf8_lossy(&o.stderr).contains("--bogus"));
}
