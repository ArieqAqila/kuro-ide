//! Getting an extension's source from where it lives: a folder, a git repository (`URL` or `URL@ref`), or a name in the extension
//! registry (a git repository with one folder per extension). The download is only a way to get a *folder*; what is in it goes through
//! exactly the checks of a local install, and nothing in it is ever executed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where bare names are looked up unless `KURO_EXT_REGISTRY` says otherwise.
pub const DEFAULT_REGISTRY: &str = "https://github.com/ArieqAqila/kuro-extensions";

/// What an `install` argument means.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    /// A folder on this computer.
    Dir(PathBuf),
    /// A git repository whose root holds `kuro-ext.toml`; `reference` is a branch, tag or commit (default: the default branch).
    Git { url: String, reference: Option<String> },
    /// A name: the folder `name/` of the registry repository.
    Named(String),
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !s.starts_with('-')
}

fn looks_like_url(s: &str) -> bool {
    s.contains("://") || (s.starts_with("git@") && s.contains(':'))
}

/// Decide what `arg` is. An existing folder always wins, so a local folder named like an extension is installed as a folder.
pub fn classify(arg: &str) -> Result<Source, String> {
    if Path::new(arg).is_dir() {
        return Ok(Source::Dir(PathBuf::from(arg)));
    }
    if looks_like_url(arg) {
        // `URL@ref`: the `@` after the last `/` (the `@` of `git@host:` is before it)
        let (url, reference) = match arg.rfind('@') {
            Some(i) if i > arg.rfind('/').unwrap_or(0) && !arg[i + 1..].is_empty() && !arg[i + 1..].contains(':') => {
                (arg[..i].to_string(), Some(arg[i + 1..].to_string()))
            }
            _ => (arg.to_string(), None),
        };
        check_url(&url)?;
        if let Some(r) = &reference {
            check_ref(r)?;
        }
        return Ok(Source::Git { url, reference });
    }
    if is_name(arg) {
        return Ok(Source::Named(arg.to_string()));
    }
    if arg.contains('/') || arg.starts_with('.') || arg.starts_with('~') {
        return Err(format!("{arg} is not a folder"));
    }
    Err(format!("`{arg}` is not a folder, a git URL or an extension name (lower-case letters, digits and `-`)"))
}

/// Only plain network and file transports; never git's `ext::` (which runs a command) or an option disguised as a URL.
fn check_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if url.starts_with('-') || url.contains("::") || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err(format!("`{url}` is not accepted as a git URL"));
    }
    let scheme_ok = ["https://", "http://", "ssh://", "git://", "file://"].iter().any(|s| lower.starts_with(s)) || url.starts_with("git@");
    if !scheme_ok {
        return Err(format!("`{url}`: only https, http, ssh, git and file URLs (and git@host:path) are accepted"));
    }
    Ok(())
}

fn check_ref(r: &str) -> Result<(), String> {
    if r.starts_with('-') || r.contains("..") || r.chars().any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c)) {
        return Err(format!("`{r}` is not accepted as a branch, tag or commit"));
    }
    Ok(())
}

/// A downloaded source: the folder to install from, and what to show as its origin. The files are deleted when this is dropped.
pub struct Fetched {
    pub dir: PathBuf,
    pub origin: String,
    _tmp: tempfile::TempDir,
}

fn git(args: &[&str], dir: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "https:http:ssh:git:file")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "git is not installed (needed to download an extension)".to_string()
            } else {
                e.to_string()
            }
        })?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr);
        Err(msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("git failed").trim().to_string())
    }
}

