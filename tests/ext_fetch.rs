//! `kuro ext install` from a git repository (`URL`, `URL@ref`) and from the registry (`NAME`). Only local `file://` repositories made by the
//! tests are used: no network, and nothing outside the temporary folders.

use kuro::ext::cli::{self, Options};
use kuro::ext::fetch;
use kuro::ext::store::{self, ExtPaths};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

const V: &str = "0.2.0";

fn manifest(name: &str, version: &str) -> String {
    format!(
        "format = 1\nname = \"{name}\"\nversion = \"{version}\"\nkuro = \">=0.2, <0.3\"\ndescription = \"test\"\n\n[[language]]\nname = \"{name}\"\nextensions = [\"{name}\"]\nkeywords = [\"if\"]\nline_comments = [\"//\"]\n"
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository with the given files, committed.
fn repo(dir: &Path, files: &[(&str, String)]) -> String {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["-c", "init.defaultBranch=main", "init", "-q", "."]);
    commit(dir, files, "first");
    format!("file://{}", dir.display())
}

fn commit(dir: &Path, files: &[(&str, String)], msg: &str) -> String {
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

struct Env {
    dir: tempfile::TempDir,
    paths: ExtPaths,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let paths = ExtPaths { root: dir.path().join("data/extensions"), key: dir.path().join("state/ext.key") };
    Env { dir, paths }
}

impl Env {
    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// `kuro ext <args>`, with the answer `answer` to the question, which is collected in `asked`.
    fn run(&self, args: &[&str], registry: &str, answer: bool, asked: &Mutex<Vec<String>>) -> cli::Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let ask = |q: &str| {
            asked.lock().unwrap().push(q.to_string());
            answer
        };
        cli::run_with(&args, &self.paths, V, &Options { registry: registry.into(), ask: &ask })
    }

    fn installed(&self) -> Vec<String> {
        store::list(&self.paths).unwrap().into_iter().map(|i| format!("{} {}", i.name, i.version)).collect()
    }
}

fn none() -> Mutex<Vec<String>> {
    Mutex::new(vec![])
}

#[test]
fn a_git_url_is_downloaded_shown_and_installed_with_yes() {
    let e = env();
    let url = repo(&e.path("src/demo"), &[("kuro-ext.toml", manifest("demo", "1.0.0")), ("README.md", "hi".into())]);
    let asked = none();
    let out = e.run(&["install", &url, "--yes"], "unused", false, &asked);
    assert_eq!((out.code, out.stdout.as_str()), (0, "installed `demo` 1.0.0 (languages: demo)\n"), "{}", out.stderr);
    assert!(asked.lock().unwrap().is_empty(), "--yes: no question");
    assert_eq!(e.installed(), ["demo 1.0.0"]);
    let listed = store::list(&e.paths).unwrap();
    assert_eq!(listed[0].source, url, "the list says where it came from");
    // it really loads
    let (loaded, warnings) = store::load_all(&e.paths, V);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.len(), 1);
    // and the files are the repository's files, not its .git
    assert!(e.paths.root.join("demo/kuro-ext.toml").is_file() && !e.paths.root.join("demo/.git").exists());
}

#[test]
fn without_yes_it_describes_the_download_and_asks_and_n_installs_nothing() {
    let e = env();
    let url = repo(&e.path("src/demo"), &[("kuro-ext.toml", manifest("demo", "1.0.0"))]);
    let asked = none();
    let out = e.run(&["install", &url], "unused", false, &asked);
    assert_eq!(out.code, 1);
    let q = asked.lock().unwrap().clone();
    assert_eq!(q.len(), 1);
    assert!(q[0].contains("`demo`") && q[0].contains("1.0.0") && q[0].contains("languages: demo") && q[0].contains(&url), "{}", q[0]);
    assert!(out.stderr.contains("not installed") && out.stderr.contains("--yes"), "{}", out.stderr);
    assert!(e.installed().is_empty(), "nothing was installed");
    // the same with the answer yes
    let out = e.run(&["install", &url], "unused", true, &none());
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(e.installed(), ["demo 1.0.0"]);
}

#[test]
fn a_branch_a_tag_or_a_commit_can_be_chosen_with_an_at_sign() {
    let e = env();
    let dir = e.path("src/demo");
    let url = repo(&dir, &[("kuro-ext.toml", manifest("demo", "1.0.0"))]);
    let first = git(&dir, &["rev-parse", "HEAD"]);
    git(&dir, &["tag", "v1"]);
    commit(&dir, &[("kuro-ext.toml", manifest("demo", "2.0.0"))], "second");
    git(&dir, &["branch", "old", &first]);
    for (reference, want) in [("v1", "1.0.0"), ("old", "1.0.0"), (first.as_str(), "1.0.0"), ("main", "2.0.0")] {
        let e2 = env();
        let out = e2.run(&["install", &format!("{url}@{reference}"), "--yes"], "unused", false, &none());
        assert_eq!(out.code, 0, "{reference}: {}", out.stderr);
        assert_eq!(e2.installed(), [format!("demo {want}")], "at {reference}");
        assert_eq!(store::list(&e2.paths).unwrap()[0].source, format!("{url}@{reference}"));
    }
    // no @: the default branch
    let out = e.run(&["install", &url, "--yes"], "unused", false, &none());
    assert_eq!(out.code, 0);
    assert_eq!(e.installed(), ["demo 2.0.0"]);
    // a reference that does not exist
    let out = env().run(&["install", &format!("{url}@nope"), "--yes"], "unused", false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("cannot download") && out.stderr.contains(" at nope"), "{}", out.stderr);
}

