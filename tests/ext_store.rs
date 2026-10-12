//! Installing, listing, removing and loading extensions, in throw-away directories (never the user's real ones).

use kuro::ext::seal;
use kuro::ext::store::{self, ExtPaths, Status};
use std::fs;
use std::path::{Path, PathBuf};

const V: &str = "0.1.0";

const MANIFEST: &str = r#"
format = 1
name = "demo"
version = "1.0.0"
kuro = ">=0.1, <0.2"
description = "a demo language"

[[language]]
name = "Demo"
extensions = ["dm"]
keywords = ["if", "then", "else"]
line_comments = ["//"]
string_quotes = ["\""]
"#;

struct Env {
    dir: tempfile::TempDir,
    paths: ExtPaths,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let paths = ExtPaths { root: dir.path().join("data/extensions"), key: dir.path().join("state/ext.key") };
    Env { dir, paths }
}

fn source(e: &Env, name: &str, manifest: &str) -> PathBuf {
    let d = e.dir.path().join("src").join(name);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("kuro-ext.toml"), manifest).unwrap();
    fs::write(d.join("README.md"), "docs").unwrap();
    d
}

#[test]
fn install_list_load_and_remove_roundtrip() {
    let e = env();
    let src = source(&e, "demo", MANIFEST);
    let got = store::install(&e.paths, &src, false, V).unwrap();
    assert_eq!((got.name.as_str(), got.version.as_str(), got.replaced), ("demo", "1.0.0", false));
    assert_eq!(got.languages, vec!["Demo"]);
    assert!(e.paths.root.join("demo/kuro-ext.toml").is_file());
    assert!(e.paths.root.join("demo/README.md").is_file());

    let list = store::list(&e.paths).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].name.as_str(), list[0].status == Status::Ok, list[0].languages.clone()), ("demo", true, vec!["Demo".to_string()]));
    assert_eq!(list[0].kuro, ">=0.1, <0.2");

    let (loaded, warnings) = store::load_all(&e.paths, V);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].manifest.language[0].name, "Demo");

    assert_eq!(store::remove(&e.paths, "demo").unwrap(), "removed `demo`");
    assert!(!e.paths.root.join("demo").exists());
    assert!(store::list(&e.paths).unwrap().is_empty());
    assert!(store::load_all(&e.paths, V).0.is_empty());
    assert!(store::remove(&e.paths, "demo").unwrap_err().contains("not installed"));
}

#[test]
fn installing_twice_needs_force_and_force_replaces() {
    let e = env();
    let src = source(&e, "demo", MANIFEST);
    store::install(&e.paths, &src, false, V).unwrap();
    let err = store::install(&e.paths, &src, false, V).unwrap_err();
    assert!(err.contains("already installed") && err.contains("--force"), "{err}");
    fs::write(src.join("kuro-ext.toml"), MANIFEST.replace("1.0.0", "2.0.0")).unwrap();
    let got = store::install(&e.paths, &src, true, V).unwrap();
    assert!(got.replaced);
    assert_eq!(store::list(&e.paths).unwrap()[0].version, "2.0.0");
    assert!(store::load_all(&e.paths, V).1.is_empty());
    // no staging or old folders are left behind
    let hidden: Vec<_> = fs::read_dir(&e.paths.root)
        .unwrap()
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".staging") || n.starts_with(".old"))
        .collect();
    assert!(hidden.is_empty(), "{hidden:?}");
}

#[test]
fn an_invalid_manifest_is_rejected_with_a_clear_message_and_installs_nothing() {
    let e = env();
    for (bad, needle) in [
        (MANIFEST.replace("format = 1", "format = 7"), "format = 7 is not supported"),
        (MANIFEST.replace("name = \"demo\"", "name = \"Not Valid\""), "name `Not Valid`"),
        (MANIFEST.replace(">=0.1, <0.2", ">=5"), "needs kuro >=5"),
        (MANIFEST.replace("[[language]]", "[[language]]\ncolor = \"#ff0000\""), "color"),
        ("not toml at all = =".to_string(), "not valid"),
        (MANIFEST.replace("keywords = [", "roles = { x = 300 }\nkeywords = ["), "no RGB, no 256-colour"),
    ] {
        let src = source(&e, "bad", &bad);
        let err = store::install(&e.paths, &src, false, V).unwrap_err();
        assert!(err.contains(needle), "wanted {needle:?} in {err:?}");
        assert!(store::list(&e.paths).unwrap().is_empty(), "nothing registered");
        assert!(!e.paths.root.join("demo").exists() && !e.paths.root.join("bad").exists());
    }
}