/// Download `url` (at `reference`) and unpack the tracked files into a fresh folder: no `.git`, no hooks, nothing outside the files
/// themselves. Symbolic links are kept as links so the installer can refuse them.
fn download(url: &str, reference: Option<&str>, subdir: Option<&str>) -> Result<Fetched, String> {
    let tmp = tempfile::tempdir().map_err(|e| format!("cannot make a temporary folder: {e}"))?;
    let repo = tmp.path().join("repo.git");
    let out = tmp.path().join("files");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    git(&["init", "--bare", "-q", "repo.git"], tmp.path())?;
    let r = reference.unwrap_or("HEAD");
    git(&["--git-dir", "repo.git", "fetch", "-q", "--depth", "1", "--no-tags", "--", url, r], tmp.path())
        .map_err(|e| format!("cannot download {url}{}: {e}", reference.map(|r| format!(" at {r}")).unwrap_or_default()))?;
    let index = tmp.path().join("index");
    let with_index = |args: &[&str]| -> Result<(), String> {
        let o = Command::new("git")
            .arg("--git-dir")
            .arg(&repo)
            .arg("--work-tree")
            .arg(tmp.path())
            .args(args)
            .current_dir(tmp.path())
            .env("GIT_INDEX_FILE", &index)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
    };
    with_index(&["read-tree", "FETCH_HEAD"])?;
    let prefix = format!("{}/", out.display());
    with_index(&["checkout-index", "-a", "-f", &format!("--prefix={prefix}")])?;
    let dir = match subdir {
        Some(s) => {
            let d = out.join(s);
            if !d.is_dir() {
                return Err(format!("the registry {url} has no extension named `{s}`"));
            }
            d
        }
        None => out,
    };
    let origin = match (subdir, reference) {
        (Some(s), _) => format!("{url} ({s})"),
        (None, Some(r)) => format!("{url}@{r}"),
        (None, None) => url.to_string(),
    };
    Ok(Fetched { dir, origin, _tmp: tmp })
}

/// Make the source of `install` available as a folder. `registry` is the repository bare names are looked up in.
pub fn fetch(source: &Source, registry: &str) -> Result<Option<Fetched>, String> {
    match source {
        Source::Dir(_) => Ok(None),
        Source::Git { url, reference } => download(url, reference.as_deref(), None).map(Some),
        Source::Named(name) => {
            check_url(registry).map_err(|e| format!("the extension registry: {e}"))?;
            download(registry, None, Some(name)).map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().display().to_string();
        assert_eq!(classify(&d).unwrap(), Source::Dir(dir.path().to_path_buf()));
        assert_eq!(classify("ksystem").unwrap(), Source::Named("ksystem".into()));
        assert_eq!(
            classify("https://github.com/x/kuro-ext-y").unwrap(),
            Source::Git { url: "https://github.com/x/kuro-ext-y".into(), reference: None }
        );
        assert_eq!(
            classify("https://github.com/x/y@v1.2").unwrap(),
            Source::Git { url: "https://github.com/x/y".into(), reference: Some("v1.2".into()) }
        );
        assert_eq!(classify("git@github.com:x/y.git").unwrap(), Source::Git { url: "git@github.com:x/y.git".into(), reference: None });
        assert_eq!(
            classify("git@github.com:x/y.git@abc1234").unwrap(),
            Source::Git { url: "git@github.com:x/y.git".into(), reference: Some("abc1234".into()) }
        );
        assert_eq!(
            classify("https://user@host.example/x/y").unwrap(),
            Source::Git { url: "https://user@host.example/x/y".into(), reference: None },
            "an @ before the last / belongs to the URL"
        );
    }

    #[test]
    fn dangerous_or_unclear_arguments_are_refused() {
        for bad in [
            "ext::sh -c touch /tmp/x",
            "ext::sh",
            "--upload-pack=x",
            "https://x/y z",
            "ftp://x/y",
            "ssh://x/y@--bad",
            "https://x/y@a..b",
            "UPPER",
            "no/such/dir",
            "./nodir",
            "~x",
        ] {
            assert!(classify(bad).is_err(), "{bad} must be refused");
        }
        assert!(check_url("-oProxyCommand=x").is_err());
        assert!(check_url("ext::sh -c id").is_err());
        assert!(check_ref("--x").is_err());
        assert!(check_ref("v1.0").is_ok());
    }
}
