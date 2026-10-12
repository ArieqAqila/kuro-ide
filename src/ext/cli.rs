//! `kuro ext install|remove|list`: the command line of the extension system. Pure (returns what to print), so tests drive it directly.

use super::store::{self, ExtPaths, Status};
use std::path::Path;

pub const USAGE: &str = "usage:
    kuro ext install DIR [--force]   install the extension in DIR (a folder containing kuro-ext.toml)
    kuro ext remove NAME             remove an installed extension
    kuro ext list                    show the installed extensions
See docs/EXTENSIONS.md.";

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

/// `args` are the words after `kuro ext`.
pub fn run(args: &[String], paths: &ExtPaths, kuro_version: &str) -> Outcome {
    match args.first().map(String::as_str) {
        Some("install") => {
            let mut force = false;
            let mut dir = None;
            for a in &args[1..] {
                match a.as_str() {
                    "--force" | "-f" => force = true,
                    s if s.starts_with('-') => return fail(format!("unknown option {s}\n{USAGE}")),
                    s if dir.is_none() => dir = Some(s),
                    _ => return fail(format!("install takes one folder\n{USAGE}")),
                }
            }
            let Some(dir) = dir else { return fail(format!("install needs a folder\n{USAGE}")) };
            match store::install(paths, Path::new(dir), force, kuro_version) {
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