#[test]
fn a_bare_name_is_looked_up_in_the_registry_repository() {
    let e = env();
    let reg = repo(
        &e.path("registry"),
        &[
            ("README.md", "registry".into()),
            ("alpha/kuro-ext.toml", manifest("alpha", "0.1.0")),
            ("alpha/README.md", "a".into()),
            ("beta/kuro-ext.toml", manifest("beta", "0.2.0")),
        ],
    );
    let out = e.run(&["install", "beta", "--yes"], &reg, false, &none());
    assert_eq!((out.code, out.stdout.as_str()), (0, "installed `beta` 0.2.0 (languages: beta)\n"), "{}", out.stderr);
    assert_eq!(e.installed(), ["beta 0.2.0"], "only the named folder");
    assert!(store::list(&e.paths).unwrap()[0].source.contains("(beta)"));
    // the question names the registry
    let asked = none();
    e.run(&["install", "alpha"], &reg, false, &asked);
    assert!(asked.lock().unwrap()[0].contains(&reg), "{:?}", asked.lock().unwrap());
    // an unknown name
    let out = e.run(&["install", "gamma", "--yes"], &reg, false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("no extension named `gamma`"), "{}", out.stderr);
}

#[test]
fn an_existing_folder_wins_over_a_name_and_is_installed_without_a_question() {
    let e = env();
    let dir = e.path("work/demo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kuro-ext.toml"), manifest("demo", "1.0.0")).unwrap();
    let asked = none();
    let out = e.run(&["install", dir.to_str().unwrap()], "unused", false, &asked);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(asked.lock().unwrap().is_empty(), "a folder of your own is not a download");
}

#[test]
fn bad_downloads_are_refused_with_a_clear_message_and_install_nothing() {
    let e = env();
    // no manifest
    let url = repo(&e.path("src/nomanifest"), &[("README.md", "x".into())]);
    let out = e.run(&["install", &url, "--yes"], "unused", false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("kuro-ext.toml"), "{}", out.stderr);
    // an invalid manifest (a colour is not allowed)
    let bad = repo(&e.path("src/bad"), &[("kuro-ext.toml", format!("{}colour = \"#ff0000\"\n", manifest("bad", "1.0.0")))]);
    let out = e.run(&["install", &bad, "--yes"], "unused", false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("cannot install"), "{}", out.stderr);
    // a symbolic link in the repository
    let linked = e.path("src/linked");
    let lurl = repo(&linked, &[("kuro-ext.toml", manifest("linked", "1.0.0"))]);
    std::os::unix::fs::symlink("/etc/passwd", linked.join("passwd")).unwrap();
    git(&linked, &["add", "-A"]);
    git(&linked, &["commit", "-q", "-m", "link"]);
    let out = e.run(&["install", &lurl, "--yes"], "unused", false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("symbolic link"), "{}", out.stderr);
    // a repository that is not there
    let out = e.run(&["install", "file:///nonexistent-kuro-test-repo", "--yes"], "unused", false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("cannot download"), "{}", out.stderr);
    assert!(e.installed().is_empty());
}

#[test]
fn a_url_cannot_make_git_run_a_command() {
    let e = env();
    let marker = e.path("pwned");
    for url in [format!("ext::sh -c touch% {}", marker.display()), "--upload-pack=touch x".to_string(), "-oProxyCommand=id".to_string()] {
        let out = e.run(&["install", &url, "--yes"], "unused", false, &none());
        assert_eq!(out.code, 1, "{url}");
    }
    // the registry setting is held to the same rule
    let out = e.run(&["install", "ksystem", "--yes"], &format!("ext::sh -c touch% {}", marker.display()), false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("registry"), "{}", out.stderr);
    assert!(!marker.exists(), "nothing was run");
}

#[test]
fn a_second_install_needs_force_and_replaces_the_first() {
    let e = env();
    let dir = e.path("src/demo");
    let url = repo(&dir, &[("kuro-ext.toml", manifest("demo", "1.0.0"))]);
    assert_eq!(e.run(&["install", &url, "--yes"], "x", false, &none()).code, 0);
    commit(&dir, &[("kuro-ext.toml", manifest("demo", "1.1.0"))], "update");
    let out = e.run(&["install", &url, "--yes"], "x", false, &none());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("already installed") && out.stderr.contains("--force"), "{}", out.stderr);
    let out = e.run(&["install", &url, "--yes", "--force"], "x", false, &none());
    assert_eq!((out.code, out.stdout.as_str()), (0, "replaced `demo` 1.1.0 (languages: demo)\n"), "{}", out.stderr);
}

#[test]
fn the_downloaded_files_are_deleted_when_the_download_is_dropped() {
    let e = env();
    let url = repo(&e.path("src/demo"), &[("kuro-ext.toml", manifest("demo", "1.0.0"))]);
    let src = fetch::classify(&url).unwrap();
    let fetched = fetch::fetch(&src, "x").unwrap().expect("a download");
    let dir = fetched.dir.clone();
    assert!(dir.join("kuro-ext.toml").is_file());
    drop(fetched);
    assert!(!dir.exists(), "the temporary copy is gone");
}
