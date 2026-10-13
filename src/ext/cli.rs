//! `kuro ext install|remove|list`: the command line of the extension system. Pure (returns what to print), so tests drive it directly.

use super::fetch::{self, Source};
use super::store::{self, ExtPaths, Status};

pub const USAGE: &str = "usage:
    kuro ext install SOURCE [--force] [--yes]   install an extension; SOURCE is one of
        DIR                  a folder containing kuro-ext.toml
        NAME                 an extension of the registry (default https://github.com/ArieqAqila/kuro-extensions; KURO_EXT_REGISTRY)
        URL or URL@REF       a git repository (https, ssh, git or file URL) holding kuro-ext.toml; REF is a branch, tag or commit
      A download shows what it is and asks first (--yes skips the question); nothing from it is ever executed.
    kuro ext remove NAME             remove an installed extension
    kuro ext list                    show the installed extensions
See docs/EXTENSIONS.md.";

/// What `install` needs from its surroundings: the registry for bare names, and the answer to "install this download?".
pub struct Options<'a> {
    pub registry: String,
    /// Asked with a description of what is about to be installed; true = go ahead.
    pub ask: &'a dyn Fn(&str) -> bool,
}

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn ok(s: String) -> Outcome {
    Outcome { code: 0, stdout: s, stderr: String::new() }
}

fn fail(s: String) -> Outcome {
    Outcome { code: 1, stdout: String::new(), stderr: s }
}

/// `args` are the words after `kuro ext`. Downloads are refused unless `--yes` is given (there is nobody to ask).
pub fn run(args: &[String], paths: &ExtPaths, kuro_version: &str) -> Outcome {
    run_with(args, paths, kuro_version, &Options { registry: fetch::DEFAULT_REGISTRY.into(), ask: &|_| false })
}

pub fn run_with(args: &[String], paths: &ExtPaths, kuro_version: &str, opts: &Options) -> Outcome {
    match args.first().map(String::as_str) {
        Some("install") => {
            let (mut force, mut yes) = (false, false);
            let mut arg = None;
            for a in &args[1..] {
                match a.as_str() {
                    "--force" | "-f" => force = true,
                    "--yes" | "-y" => yes = true,
                    s if s.starts_with('-') => return fail(format!("unknown option {s}\n{USAGE}")),
                    s if arg.is_none() => arg = Some(s),
                    _ => return fail(format!("install takes one source\n{USAGE}")),
                }
            }
            let Some(arg) = arg else { return fail(format!("install needs a source\n{USAGE}")) };
            let source = match fetch::classify(arg) {
                Ok(s) => s,
                Err(e) => return fail(format!("cannot install: {e}\n")),
            };
            let fetched = match fetch::fetch(&source, &opts.registry) {
                Ok(f) => f,
                Err(e) => return fail(format!("cannot install: {e}\n")),
            };
            let result = match (&source, &fetched) {
                (Source::Dir(d), _) => store::install(paths, d, force, kuro_version),
                (_, Some(f)) => {
                    // a download is shown and confirmed before it is installed
                    let m = match store::peek(&f.dir, kuro_version) {
                        Ok(m) => m,
                        Err(e) => return fail(format!("cannot install: {e}\n")),
                    };
                    let langs: Vec<&str> = m.language.iter().map(|l| l.name.as_str()).collect();
                    let what = format!("install `{}` {} (languages: {}) from {}?", m.name, m.version, langs.join(", "), f.origin);
                    if !yes && !(opts.ask)(&what) {
                        return fail(format!("not installed: {what}\n(answer y at the question, or use --yes)\n"));
                    }
                    store::install_from(paths, &f.dir, &f.origin, force, kuro_version)
                }
                (_, None) => unreachable!("a download always yields a folder"),
            };
            match result {
                Ok(i) => ok(format!(
                    "{} `{}` {} (languages: {})\n",
                    if i.replaced { "replaced" } else { "installed" },
                    i.name,
                    i.version,
                    i.languages.join(", ")
                )),
                Err(e) => fail(format!("cannot install: {e}\n")),
            }
        }
        Some("remove") | Some("uninstall") => match args.get(1) {
            Some(name) if args.len() == 2 => match store::remove(paths, name) {
                Ok(m) => ok(format!("{m}\n")),
                Err(e) => fail(format!("cannot remove: {e}\n")),
            },
            _ => fail(format!("remove takes one extension name\n{USAGE}")),
        },
        Some("list") if args.len() == 1 => match store::list(paths) {
            Ok(items) if items.is_empty() => ok("no extensions installed\n".into()),
            Ok(items) => {
                let mut out = String::new();
                for i in items {
                    let status = match i.status {
                        Status::Ok => "ok",
                        Status::BadSeal => "NOT LOADED: files do not match the seal (kuro ext install --force to repair)",
                        Status::MissingFiles => "NOT LOADED: files are missing (kuro ext remove to clean up)",
                        Status::Incomplete => "INCOMPLETE: an install was interrupted (kuro ext remove to clean up)",
                    };
                    out.push_str(&format!(
                        "{} {}  languages: {}  kuro: {}  [{}]\n    from {}\n",
                        i.name,
                        i.version,
                        if i.languages.is_empty() { "-".into() } else { i.languages.join(", ") },
                        i.kuro,
                        status,
                        i.source
                    ));
                }
                ok(out)
            }
            Err(e) => fail(format!("cannot list: {e}\n")),
        },
        Some("-h") | Some("--help") | Some("help") => ok(format!("{USAGE}\n")),
        Some(other) => fail(format!("unknown ext command `{other}`\n{USAGE}")),
        None => fail(USAGE.to_string() + "\n"),
    }
}