#[test]
fn unsafe_sources_are_refused() {
    let e = env();
    // not a folder / no manifest / a symlink / a hidden file / too many files
    let file = e.dir.path().join("plain.txt");
    fs::write(&file, "x").unwrap();
    assert!(store::install(&e.paths, &file, false, V).unwrap_err().contains("not a directory"));
    let empty = e.dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(store::install(&e.paths, &empty, false, V).unwrap_err().contains("no kuro-ext.toml"));
    assert!(store::install(&e.paths, &e.dir.path().join("nope"), false, V).is_err());

    let src = source(&e, "demo", MANIFEST);
    std::os::unix::fs::symlink("/etc/passwd", src.join("link")).unwrap();
    assert!(store::install(&e.paths, &src, false, V).unwrap_err().contains("symbolic link"));
    fs::remove_file(src.join("link")).unwrap();
    fs::write(src.join(".hidden"), "x").unwrap();
    assert!(store::install(&e.paths, &src, false, V).unwrap_err().contains("hidden"));
    fs::remove_file(src.join(".hidden")).unwrap();
    for i in 0..store::MAX_FILES + 1 {
        fs::write(src.join(format!("f{i}")), "x").unwrap();
    }
    assert!(store::install(&e.paths, &src, false, V).unwrap_err().contains("too large"));
    assert!(store::list(&e.paths).unwrap().is_empty());
}

/// An extension is only loadable if kuro itself installed it: no seal, no load.
mod only_kuro_loads_extensions {
    use super::*;

    fn installed() -> Env {
        let e = env();
        let src = source(&e, "demo", MANIFEST);
        store::install(&e.paths, &src, false, V).unwrap();
        e
    }

    #[test]
    fn an_edited_file_fails_the_seal_and_is_skipped_with_a_warning() {
        let e = installed();
        fs::write(e.paths.root.join("demo/kuro-ext.toml"), MANIFEST.replace("\"if\"", "\"if\", \"evil\"")).unwrap();
        let (loaded, warnings) = store::load_all(&e.paths, V);
        assert!(loaded.is_empty());
        assert!(warnings[0].contains("do not match the seal"), "{warnings:?}");
        assert_eq!(store::list(&e.paths).unwrap()[0].status, Status::BadSeal);
        // an added file counts too
        let e = installed();
        fs::write(e.paths.root.join("demo/extra.txt"), "x").unwrap();
        assert!(store::load_all(&e.paths, V).0.is_empty());
    }

    #[test]
    fn a_folder_copied_in_by_hand_is_not_loaded() {
        let e = installed();
        let copy = e.paths.root.join("handmade");
        fs::create_dir_all(&copy).unwrap();
        fs::copy(e.paths.root.join("demo/kuro-ext.toml"), copy.join("kuro-ext.toml")).unwrap();
        let (loaded, _) = store::load_all(&e.paths, V);
        assert_eq!(loaded.len(), 1, "only the registered, sealed one");
        assert_eq!(loaded[0].manifest.name, "demo");
        let list = store::list(&e.paths).unwrap();
        assert!(list.iter().any(|l| l.name == "handmade" && l.status == Status::Incomplete));
        assert!(store::remove(&e.paths, "handmade").unwrap().contains("incomplete"));
        assert_eq!(store::list(&e.paths).unwrap().len(), 1);
    }

    #[test]
    fn another_users_key_or_a_missing_key_cannot_load_it() {
        let e = installed();
        // another machine / user: same files and registry, a different secret
        let other = ExtPaths { root: e.paths.root.clone(), key: e.dir.path().join("elsewhere/ext.key") };
        seal::key_or_create(&other.key).unwrap();
        let (loaded, warnings) = store::load_all(&other, V);
        assert!(loaded.is_empty() && warnings[0].contains("seal"), "{warnings:?}");
        // no key at all
        fs::remove_file(&e.paths.key).unwrap();
        let (loaded, warnings) = store::load_all(&e.paths, V);
        assert!(loaded.is_empty() && warnings[0].contains("key"), "{warnings:?}");
    }

    #[test]
    fn a_copy_of_the_installed_tree_in_another_place_does_not_load() {
        let e = installed();
        let dest = e.dir.path().join("copy/extensions");
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(dest.join("demo")).unwrap();
        for f in ["kuro-ext.toml", "README.md"] {
            fs::copy(e.paths.root.join("demo").join(f), dest.join("demo").join(f)).unwrap();
        }
        // without the registry entry nothing is loadable...
        let p = ExtPaths { root: dest.clone(), key: e.paths.key.clone() };
        assert!(store::load_all(&p, V).0.is_empty());
        // ...and even with it copied, a different key (the other user's) fails
        fs::copy(e.paths.root.join("registry.toml"), dest.join("registry.toml")).unwrap();
        let p2 = ExtPaths { root: dest, key: e.dir.path().join("k2/ext.key") };
        seal::key_or_create(&p2.key).unwrap();
        assert!(store::load_all(&p2, V).0.is_empty());
    }

