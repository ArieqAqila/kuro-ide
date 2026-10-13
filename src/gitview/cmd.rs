//! Everything that talks to `git`. This is the **only** place that starts a git process, and it only runs read-only commands.
//!
//! Why the `git` command and not a library: it behaves exactly like the user's own git (their config, rename detection, worktrees,
//! huge repositories) and adds no dependency or C library to kuro. Every call uses `--no-pager`, `LC_ALL=C`, `GIT_OPTIONAL_LOCKS=0`
//! and no terminal prompts; commands that produce diffs also pass `--no-ext-diff --no-textconv`, so a repository can never make kuro run a
//! program it configured (`diff.external`, `textconv`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// The `git` program is not installed.
    NoGit,
    /// The directory is not inside a git repository.
    NotRepo,
    /// A command failed; the message is git's own.
    Failed(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::NoGit => write!(f, "git is not installed"),
            GitError::NotRepo => write!(f, "this directory is not in a git repository"),
            GitError::Failed(m) => write!(f, "{m}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub email: String,
    /// `2026-10-12 23:52`
    pub date: String,
    pub subject: String,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// `A`dded, `M`odified, `D`eleted, `R`enamed, `C`opied, `T`ype changed.
    pub status: char,
    pub path: String,
    /// The old path of a rename or copy.
    pub old_path: Option<String>,
    /// Similarity percentage of a rename or copy.
    pub score: Option<u8>,
}

impl Change {
    pub fn label(&self) -> String {
        match &self.old_path {
            Some(old) => format!("{} {old} → {}", self.status, self.path),
            None => format!("{} {}", self.status, self.path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    /// A detached HEAD, with the short hash it points at.
    Detached(String),
    /// A branch with no commits yet.
    Unborn(String),
}

impl Head {
    pub fn label(&self) -> String {
        match self {
            Head::Branch(b) => format!("on {b}"),
            Head::Detached(h) => format!("HEAD detached at {h}"),
            Head::Unborn(b) => format!("on {b}: no commits yet"),
        }
    }
}

pub struct Git {
    pub top: PathBuf,
    /// Extra environment for every call (tests isolate git from the user's configuration with it).
    pub env: Vec<(String, String)>,
}

const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';
const LOG_FORMAT: &str = "%x1e%H%x1f%h%x1f%an%x1f%ae%x1f%ad%x1f%s%x1f%P";

fn git_command(dir: &Path, env: &[(String, String)], args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("--no-pager").args(["-c", "core.quotepath=off", "-c", "core.fsmonitor=false"]).arg("-C").arg(dir).args(args);
    cmd.env("LC_ALL", "C").env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

fn run_in(dir: &Path, env: &[(String, String)], args: &[&str]) -> Result<Vec<u8>, GitError> {
    let mut cmd = git_command(dir, env, args);
    let out = cmd
        .output()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { GitError::NoGit } else { GitError::Failed(e.to_string()) })?;
    if out.status.success() { Ok(out.stdout) } else { Err(GitError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string())) }
}

/// The root of the repository containing `dir`, or `NotRepo`.
pub fn toplevel(dir: &Path, env: &[(String, String)]) -> Result<PathBuf, GitError> {
    match run_in(dir, env, &["rev-parse", "--show-toplevel"]) {
        Ok(out) => Ok(PathBuf::from(String::from_utf8_lossy(&out).trim_end_matches('\n'))),
        Err(GitError::NoGit) => Err(GitError::NoGit),
        // outside a repository, in a bare repository's git dir, or a directory that does not exist
        Err(_) => Err(GitError::NotRepo),
    }
}

fn text(b: Vec<u8>) -> String {
    String::from_utf8_lossy(&b).into_owned()
}

/// Parse the output of `log` with [`LOG_FORMAT`].
pub fn parse_log(out: &str) -> Vec<Commit> {
    out.split(RECORD)
        .filter(|r| !r.trim().is_empty())
        .filter_map(|r| {
            let mut f = r.split(FIELD);
            let hash = f.next()?.trim().to_string();
            let short = f.next()?.to_string();
            let author = f.next()?.to_string();
            let email = f.next()?.to_string();
            let date = f.next()?.to_string();
            let subject = f.next()?.to_string();
            let parents = f.next()?.split_whitespace().map(String::from).collect();
            Some(Commit { hash, short, author, email, date, subject, parents })
        })
        .collect()
}

/// Parse NUL-separated `--name-status` output (`M\0path\0R095\0old\0new\0`).
pub fn parse_name_status(out: &str) -> Vec<Change> {
    let mut toks = out.split('\0').map(|t| t.trim_matches('\n')).filter(|t| !t.is_empty());
    let mut changes = Vec::new();
    while let Some(st) = toks.next() {
        let status = st.chars().next().unwrap_or('M');
        let score = st[1..].parse::<u8>().ok();
        if matches!(status, 'R' | 'C') {
            let (Some(old), Some(new)) = (toks.next(), toks.next()) else { break };
            changes.push(Change { status, path: new.to_string(), old_path: Some(old.to_string()), score });
        } else {
            let Some(p) = toks.next() else { break };
            changes.push(Change { status, path: p.to_string(), old_path: None, score });
        }
    }
    changes
}

/// Parse `log --follow --name-status -z --format=`[`LOG_FORMAT`]: each commit with the change that touched the followed file.
pub fn parse_file_log(out: &str) -> Vec<(Commit, Option<Change>)> {
    out.split(RECORD)
        .filter(|r| !r.trim().is_empty())
        .filter_map(|r| {
            let (head, rest) = r.split_once('\0').unwrap_or((r, ""));
            let commit = parse_log(head).into_iter().next()?;
            Some((commit, parse_name_status(rest).into_iter().next()))
        })
        .collect()
}

/// Cuts the growing output of a `log` into whole records, so commits can be shown while git is still walking the history.
#[derive(Default)]
pub struct RecordStream {
    buf: Vec<u8>,
}

impl RecordStream {
    /// Add output; returns the commits whose records are complete (a record is complete when the next one has started).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<(Commit, Option<Change>)> {
        self.buf.extend_from_slice(bytes);
        match self.buf.iter().rposition(|b| *b == RECORD as u8) {
            Some(i) if i > 0 => {
                let done: Vec<u8> = self.buf.drain(..i).collect();
                parse_file_log(&String::from_utf8_lossy(&done))
            }
            _ => vec![],
        }
    }

    /// The output has ended: the last record is complete too.
    pub fn finish(&mut self) -> Vec<(Commit, Option<Change>)> {
        let rest = std::mem::take(&mut self.buf);
        parse_file_log(&String::from_utf8_lossy(&rest))
    }
}

/// What the reading of a file's history sends while it runs.
pub enum LogBatch {
    /// More commits, newest first.
    Items(Vec<(Commit, Option<Change>)>),
    /// git has finished (or failed, with its message).
    Done(Result<(), String>),
}

/// `git log --follow` running in the background: commits arrive as git finds them, so the list can be shown at once instead of
/// after the whole history was walked. Dropping it stops git.
pub struct FileLog {
    pub rx: std::sync::mpsc::Receiver<LogBatch>,
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
}

impl Drop for FileLog {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Git {
    /// Start reading the history of `path` (renames followed) in the background.
    pub fn spawn_file_log(&self, path: &str) -> Result<FileLog, GitError> {
        use std::io::Read;
        let format = format!("--format={LOG_FORMAT}");
        let mut cmd = git_command(
            &self.top,
            &self.env,
            &["log", "--follow", "--name-status", "-M", "-z", "--date=format:%Y-%m-%d %H:%M", &format, "--", path],
        );
        let mut child = cmd
            .spawn()
            .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { GitError::NoGit } else { GitError::Failed(e.to_string()) })?;
        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let child = std::sync::Arc::new(std::sync::Mutex::new(child));
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = child.clone();
        std::thread::spawn(move || {
            let mut stream = RecordStream::default();
            let mut chunk = [0u8; 16 * 1024];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let items = stream.push(&chunk[..n]);
                        if !items.is_empty() && tx.send(LogBatch::Items(items)).is_err() {
                            return; // nobody is listening any more
                        }
                    }
                }
            }
            let items = stream.finish();
            if !items.is_empty() && tx.send(LogBatch::Items(items)).is_err() {
                return;
            }
            let mut err = String::new();
            let _ = stderr.read_to_string(&mut err);
            let ok = waiter.lock().ok().and_then(|mut c| c.wait().ok()).is_some_and(|s| s.success());
            let _ = tx.send(LogBatch::Done(if ok { Ok(()) } else { Err(err.trim().to_string()) }));
        });
        Ok(FileLog { rx, child })
    }

    pub fn open(dir: &Path, env: Vec<(String, String)>) -> Result<Git, GitError> {
        let top = toplevel(dir, &env)?;
        Ok(Git { top, env })
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        run_in(&self.top, &self.env, args)
    }

    pub fn head(&self) -> Result<Head, GitError> {
        let branch =
            self.run(&["symbolic-ref", "-q", "--short", "HEAD"]).ok().map(|b| text(b).trim().to_string()).filter(|b| !b.is_empty());
        let commit = self
            .run(&["rev-parse", "--verify", "-q", "--short", "HEAD"])
            .ok()
            .map(|b| text(b).trim().to_string())
            .filter(|b| !b.is_empty());
        Ok(match (branch, commit) {
            (Some(b), Some(_)) => Head::Branch(b),
            (Some(b), None) => Head::Unborn(b),
            (None, Some(c)) => Head::Detached(c),
            (None, None) => Head::Unborn("HEAD".into()),
        })
    }

    pub fn has_commits(&self) -> bool {
        self.run(&["rev-parse", "--verify", "-q", "HEAD"]).is_ok()
    }

    /// Tracked files at HEAD, sorted.
    pub fn tree(&self) -> Result<Vec<String>, GitError> {
        if !self.has_commits() {
            return Ok(vec![]);
        }
        let out = self.run(&["ls-tree", "-r", "-z", "--name-only", "HEAD"])?;
        let mut v: Vec<String> = text(out).split('\0').filter(|s| !s.is_empty()).map(String::from).collect();
        v.sort();
        Ok(v)
    }

    /// Commits reachable from HEAD, newest first, `count` of them after skipping `skip`; for one file when `path` is given
    /// (following renames when `follow`).
    pub fn log(&self, path: Option<&str>, follow: bool, skip: usize, count: usize) -> Result<Vec<Commit>, GitError> {
        if !self.has_commits() {
            return Ok(vec![]);
        }
        let (skip, count) = (format!("--skip={skip}"), format!("--max-count={count}"));
        let format = format!("--format={LOG_FORMAT}");
        let mut args = vec!["log", "--date=format:%Y-%m-%d %H:%M", &format, &skip, &count];
        if follow && path.is_some() {
            args.push("--follow");
        }
        if let Some(p) = path {
            args.push("--");
            args.push(p);
        }
        Ok(parse_log(&text(self.run(&args)?)))
    }

    /// The newest `count` commits that touched `path` (renames followed), each with the change made to the file; reads the same stream as
    /// the screen does and stops git once it has enough.
    pub fn file_history(&self, path: &str, count: usize) -> Result<Vec<(Commit, Option<Change>)>, GitError> {
        if !self.has_commits() {
            return Ok(vec![]);
        }
        let log = self.spawn_file_log(path)?;
        let mut out = vec![];
        while let Ok(batch) = log.rx.recv() {
            match batch {
                LogBatch::Items(v) => {
                    out.extend(v);
                    if out.len() >= count {
                        out.truncate(count);
                        break;
                    }
                }
                LogBatch::Done(r) => {
                    r.map_err(GitError::Failed)?;
                    break;
                }
            }
        }
        Ok(out)
    }

    /// The hash of git's empty tree (differs between SHA-1 and SHA-256 repositories).
    fn empty_tree(&self) -> String {
        self.run(&["hash-object", "-t", "tree", "/dev/null"])
            .map(|b| text(b).trim().to_string())
            .unwrap_or_else(|_| "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into())
    }

    /// The commit a diff of `c` is taken against: its first parent (also for merges), or the empty tree for a root commit.
    fn base_of(&self, c: &Commit) -> String {
        c.parents.first().cloned().unwrap_or_else(|| self.empty_tree())
    }

    pub fn commit_message(&self, c: &Commit) -> Result<String, GitError> {
        Ok(text(self.run(&["show", "-s", "--format=%B", &c.hash])?).trim_end().to_string())
    }

    /// The files a commit changed (against its first parent), renames detected.
    pub fn changes(&self, c: &Commit) -> Result<Vec<Change>, GitError> {
        let base = self.base_of(c);
        Ok(parse_name_status(&text(self.run(&["diff", "--no-ext-diff", "--no-textconv", "-M", "--name-status", "-z", &base, &c.hash])?)))
    }

    /// The unified diff of a commit, or of one file in it (give both paths of a rename).
    pub fn diff(&self, c: &Commit, paths: &[&str]) -> Result<String, GitError> {
        let base = self.base_of(c);
        let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color", "-M", &base, &c.hash];
        if !paths.is_empty() {
            args.push("--");
            args.extend(paths);
        }
        Ok(text(self.run(&args)?))
    }

    /// A file as it was at a commit.
    pub fn file_at(&self, c: &Commit, path: &str) -> Result<Vec<u8>, GitError> {
        self.run(&["show", &format!("{}:{path}", c.hash)])
    }

    /// A path typed relative to `cwd` (or absolute), as a path relative to the repository root. The file need not exist (it may have been
    /// deleted); `..` is resolved lexically, and a path that leaves the repository is an error.
    pub fn repo_path(&self, cwd: &Path, file: &str) -> Result<String, GitError> {
        let abs = if Path::new(file).is_absolute() { PathBuf::from(file) } else { cwd.join(file) };
        let mut parts: Vec<String> = Vec::new();
        for c in abs.components() {
            use std::path::Component::*;
            match c {
                Normal(p) => parts.push(p.to_string_lossy().into_owned()),
                ParentDir => {
                    parts.pop();
                }
                _ => parts.clear(),
            }
        }
        let top = self.top.canonicalize().unwrap_or_else(|_| self.top.clone());
        let top_parts: Vec<String> = top
            .components()
            .filter_map(|c| if let std::path::Component::Normal(p) = c { Some(p.to_string_lossy().into_owned()) } else { None })
            .collect();
        // the directory may be reached through a symbolic link: compare with the real path of the part that exists
        let real_parent = abs.parent().and_then(|p| p.canonicalize().ok());
        let parts = match real_parent {
            Some(rp) => {
                let mut v: Vec<String> = rp
                    .components()
                    .filter_map(|c| if let std::path::Component::Normal(p) = c { Some(p.to_string_lossy().into_owned()) } else { None })
                    .collect();
                if let Some(name) = abs.file_name().filter(|n| *n != ".." && *n != ".") {
                    v.push(name.to_string_lossy().into_owned());
                }
                v
            }
            None => parts,
        };
        if parts.len() > top_parts.len() && parts[..top_parts.len()] == top_parts[..] {
            Ok(parts[top_parts.len()..].join("/"))
        } else {
            Err(GitError::Failed(format!("{file} is outside the repository")))
        }
    }

    /// Does `path` (relative to the repository root, or to `cwd` when given relative) have any history?
    pub fn has_history(&self, path: &str) -> bool {
        self.log(Some(path), false, 0, 1).map(|l| !l.is_empty()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_records_are_parsed() {
        let out = "\u{1e}aaa\u{1f}aa\u{1f}Ann\u{1f}a@x\u{1f}2026-01-02 03:04\u{1f}fix: a thing\u{1f}bbb ccc\n\u{1e}ddd\u{1f}dd\u{1f}Bob\u{1f}b@x\u{1f}2026-01-01 00:00\u{1f}root\u{1f}\n";
        let c = parse_log(out);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].hash.as_str(), c[0].author.as_str(), c[0].subject.as_str()), ("aaa", "Ann", "fix: a thing"));
        assert_eq!(c[0].parents, vec!["bbb", "ccc"]);
        assert!(c[1].parents.is_empty(), "a root commit has no parents");
        assert!(parse_log("").is_empty());
        assert!(parse_log("garbage without separators").is_empty());
    }

    #[test]
    fn name_status_handles_renames_copies_and_odd_names() {
        let c = parse_name_status("M\0a.txt\0A\0new file.txt\0D\0gone\0R095\0old.txt\0new.txt\0C100\0s\0d\0T\0link\0");
        assert_eq!(c.len(), 6);
        assert_eq!(c[0], Change { status: 'M', path: "a.txt".into(), old_path: None, score: None });
        assert_eq!(c[1].path, "new file.txt");
        assert_eq!((c[3].status, c[3].old_path.as_deref(), c[3].path.as_str(), c[3].score), ('R', Some("old.txt"), "new.txt", Some(95)));
        assert_eq!(c[3].label(), "R old.txt → new.txt");
        assert_eq!(c[4].status, 'C');
        assert!(parse_name_status("").is_empty());
        assert!(parse_name_status("R090\0only-one\0").is_empty(), "a truncated rename is dropped, not mis-parsed");
    }

    #[test]
    fn file_log_output_pairs_each_commit_with_its_change() {
        let rec = |h: &str| format!("\u{1e}{h}\u{1f}{h}\u{1f}an\u{1f}e@x\u{1f}2026-01-02 03:04\u{1f}subject {h}\u{1f}p1 p2");
        let out = format!("{}\0\nR090\0a.txt\0b.txt\0{}\0\nA\0a.txt\0", rec("h2"), rec("h1"));
        let f = parse_file_log(&out);
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].0.hash.as_str(), f[0].0.subject.as_str(), f[0].0.parents.len()), ("h2", "subject h2", 2));
        assert_eq!(f[0].1.as_ref().unwrap().old_path.as_deref(), Some("a.txt"));
        assert_eq!(f[1].1.as_ref().unwrap().status, 'A');
    }

    #[test]
    fn a_log_cut_at_any_byte_gives_the_same_commits_as_the_whole_log() {
        let rec = |h: &str, extra: &str| {
            format!("\u{1e}{h}\u{1f}{h}\u{1f}an é\u{1f}e@x\u{1f}2026-01-02 03:04\u{1f}subject {h} ✓\u{1f}p1\0\n{extra}")
        };
        let whole = format!("{}{}{}", rec("h3", "M\0a.txt\0"), rec("h2", "R090\0a.txt\0b.txt\0"), rec("h1", "A\0a.txt\0"));
        let expected = parse_file_log(&whole);
        assert_eq!(expected.len(), 3);
        let bytes = whole.as_bytes();
        for cut in 0..=bytes.len() {
            let mut st = RecordStream::default();
            let mut got = st.push(&bytes[..cut]);
            got.extend(st.push(&bytes[cut..]));
            got.extend(st.finish());
            assert_eq!(got, expected, "cut at byte {cut}");
        }
        // one byte at a time, as a slow pipe could deliver it
        let mut st = RecordStream::default();
        let mut got = vec![];
        for b in bytes {
            got.extend(st.push(&[*b]));
        }
        got.extend(st.finish());
        assert_eq!(got, expected);
    }

    #[test]
    fn a_record_is_only_released_when_the_next_one_has_started() {
        let mut st = RecordStream::default();
        let first = "\u{1e}h1\u{1f}h1\u{1f}a\u{1f}e\u{1f}d\u{1f}s\u{1f}\0\nA\0f\0";
        assert!(st.push(first.as_bytes()).is_empty(), "it might still grow");
        let out = st.push("\u{1e}h2".as_bytes());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.hash, "h1");
    }

    #[test]
    fn head_labels() {
        assert_eq!(Head::Branch("main".into()).label(), "on main");
        assert_eq!(Head::Detached("abc1234".into()).label(), "HEAD detached at abc1234");
        assert!(Head::Unborn("main".into()).label().contains("no commits"));
    }
}
