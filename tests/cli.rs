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
    // "kuro 0.2.0 (775d078)" or "(775d078-dirty)", or "(unknown)" outside a git checkout
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
            "format = 1\nname = \"demo\"\nversion = \"1.0.0\"\nkuro = \">=0.2, <0.3\"\n[[language]]\nname = \"Demo\"\nextensions = [\"dm\"]\nkeywords = [\"if\"]\n",
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
        assert!(out.starts_with("demo 1.0.0  languages: Demo  kuro: >=0.2, <0.3  [ok]"), "{out}");
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

// ---------------------------------------------------------------- kuro git (the real binary, real throw-away repositories)

mod git_cli {
    use std::path::Path;
    use std::process::Command;

    fn kuro_git(cwd: &Path, args: &[&str], path: Option<&str>) -> (i32, String, String) {
        let mut c = Command::new(env!("CARGO_BIN_EXE_kuro"));
        c.arg("git")
            .args(args)
            .current_dir(cwd)
            .env("HOME", cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("KURO_CONFIG_DIR");
        if let Some(p) = path {
            c.env("PATH", p);
        }
        let out = c.output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn repo_with_a_commit() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "-q", "."],
            vec!["config", "user.name", "T"],
            vec!["config", "user.email", "t@e.invalid"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            assert!(
                Command::new("git").args(&args).current_dir(d.path()).env("GIT_CONFIG_GLOBAL", "/dev/null").status().unwrap().success()
            );
        }
        std::fs::write(d.path().join("a.txt"), "hi\n").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-q", "-m", "first"]] {
            assert!(
                Command::new("git").args(&args).current_dir(d.path()).env("GIT_CONFIG_GLOBAL", "/dev/null").status().unwrap().success()
            );
        }
        d
    }

    #[test]
    fn outside_a_repository_it_prints_exactly_direktori_tidak_ada_git_and_exits_non_zero() {
        let d = tempfile::tempdir().unwrap();
        let (code, out, err) = kuro_git(d.path(), &[], None);
        assert_eq!(err, "direktori tidak ada git\n");
        assert_eq!(out, "");
        assert_eq!(code, 128);
        // also with -fl, and from a subfolder
        let (code, _, err) = kuro_git(d.path(), &["-fl", "x.txt"], None);
        assert_eq!((code, err.as_str()), (128, "direktori tidak ada git\n"));
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let (code, _, err) = kuro_git(&d.path().join("sub"), &[], None);
        assert_eq!((code, err.as_str()), (128, "direktori tidak ada git\n"));
    }

    #[test]
    fn a_missing_file_gives_a_message_and_status_1_and_bad_arguments_status_2() {
        let r = repo_with_a_commit();
        let (code, out, err) = kuro_git(r.path(), &["-fl", "missing.txt"], None);
        assert_eq!((code, out.as_str(), err.as_str()), (1, "", "no history for missing.txt\n"));
        let (code, _, err) = kuro_git(r.path(), &["-fl", "../outside.txt"], None);
        assert_eq!(code, 1);
        assert!(err.contains("outside the repository"), "{err}");
        let (code, _, err) = kuro_git(r.path(), &["-fl"], None);
        assert_eq!(code, 2);
        assert!(err.contains("-fl needs a file name"), "{err}");
        let (code, _, err) = kuro_git(r.path(), &["--bogus"], None);
        assert_eq!(code, 2);
        assert!(err.contains("unknown argument"), "{err}");
        let (code, out, _) = kuro_git(r.path(), &["--help"], None);
        assert_eq!(code, 0);
        assert!(out.contains("kuro git -fl FILE"));
    }

    #[test]
    fn without_the_git_program_it_says_so() {
        let r = repo_with_a_commit();
        let (code, _, err) = kuro_git(r.path(), &[], Some("/nonexistent-bin-dir"));
        assert_eq!((code, err.as_str()), (127, "git is not installed\n"));
    }
}

// ---------------------------------------------------------------- kuro history (the real binary, a throw-away state folder)

mod history_cli {
    use kuro::history::{HistoryOptions, Store};
    use std::path::Path;
    use std::process::Command;

