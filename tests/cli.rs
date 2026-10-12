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

// ---------------------------------------------------------------- kuro ext (the real binary, in a throw-away HOME)

mod ext_cli {
    use std::path::Path;
    use std::process::Command;

    fn kuro(home: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_kuro"))
            .args(args)
            .env("HOME", home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env_remove("KURO_CONFIG_DIR")
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn demo(dir: &Path) -> std::path::PathBuf {
        let d = dir.join("demo-src");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("kuro-ext.toml"),
            "format = 1\nname = \"demo\"\nversion = \"1.0.0\"\nkuro = \">=0.1, <0.2\"\n[[language]]\nname = \"Demo\"\nextensions = [\"dm\"]\nkeywords = [\"if\"]\n",
        )
        .unwrap();
        d
    }

    #[test]
    fn install_list_and_remove_from_the_command_line() {
        let home = tempfile::tempdir().unwrap();
        let src = demo(home.path());
        let (code, out, _) = kuro(home.path(), &["ext", "list"]);
        assert_eq!((code, out.as_str()), (0, "no extensions installed\n"));
        let (code, out, err) = kuro(home.path(), &["ext", "install", src.to_str().unwrap()]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "installed `demo` 1.0.0 (languages: Demo)\n");
        let (_, out, _) = kuro(home.path(), &["ext", "list"]);
        assert!(out.starts_with("demo 1.0.0  languages: Demo  kuro: >=0.1, <0.2  [ok]"), "{out}");
        let (code, _, err) = kuro(home.path(), &["ext", "install", src.to_str().unwrap()]);
        assert_eq!(code, 1);
        assert!(err.contains("already installed") && err.contains("--force"), "{err}");
        let (code, out, _) = kuro(home.path(), &["ext", "install", "--force", src.to_str().unwrap()]);
        assert_eq!((code, out.starts_with("replaced `demo`")), (0, true));
        let (code, out, _) = kuro(home.path(), &["ext", "remove", "demo"]);
        assert_eq!((code, out.as_str()), (0, "removed `demo`\n"));
        let (code, _, err) = kuro(home.path(), &["ext", "remove", "demo"]);
        assert_eq!(code, 1);
        assert!(err.contains("not installed"), "{err}");
        // it all happened under the throw-away HOME
        assert!(home.path().join("data/kuro/extensions").exists());
    }

    #[test]
    fn mistakes_get_a_usage_message_and_a_non_zero_exit() {
        let home = tempfile::tempdir().unwrap();
        for args in [
            &["ext"][..],
            &["ext", "frobnicate"],
            &["ext", "install"],
            &["ext", "remove"],
            &["ext", "remove", "a", "b"],
            &["ext", "list", "x"],
            &["ext", "install", "--bogus", "d"],
        ] {
            let (code, _, err) = kuro(home.path(), args);
            assert_eq!(code, 1, "{args:?}");
            assert!(err.contains("usage:") || err.contains("unknown"), "{args:?}: {err}");
        }
        let (code, out, _) = kuro(home.path(), &["ext", "--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("kuro ext install"));
    }

    #[test]
    fn an_invalid_manifest_gets_a_clear_message() {
        let home = tempfile::tempdir().unwrap();
        let src = demo(home.path());
        std::fs::write(src.join("kuro-ext.toml"), "format = 1\nname = \"demo\"\nversion = \"1\"\nkuro = \">=0.1\"\ncolour = \"#ff0000\"\n")
            .unwrap();
        let (code, _, err) = kuro(home.path(), &["ext", "install", src.to_str().unwrap()]);
        assert_eq!(code, 1);
        assert!(err.starts_with("cannot install:") && err.contains("colour"), "{err}");
        assert_eq!(kuro(home.path(), &["ext", "list"]).1, "no extensions installed\n");
    }

    #[test]
    fn the_bundled_ksystem_extension_installs_cleanly() {
        let home = tempfile::tempdir().unwrap();
        let src = format!("{}/extensions/ksystem", env!("CARGO_MANIFEST_DIR"));
        let (code, out, err) = kuro(home.path(), &["ext", "install", &src]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "installed `ksystem` 0.1.0 (languages: ksystem)\n");
    }
}