    #[test]
    fn a_registry_entry_whose_files_are_gone_is_reported_not_fatal() {
        let e = installed();
        fs::remove_dir_all(e.paths.root.join("demo")).unwrap();
        let (loaded, warnings) = store::load_all(&e.paths, V);
        assert!(loaded.is_empty() && warnings[0].contains("files are missing"), "{warnings:?}");
        assert_eq!(store::list(&e.paths).unwrap()[0].status, Status::MissingFiles);
        assert!(store::remove(&e.paths, "demo").is_ok(), "the entry can still be removed");
    }

    #[test]
    fn after_a_kuro_upgrade_an_incompatible_extension_is_skipped_with_the_reason() {
        let e = installed();
        let (loaded, warnings) = store::load_all(&e.paths, "0.2.0");
        assert!(loaded.is_empty());
        assert!(warnings[0].contains("needs kuro >=0.1, <0.2 but this is kuro 0.2.0"), "{warnings:?}");
    }

    #[test]
    fn a_damaged_registry_is_reported_and_nothing_loads() {
        let e = installed();
        fs::write(e.paths.root.join("registry.toml"), "[[extension]\nbroken").unwrap();
        let (loaded, warnings) = store::load_all(&e.paths, V);
        assert!(loaded.is_empty() && warnings[0].contains("registry.toml is damaged"), "{warnings:?}");
        assert!(store::install(&e.paths, &e.dir.path().join("src/demo"), false, V).unwrap_err().contains("damaged"));
    }
}

#[test]
fn leftovers_of_an_interrupted_install_are_swept_and_never_loaded() {
    let e = env();
    fs::create_dir_all(e.paths.root.join(".staging-123-ab")).unwrap();
    fs::write(e.paths.root.join(".staging-123-ab/kuro-ext.toml"), MANIFEST).unwrap();
    fs::create_dir_all(e.paths.root.join(".old-123-cd")).unwrap();
    assert!(store::load_all(&e.paths, V).0.is_empty());
    let src = source(&e, "demo", MANIFEST);
    store::install(&e.paths, &src, false, V).unwrap();
    assert!(!e.paths.root.join(".staging-123-ab").exists() && !e.paths.root.join(".old-123-cd").exists());
}

#[test]
fn a_folder_with_no_registry_entry_is_incomplete_and_never_loads() {
    // what a crash between "rename into place" and "write the registry" leaves behind
    let e = env();
    let src = source(&e, "demo", MANIFEST);
    fs::create_dir_all(&e.paths.root).unwrap();
    fs::create_dir_all(e.paths.root.join("demo")).unwrap();
    fs::copy(src.join("kuro-ext.toml"), e.paths.root.join("demo/kuro-ext.toml")).unwrap();
    assert!(store::load_all(&e.paths, V).0.is_empty());
    assert_eq!(store::list(&e.paths).unwrap()[0].status, Status::Incomplete);
    // installing over it works (it was never registered), and so does a fresh install after removing it
    let got = store::install(&e.paths, &src, false, V).unwrap();
    assert!(!got.replaced);
    assert_eq!(store::load_all(&e.paths, V).0.len(), 1);
}

#[test]
fn two_installs_at_once_both_end_up_registered() {
    let e = env();
    let a = source(&e, "a", &MANIFEST.replace("name = \"demo\"", "name = \"alpha\"").replace("Demo", "Alpha"));
    let b = source(&e, "b", &MANIFEST.replace("name = \"demo\"", "name = \"beta\"").replace("Demo", "Beta"));
    let (pa, pb) = (e.paths.clone(), e.paths.clone());
    let t1 = std::thread::spawn(move || store::install(&pa, &a, false, V));
    let t2 = std::thread::spawn(move || store::install(&pb, &b, false, V));
    t1.join().unwrap().unwrap();
    t2.join().unwrap().unwrap();
    let names: Vec<String> = store::list(&e.paths).unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let (loaded, warnings) = store::load_all(&e.paths, V);
    assert_eq!((loaded.len(), warnings.len()), (2, 0));
}

#[test]
fn removing_validates_the_name_so_it_cannot_leave_the_extensions_folder() {
    let e = env();
    let victim = e.dir.path().join("victim");
    fs::create_dir_all(&victim).unwrap();
    for bad in ["../victim", "/etc", "..", "a/b", ""] {
        assert!(store::remove(&e.paths, bad).unwrap_err().contains("not a valid extension name"), "{bad:?}");
    }
    assert!(Path::new(&victim).exists());
}

#[test]
fn the_key_file_is_private_and_the_registry_names_no_secret() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    store::install(&e.paths, &source(&e, "demo", MANIFEST), false, V).unwrap();
    assert_eq!(fs::metadata(&e.paths.key).unwrap().permissions().mode() & 0o777, 0o600);
    let key = fs::read(&e.paths.key).unwrap();
    let reg = fs::read_to_string(e.paths.root.join("registry.toml")).unwrap();
    assert!(!reg.contains(&seal::hex(&key)), "the key never appears in the registry");
    assert!(reg.contains("seal ="));
}