    fn kuro(home: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_kuro"))
            .arg("history")
            .args(args)
            .env("HOME", home)
            .env("XDG_STATE_HOME", home.join("state"))
            .env_remove("KURO_CONFIG_DIR")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn store(home: &Path) -> Store {
        Store::new(home.join("state/kuro/history"), HistoryOptions::default())
    }

    fn record(home: &Path, name: &str, original: &str, new: &str) -> std::path::PathBuf {
        let p = home.join("work").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, original).unwrap();
        let s = store(home);
        s.before_write(&p, Some(original.as_bytes()), new.len() as u64).unwrap();
        std::fs::write(&p, new).unwrap();
        s.after_write(&p, Some(original.as_bytes()), new.as_bytes());
        p
    }

    #[test]
    fn list_show_delete_and_clear() {
        let home = tempfile::tempdir().unwrap();
        let (code, out, _) = kuro(home.path(), &["list"]);
        assert_eq!((code, out.as_str()), (0, "no history: kuro has not written any file yet\n"));
        let a = record(home.path(), "a.txt", "a0\n", "a1\n");
        let b = record(home.path(), "b.txt", "b0\n", "b1\n");
        let c = record(home.path(), "c.txt", "c0\n", "c1\n");
        let (code, out, _) = kuro(home.path(), &["list"]);
        assert_eq!(code, 0);
        assert!(out.contains("a.txt") && out.contains("b.txt") && out.contains("c.txt"), "{out}");
        assert_eq!(out.matches("changed ").count(), 3, "{out}");
        assert!(out.contains("1 write, 0 outside, 0 restored") && out.contains("3 files"), "{out}");
        let key = kuro::history::key_of(&a);
        let (code, out, _) = kuro(home.path(), &["show", &key[..6]]);
        assert_eq!(code, 0);
        assert!(out.contains("a.txt") && out.contains("original: 3 bytes") && out.contains("kuro wrote it"), "{out}");
        // delete two at once (the third stays); the files themselves are not touched
        let (code, out, _) = kuro(home.path(), &["delete", &kuro::history::key_of(&a), &kuro::history::key_of(&b)[..8]]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(out.matches("deleted ").count(), 2);
        assert_eq!(store(home.path()).entries().len(), 1);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a1\n");
        // clear needs --yes when there is no terminal to ask on
        let (code, _, err) = kuro(home.path(), &["clear"]);
        assert_eq!(code, 1);
        assert!(err.contains("--yes"), "{err}");
        assert_eq!(store(home.path()).entries().len(), 1, "nothing deleted without the answer");
        let (code, out, _) = kuro(home.path(), &["clear", "--yes"]);
        assert_eq!((code, out.as_str()), (0, "deleted the history of 1 file\n"));
        assert!(store(home.path()).entries().is_empty());
        let _ = c;
    }

    #[test]
    fn mistakes_say_what_is_wrong() {
        let home = tempfile::tempdir().unwrap();
        record(home.path(), "a.txt", "x", "y");
        for args in [&[][..], &["frobnicate"], &["show"], &["delete"], &["list", "extra"]] {
            let (code, _, err) = kuro(home.path(), args);
            assert_eq!(code, 1, "{args:?}");
            assert!(err.contains("usage:"), "{args:?}: {err}");
        }
        let (code, _, err) = kuro(home.path(), &["show", "zzzz"]);
        assert_eq!(code, 1);
        assert!(err.contains("no history"), "{err}");
        let (code, _, err) = kuro(home.path(), &["delete", "zzzz"]);
        assert_eq!(code, 1);
        assert!(err.contains("no history"), "{err}");
        assert_eq!(store(home.path()).entries().len(), 1, "a failed delete deletes nothing");
        let (code, out, _) = kuro(home.path(), &["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("kuro history delete"));
    }

    #[test]
    fn show_lists_outside_changes_and_restores() {
        let home = tempfile::tempdir().unwrap();
        let p = record(home.path(), "a.txt", "orig\n", "kuro\n");
        std::fs::write(&p, "other editor\n").unwrap();
        let s = store(home.path());
        s.note_open(&p, std::fs::read(&p).ok().as_deref());
        s.restore(&s.plan(Some(&p))[0]).unwrap();
        let (_, out, _) = kuro(home.path(), &["show", &kuro::history::key_of(&p)]);
        assert!(out.contains("changed OUTSIDE kuro") && out.contains("hard undo restored the original"), "{out}");
    }
}
