//! `kuro git`: a browser for a repository's history and its working-tree changes. See docs/GIT.md.
//!
//! One full-screen program with five views: the file **tree**, a **history** of commits (of the repository, or of one file with renames
//! followed), one **commit** with its changed files, a **diff**, and the **changes** of the working tree (what VS Code's Source Control shows).
//! A file at a commit opens in the normal editor, read-only. All git calls are in [`cmd`]; the only ones that change anything are the
//! discards of the changes view, which always ask first.

pub mod cmd;
mod inline;
mod ui;

use crate::clipboard::Clipboard;
use crate::config::Config;
use crate::scrollbar::{self, Part};
use cmd::{Change, Commit, Git, GitError, Head, WtChange, WtKind};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};

/// The keys of `kuro git` as `(view, keys, what they do)`, checked against docs/GIT.md and exercised by tests/git_ui.rs.
pub const KEYS: &[(&str, &str, &str)] = &[
    ("Everywhere", "q Ctrl+Q", "Quit"),
    ("Everywhere", "Esc Backspace", "Go back one view"),
    ("Lists", "↑ ↓ k j", "Move the selection"),
    ("Lists", "PageUp PageDown", "Move a page"),
    ("Lists", "b Space Ctrl+B Ctrl+F", "Move a page up / down (for terminals that keep PageUp / PageDown for themselves)"),
    ("Lists", "Ctrl+U Ctrl+D", "Move half a page up / down"),
    ("Lists", "Home End g G", "First / last row"),
    ("Tree", "Enter →", "Open a folder (or fold it); on a file: show that file's history"),
    ("Tree", "a Tab", "History of the whole repository"),
    ("Tree", "c", "The changes of the working tree (also from the history)"),
    ("Changes", "Enter → d", "Diff of the selected file against HEAD"),
    ("Changes", "x Delete", "Discard the changes of the selected file (asks first)"),
    ("Changes", "X", "Discard all changes (asks first; untracked files get a question of their own)"),
    ("Changes", "r", "Look again (the list also updates by itself)"),
    ("Diff of a change", "x", "Discard the changes of this file (asks first)"),
    ("Diff of a change", "n p", "Next / previous changed file"),
    ("History", "Enter →", "Open the selected commit"),
    ("History", "d", "Show its diff (of the followed file in a file history)"),
    ("History", "Tab t", "Back to the tree"),
    ("Commit", "Enter →", "Diff of the selected file in this commit"),
    ("Commit", "d", "The whole diff of the commit"),
    ("Commit", "o", "Open the selected file as it was at this commit (read-only)"),
    ("Commit", "n p", "Next older / newer commit"),
    ("Diff", "↑ ↓ PageUp PageDown Home End", "Scroll"),
    ("Diff", "b Space Ctrl+B Ctrl+F Ctrl+U Ctrl+D", "Scroll a page or half a page, like less"),
    ("Diff", "← → h l", "Scroll sideways"),
    ("Diff", "n p", "Next older / newer commit, same file if it is there"),
    ("Diff", "o", "Open the file at this commit (read-only)"),
    ("File view", "q", "Close the file view (Ctrl+Q and :q too)"),
    ("Mouse", "Click a row / double-click", "Select / open"),
    ("Mouse", "Wheel", "Scroll"),
    ("Mouse", "Click or drag a scrollbar", "Step, page, scroll"),
];

/// Makes the clipboard of the read-only file view (tests replace it with an in-memory one).
pub type ClipboardFactory = Box<dyn Fn(&Config) -> Box<dyn Clipboard>>;

pub const PAGE: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Tree,
    History,
    Commit,
    Diff,
    /// The changes of the working tree (what `git status` shows), like VS Code's Source Control.
    Changes,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Scope {
    Repo,
    /// The history of one file (path relative to the repository root), renames followed.
    File(String),
}

#[derive(Clone, Debug)]
pub struct TreeRow {
    pub depth: usize,
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub open: bool,
}

#[derive(Clone, Debug)]
pub struct Detail {
    pub commit: Commit,
    pub message: String,
    pub changes: Vec<Change>,
}

#[derive(Clone, Copy)]
struct BarDrag {
    vertical: bool,
    grab: usize,
    origin: isize,
    offset: usize,
}

/// Where a list or the diff is scrolled to.
#[derive(Clone, Copy, Default, Debug)]
pub struct Scroll {
    pub sel: usize,
    pub top: usize,
}

/// The entries of `list` that are still changed (the working tree is looked at again before anything is thrown away).
fn still(now: &[WtChange], list: &[WtChange]) -> Vec<WtChange> {
    list.iter().filter(|w| now.contains(w)).cloned().collect()
}

/// A question before changes are thrown away.
#[derive(Clone, Debug)]
pub struct Confirm {
    /// Tracked changes to restore or remove (asked at stage 1).
    pub tracked: Vec<WtChange>,
    /// Untracked files to delete (asked at stage 2, a question of their own).
    pub untracked: Vec<WtChange>,
    pub stage: u8,
    /// Asked for everything (`X`): after the tracked files the untracked ones are asked about.
    pub all: bool,
}

pub struct GitApp {
    pub git: Git,
    pub cfg: Config,
    pub head: Head,
    /// Shown in the header (the folder name of the repository).
    pub repo_name: String,
    pub view: View,
    pub scope: Scope,
    back: Vec<View>,
    files: Vec<String>,
    expanded: HashSet<String>,
    pub rows: Vec<TreeRow>,
    pub tree: Scroll,
    pub commits: Vec<Commit>,
    pub all_loaded: bool,
    /// In a file history: the change each commit made to the followed file (so renames show the path it had).
    pub followed: HashMap<String, Change>,
    pub history: Scroll,
    pub detail: Option<Detail>,
    pub files_scroll: Scroll,
    pub diff: Vec<String>,
    /// For each diff line the cells where the change inside the line is (see `inline`), drawn in reverse video.
    pub diff_marks: Vec<Vec<(usize, usize)>>,
    pub diff_title: String,
    /// The file of the diff, as (old path, new path), or `None` for the whole commit.
    pub diff_file: Option<(Option<String>, String)>,
    pub diff_top: usize,
    pub diff_left: usize,
    pub viewer: Option<Box<crate::app::App>>,
    pub msg: String,
    pub quit: bool,
    pub area: Rect,
    drag: Option<BarDrag>,
    last_click: Option<(View, usize)>,
    pub make_clipboard: ClipboardFactory,
    /// Extension languages and terminal colours, handed to the read-only file view.
    pub ext_langs: Vec<&'static crate::syntax::LangSpec>,
    pub palette: Option<crate::palette::Palette>,
    /// The history of a file that git is still reading in the background (it can take many seconds on a large repository):
    /// commits are added to the list as they arrive. Dropping it stops git.
    loader: Option<cmd::FileLog>,
    /// The working tree's changes (see `refresh_changes`), when they were last looked at, and how long that took.
    pub changes: Vec<WtChange>,
    pub changes_scroll: Scroll,
    status_at: Option<std::time::Instant>,
    status_cost: std::time::Duration,
    /// The diff on screen is the working-tree diff of this path.
    pub wt_diff: Option<String>,
    pub confirm: Option<Confirm>,
    /// How often the changes view looks at the working tree again (the tree view: three times as seldom).
    pub poll_interval: std::time::Duration,
}

/// `$XDG_CACHE_HOME/kuro/git` (`~/.cache/kuro/git`).
fn cache_root() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("kuro/git"))
}

fn text_lines(s: &str) -> Vec<String> {
    s.lines().map(|l| l.replace('\t', "    ").replace('\r', "")).collect()
}

impl GitApp {
    /// Open the repository containing the current directory (`file`: start in that file's history).
    pub fn new(mut git: Git, cfg: Config, file: Option<String>) -> Result<GitApp, GitError> {
        if cfg.git.graph_cache {
            git.cache_root = cache_root();
            git.cache_min_commits = cfg.git.graph_cache_min_commits;
        }
        let head = git.head()?;
        let files = git.tree()?;
        let make_clipboard: ClipboardFactory = Box::new(|c: &Config| crate::clipboard::detect(&c.clipboard.backend));
        let repo_name = git.top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
        let mut app = GitApp {
            git,
            cfg,
            repo_name,
            head,
            view: View::Tree,
            scope: Scope::Repo,
            back: vec![],
            files,
            expanded: HashSet::new(),
            rows: vec![],
            tree: Scroll::default(),
            commits: vec![],
            all_loaded: false,
            followed: HashMap::new(),
            history: Scroll::default(),
            detail: None,
            files_scroll: Scroll::default(),
            diff: vec![],
            diff_marks: vec![],
            diff_title: String::new(),
            diff_file: None,
            diff_top: 0,
            diff_left: 0,
            viewer: None,
            msg: String::new(),
            quit: false,
            area: Rect::new(0, 0, 80, 24),
            drag: None,
            last_click: None,
            make_clipboard,
            ext_langs: vec![],
            palette: None,
            loader: None,
            changes: vec![],
            changes_scroll: Scroll::default(),
            status_at: None,
            status_cost: std::time::Duration::ZERO,
            wt_diff: None,
            confirm: None,
            poll_interval: std::time::Duration::from_millis(1500),
        };
        app.rebuild_tree();
        let _ = app.refresh_changes();
        match file {
            Some(f) => {
                // a file that is not in the current tree may have been deleted: finding out takes a walk of the history, so it is checked
                // first (a file that exists starts at once)
                if !app.files.contains(&f) && !app.git.has_history(&f) {
                    return Err(GitError::Failed(format!("no history for {f}")));
                }
                app.show_file_history(&f);
            }
            None => {
                app.load_history(Scope::Repo)?;
                app.view = if app.git.has_commits() { View::Tree } else { View::History };
            }
        }
        Ok(app)
    }

    // ---------------------------------------------------------------------------- tree

    fn rebuild_tree(&mut self) {
        let mut rows = Vec::new();
        let mut seen_dirs: HashSet<String> = HashSet::new();
        for f in &self.files {
            let parts: Vec<&str> = f.split('/').collect();
            // every folder on the way, once, and only when all its parents are open
            let mut visible = true;
            let mut prefix = String::new();
            for (i, part) in parts[..parts.len() - 1].iter().enumerate() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                if visible && seen_dirs.insert(prefix.clone()) {
                    rows.push(TreeRow {
                        depth: i,
                        name: part.to_string(),
                        path: prefix.clone(),
                        dir: true,
                        open: self.expanded.contains(&prefix),
                    });
                }
                if !self.expanded.contains(&prefix) {
                    visible = false;
                }
            }
            if visible {
                rows.push(TreeRow {
                    depth: parts.len() - 1,
                    name: parts[parts.len() - 1].to_string(),
                    path: f.clone(),
                    dir: false,
                    open: false,
                });
            }
        }
        self.rows = rows;
        self.tree.sel = self.tree.sel.min(self.rows.len().saturating_sub(1));
    }

    // ---------------------------------------------------------------------------- history

    fn load_history(&mut self, scope: Scope) -> Result<(), GitError> {
        self.scope = scope;
        self.commits.clear();
        self.followed.clear();
        self.all_loaded = false;
        self.history = Scroll::default();
        self.load_more()
    }

    /// Load the next page of commits (the history of the repository; a file's history arrives on its own, see `tick`).
    pub fn load_more(&mut self) -> Result<(), GitError> {
        if self.all_loaded || matches!(self.scope, Scope::File(_)) {
            return Ok(());
        }
        let page = self.git.log(None, false, self.commits.len(), PAGE)?;
        if page.len() < PAGE {
            self.all_loaded = true;
        }
        self.commits.extend(page);
        Ok(())
    }

    /// Show the history of `path` at once and let git fill the list in the background.
    pub fn show_file_history(&mut self, path: &str) {
        self.scope = Scope::File(path.to_string());
        self.commits.clear();
        self.followed.clear();
        self.all_loaded = false;
        self.history = Scroll::default();
        self.view = View::History;
        self.back = vec![View::Tree];
        self.loader = None;
        // a big repository gets a commit graph in kuro's cache, for the next time (a no-op when it is there, or not needed)
        self.git.ensure_graph_in_background();
        match self.git.spawn_file_log(path) {
            Ok(l) => self.loader = Some(l),
            Err(e) => {
                self.all_loaded = true;
                self.msg = e.to_string();
            }
        }
    }

    /// Is git still reading a file's history?
    pub fn loading(&self) -> bool {
        self.loader.is_some()
    }

    /// Take what the background reading has found so far; returns true when something on screen changed.
    pub fn tick(&mut self) -> bool {
        // leaving the history for the tree stops the reading
        if self.view == View::Tree && self.loader.is_some() {
            self.loader = None;
        }
        let mut changed = self.poll_changes();
        let mut finished: Option<Result<(), String>> = None;
        if let Some(l) = &self.loader {
            while let Ok(batch) = l.rx.try_recv() {
                changed = true;
                match batch {
                    cmd::LogBatch::Items(items) => {
                        for (commit, change) in items {
                            if let Some(c) = change {
                                self.followed.insert(commit.hash.clone(), c);
                            }
                            self.commits.push(commit);
                        }
                    }
                    cmd::LogBatch::Done(r) => {
                        finished = Some(r);
                        break;
                    }
                }
            }
        }
        if let Some(r) = finished {
            self.loader = None;
            self.all_loaded = true;
            match r {
                Ok(()) if self.commits.is_empty() => {
                    let name = match &self.scope {
                        Scope::File(f) => f.clone(),
                        Scope::Repo => String::new(),
                    };
                    self.msg = format!("no history for {name}");
                    let _ = self.load_history(Scope::Repo);
                    self.view = View::Tree;
                }
                Ok(()) => {}
                Err(e) => self.msg = e,
            }
        }
        changed
    }

    /// Wait until the background reading is finished (tests, and anything that needs the whole list).
    pub fn wait_loaded(&mut self) {
        let start = std::time::Instant::now();
        while self.loader.is_some() && start.elapsed() < std::time::Duration::from_secs(60) {
            self.tick();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn show_repo_history(&mut self) {
        self.loader = None;
        if let Err(e) = self.load_history(Scope::Repo) {
            self.msg = e.to_string();
        }
        self.view = View::History;
        self.back = vec![View::Tree];
    }

    // ---------------------------------------------------------------------------- working-tree changes

    /// Look at the working tree again. Returns true when the list is different from before. The selection stays on the same path.
    pub fn refresh_changes(&mut self) -> Result<bool, GitError> {
        let start = std::time::Instant::now();
        let now = self.git.status()?;
        self.status_cost = start.elapsed();
        self.status_at = Some(std::time::Instant::now());
        if now == self.changes {
            return Ok(false);
        }
        let keep = self.changes.get(self.changes_scroll.sel).map(|c| c.path.clone());
        self.changes = now;
        let sel = keep.and_then(|p| self.changes.iter().position(|c| c.path == p)).unwrap_or(self.changes_scroll.sel);
        self.changes_scroll.sel = sel.min(self.changes.len().saturating_sub(1));
        let (len, h) = (self.changes.len(), self.list_height());
        self.changes_scroll.top = self.changes_scroll.top.min(len.saturating_sub(h));
        Ok(true)
    }

    /// Look again every now and then while the changes (or the tree with its count) are on screen: VS Code watches the files, kuro asks
    /// git, at a pace that stays well under a tenth of the time even when `git status` is slow in a huge repository.
    fn poll_changes(&mut self) -> bool {
        if self.confirm.is_some() || self.viewer.is_some() {
            return false;
        }
        let wanted = match self.view {
            View::Changes => self.poll_interval,
            View::Tree => self.poll_interval * 3,
            View::Diff if self.wt_diff.is_some() => self.poll_interval,
            _ => return false,
        };
        let wait = wanted.max(self.status_cost * 10);
        if self.status_at.is_some_and(|t| t.elapsed() < wait) {
            return false;
        }
        let changed = self.refresh_changes().unwrap_or(false);
        if self.view == View::Diff && self.wt_diff.is_some() {
            // the status letters may be the same while the file changed again: compare the diff itself
            let before = self.diff.clone();
            self.reload_wt_diff();
            return changed || self.diff != before || self.view != View::Diff;
        }
        changed
    }

    fn show_changes(&mut self) {
        if let Err(e) = self.refresh_changes() {
            return self.msg = e.to_string();
        }
        if self.view != View::Changes {
            self.back.push(self.view);
        }
        self.view = View::Changes;
        self.msg.clear();
    }

    /// The diff of working-tree change number `i` against HEAD.
    fn show_wt_diff(&mut self, i: usize) {
        let Some(c) = self.changes.get(i).cloned() else { return };
        match self.git.worktree_diff(&c) {
            Ok(text) => {
                self.diff = text_lines(&text);
                if self.diff.is_empty() {
                    self.diff = vec!["(no textual changes)".into()];
                }
                self.diff_marks = inline::marks(&self.diff);
                self.diff_title = format!("{}  (working tree against HEAD)", c.path);
                self.diff_file = None;
                self.diff_top = 0;
                self.diff_left = 0;
                self.wt_diff = Some(c.path.clone());
                self.changes_scroll.sel = i;
                if self.view != View::Diff {
                    self.back.push(self.view);
                }
                self.view = View::Diff;
            }
            Err(e) => self.msg = e.to_string(),
        }
    }

    /// The change under the diff was updated (the file changed again, or it is no longer changed).
    fn reload_wt_diff(&mut self) {
        let Some(path) = self.wt_diff.clone() else { return };
        match self.changes.iter().position(|c| c.path == path) {
            Some(i) => {
                let (top, left) = (self.diff_top, self.diff_left);
                self.show_wt_diff(i);
                self.diff_top = top.min(self.diff.len().saturating_sub(1));
                self.diff_left = left;
            }
            None => {
                self.msg = format!("{path} is not changed any more");
                self.wt_diff = None;
                self.view = View::Changes;
                self.back.clear();
                self.back.push(View::Tree);
            }
        }
    }

    /// Next (`+1`) or previous (`-1`) changed file, staying in the diff.
    fn step_change(&mut self, delta: isize) {
        let next = self.changes_scroll.sel as isize + delta;
        if next < 0 || next as usize >= self.changes.len() {
            return self.msg = if delta > 0 { "this is the last changed file".into() } else { "this is the first changed file".into() };
        }
        self.show_wt_diff(next as usize);
    }

    /// Ask before discarding the changes of one file.
    fn ask_discard_one(&mut self, i: usize) {
        let Some(c) = self.changes.get(i).cloned() else { return };
        match c.kind() {
            WtKind::Conflict => self.msg = format!("{} is a merge conflict: resolve it with git, kuro leaves it alone", c.path),
            WtKind::Untracked => self.confirm = Some(Confirm { tracked: vec![], untracked: vec![c], stage: 2, all: false }),
            _ => self.confirm = Some(Confirm { tracked: vec![c], untracked: vec![], stage: 1, all: false }),
        }
    }

    /// Ask before discarding everything.
    fn ask_discard_all(&mut self) {
        let tracked: Vec<WtChange> =
            self.changes.iter().filter(|c| matches!(c.kind(), WtKind::Modified | WtKind::Deleted | WtKind::New)).cloned().collect();
        let untracked: Vec<WtChange> = self.changes.iter().filter(|c| c.kind() == WtKind::Untracked).cloned().collect();
        if tracked.is_empty() && untracked.is_empty() {
            return self.msg = "nothing to discard".into();
        }
        let stage = if tracked.is_empty() { 2 } else { 1 };
        self.confirm = Some(Confirm { tracked, untracked, stage, all: true });
    }

    fn confirm_key(&mut self, k: KeyEvent) {
        let Some(c) = self.confirm.clone() else { return };
        let yes = matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'));
        let no = matches!(k.code, KeyCode::Char('n') | KeyCode::Char('N'));
        let esc = matches!(k.code, KeyCode::Esc);
        if !(yes || no || esc) {
            return;
        }
        // everything is looked at again now: only what is still changed, and was shown in the question, is discarded
        let _ = self.refresh_changes();
        match (c.stage, yes) {
            (1, true) => {
                let done = self.git.discard(&still(&self.changes, &c.tracked));
                let _ = self.refresh_changes();
                match done {
                    Err(e) => {
                        self.confirm = None;
                        return self.msg = format!("discard failed: {e}");
                    }
                    Ok(n) => self.msg = format!("discarded the changes of {n} file{}", if n == 1 { "" } else { "s" }),
                }
                if c.all && !c.untracked.is_empty() {
                    self.confirm = Some(Confirm { tracked: vec![], untracked: still(&self.changes, &c.untracked), stage: 2, all: true });
                    if self.confirm.as_ref().is_some_and(|x| x.untracked.is_empty()) {
                        self.confirm = None;
                    }
                } else {
                    self.confirm = None;
                }
            }
            (2, true) => {
                let list = still(&self.changes, &c.untracked);
                self.confirm = None;
                match self.git.discard(&list) {
                    Ok(n) => self.msg = format!("deleted {n} untracked file{}", if n == 1 { "" } else { "s" }),
                    Err(e) => self.msg = format!("delete failed: {e}"),
                }
                let _ = self.refresh_changes();
            }
            (2, false) if no && c.all => {
                self.confirm = None;
                self.msg = "untracked files kept".into();
            }
            _ => {
                self.confirm = None;
                self.msg = "cancelled: nothing was changed".into();
            }
        }
        // the diff or the list that was open may be about a file that is gone now
        if self.confirm.is_none() && self.view == View::Diff && self.wt_diff.is_some() {
            self.reload_wt_diff();
        }
        self.changes_scroll.sel = self.changes_scroll.sel.min(self.changes.len().saturating_sub(1));
    }

    // ---------------------------------------------------------------------------- commit and diff

    fn enter_commit(&mut self, idx: usize) {
        let Some(c) = self.commits.get(idx).cloned() else { return };
        let message = self.git.commit_message(&c).unwrap_or_default();
        let changes = match self.git.changes(&c) {
            Ok(ch) => ch,
            Err(e) => return self.msg = e.to_string(),
        };
        // in a file history, start on the followed file
        let mut sel = 0;
        if let Some(fc) = self.followed.get(&c.hash)
            && let Some(i) = changes.iter().position(|ch| ch.path == fc.path)
        {
            sel = i;
        }
        self.history.sel = idx;
        self.detail = Some(Detail { commit: c, message, changes });
        self.files_scroll = Scroll { sel, top: 0 };
        if self.view != View::Diff {
            self.back.push(self.view);
        }
        self.view = View::Commit;
    }

    fn show_diff(&mut self, whole: bool) {
        let Some(d) = &self.detail else { return };
        let commit = d.commit.clone();
        let file = if whole { None } else { d.changes.get(self.files_scroll.sel).map(|c| (c.old_path.clone(), c.path.clone())) };
        let paths: Vec<&str> = match &file {
            Some((Some(old), new)) => vec![old.as_str(), new.as_str()],
            Some((None, new)) => vec![new.as_str()],
            None => vec![],
        };
        match self.git.diff(&commit, &paths) {
            Ok(text) => {
                self.diff = text_lines(&text);
                if self.diff.is_empty() {
                    self.diff = vec!["(no textual changes)".into()];
                }
                self.diff_marks = inline::marks(&self.diff);
                self.diff_title = match &file {
                    Some((_, p)) => format!("{} {}  {}", commit.short, p, commit.subject),
                    None => format!("{}  {}", commit.short, commit.subject),
                };
                self.diff_file = file;
                self.wt_diff = None;
                self.diff_top = 0;
                self.diff_left = 0;
                if self.view != View::Diff {
                    self.back.push(self.view);
                }
                self.view = View::Diff;
            }
            Err(e) => self.msg = e.to_string(),
        }
    }

    /// Move to the next older (`+1`) or newer (`-1`) commit, staying in the diff when that is where we are.
    fn step_commit(&mut self, delta: isize) {
        let Some(d) = &self.detail else { return };
        let Some(cur) = self.commits.iter().position(|c| c.hash == d.commit.hash) else { return };
        let next = cur as isize + delta;
        if next < 0 {
            return self.msg = "this is the newest commit".into();
        }
        if next as usize >= self.commits.len() {
            if !self.all_loaded {
                let _ = self.load_more();
            }
            if next as usize >= self.commits.len() {
                return self.msg = "this is the oldest commit".into();
            }
        }
        let was_diff = self.view == View::Diff;
        let want = self.diff_file.as_ref().map(|(_, p)| p.clone());
        let saved_back = self.back.clone();
        self.view = View::Commit;
        self.enter_commit(next as usize);
        self.back = saved_back;
        if was_diff {
            // the same file if this commit touches it (by its new or old name), else the whole diff
            let idx = want
                .as_ref()
                .and_then(|w| self.detail.as_ref().unwrap().changes.iter().position(|c| &c.path == w || c.old_path.as_ref() == Some(w)));
            match idx {
                Some(i) => {
                    self.files_scroll.sel = i;
                    self.show_diff(false);
                }
                None => self.show_diff(true),
            }
            self.view = View::Diff;
        }
    }

    /// Open the file of the selected change (or of the diff) as it was at the commit, read-only, in the editor.
    fn open_file(&mut self) {
        let Some(d) = &self.detail else { return };
        let change = match (self.view, &self.diff_file) {
            (View::Diff, Some((old, new))) => d.changes.iter().find(|c| &c.path == new && &c.old_path == old).cloned(),
            _ => d.changes.get(self.files_scroll.sel).cloned(),
        };
        let Some(change) = change else { return self.msg = "pick a file first".into() };
        // a deleted file only exists in the parent
        let (at, path) = if change.status == 'D' {
            match d.commit.parents.first() {
                Some(p) => (Commit { hash: p.clone(), short: p.chars().take(7).collect(), ..d.commit.clone() }, change.path.clone()),
                None => return self.msg = "the file does not exist in this commit".into(),
            }
        } else {
            (d.commit.clone(), change.path.clone())
        };
        let bytes = match self.git.file_at(&at, &path) {
            Ok(b) => b,
            Err(e) => return self.msg = e.to_string(),
        };
        if bytes.contains(&0) {
            return self.msg = format!("{path} is a binary file");
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut cfg = self.cfg.clone();
        cfg.sync.enabled = false;
        let buffer = crate::buffer::Buffer::from_str(&text);
        let mut app = crate::app::App::new(cfg, buffer, (self.make_clipboard)(&self.cfg));
        app.set_ext_langs(self.ext_langs.clone());
        if let Some(p) = &self.palette {
            app.set_palette(p.clone());
        }
        app.buffer.path = Some(std::path::PathBuf::from(&path));
        app.view_only = Some(format!("read-only: {path} at {}", at.short));
        app.update_lang();
        app.flash(format!("{path} at {}  (read-only; q closes)", at.short));
        self.viewer = Some(Box::new(app));
    }

    fn go_back(&mut self) {
        self.msg.clear();
        match self.back.pop() {
            Some(v) => self.view = v,
            None => {
                if self.view != View::Tree && self.git.has_commits() {
                    self.view = View::Tree;
                }
            }
        }
    }

    // ---------------------------------------------------------------------------- navigation helpers

    fn list_len(&self) -> usize {
        match self.view {
            View::Tree => self.rows.len(),
            View::History => self.commits.len(),
            View::Commit => self.detail.as_ref().map_or(0, |d| d.changes.len()),
            View::Diff => self.diff.len(),
            View::Changes => self.changes.len(),
        }
    }

    pub fn list_height(&self) -> usize {
        (ui::layout(self, self.area).list.height as usize).max(1)
    }

    fn scroll_of(&mut self) -> &mut Scroll {
        match self.view {
            View::Tree => &mut self.tree,
            View::History => &mut self.history,
            View::Changes => &mut self.changes_scroll,
            _ => &mut self.files_scroll,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let (len, h) = (self.list_len(), self.list_height());
        if len == 0 {
            return;
        }
        let s = self.scroll_of();
        s.sel = (s.sel as isize + delta).clamp(0, len as isize - 1) as usize;
        if s.sel < s.top {
            s.top = s.sel;
        } else if s.sel >= s.top + h {
            s.top = s.sel + 1 - h;
        }
        if self.view == View::History && self.history.sel + 20 >= self.commits.len() && !self.all_loaded {
            let _ = self.load_more();
        }
    }

    fn select_row(&mut self, row: usize) {
        let len = self.list_len();
        if len == 0 {
            return;
        }
        let r = row.min(len - 1);
        let (h, view) = (self.list_height(), self.view);
        let s = self.scroll_of();
        s.sel = r;
        if s.sel < s.top {
            s.top = s.sel;
        } else if s.sel >= s.top + h {
            s.top = s.sel + 1 - h;
        }
        let _ = view;
    }

    fn open_selected(&mut self) {
        self.msg.clear();
        match self.view {
            View::Tree => {
                let Some(row) = self.rows.get(self.tree.sel).cloned() else { return };
                if row.dir {
                    if !self.expanded.remove(&row.path) {
                        self.expanded.insert(row.path.clone());
                    }
                    self.rebuild_tree();
                } else {
                    self.show_file_history(&row.path);
                }
            }
            View::History => {
                let i = self.history.sel;
                self.enter_commit(i)
            }
            View::Commit => self.show_diff(false),
            View::Diff => {}
            View::Changes => self.show_wt_diff(self.changes_scroll.sel),
        }
    }

    // ---------------------------------------------------------------------------- keys

    pub fn handle_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if let Some(v) = &mut self.viewer {
            let close = matches!(k.code, KeyCode::Char('q'))
                && k.modifiers.is_empty()
                && v.mode == crate::app::Mode::Normal
                && v.focus == crate::app::Focus::Editor
                && !v.find.open;
            if close {
                self.viewer = None;
                return;
            }
            v.handle_key(k);
            if v.quit {
                self.viewer = None;
            }
            return;
        }
        if self.confirm.is_some() {
            return self.confirm_key(k);
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.list_height().saturating_sub(1).max(1) as isize;
        self.msg.clear();
        // paging keys, also the `less` ones: some terminals (foot with `scrollback-up-page=Page_Up`, for one) keep PageUp / PageDown
        // for their own scrollback and never pass them on
        let paging = match (k.code, ctrl) {
            (KeyCode::PageDown, _) | (KeyCode::Char(' '), false) | (KeyCode::Char('f'), true) => Some(page),
            (KeyCode::PageUp, _) | (KeyCode::Char('b'), _) => Some(-page),
            (KeyCode::Char('d'), true) => Some((page / 2).max(1)),
            (KeyCode::Char('u'), true) => Some(-(page / 2).max(1)),
            _ => None,
        };
        if let Some(d) = paging {
            if self.view == View::Diff {
                let max_top = self.diff.len().saturating_sub(self.list_height());
                self.diff_top = (self.diff_top as isize + d).clamp(0, max_top as isize) as usize;
            } else {
                self.move_selection(d);
            }
            return;
        }
        match k.code {
            KeyCode::Char('q') if !ctrl => self.quit = true,
            KeyCode::Char('q') | KeyCode::Char('Q') if ctrl => self.quit = true,
            KeyCode::Esc | KeyCode::Backspace => self.go_back(),
            KeyCode::Char('c') if matches!(self.view, View::Tree | View::History) => self.show_changes(),
            KeyCode::Char('r') if self.view == View::Changes => match self.refresh_changes() {
                Ok(_) => self.msg = format!("{} changed file{}", self.changes.len(), if self.changes.len() == 1 { "" } else { "s" }),
                Err(e) => self.msg = e.to_string(),
            },
            KeyCode::Char('x') | KeyCode::Delete if self.view == View::Changes => self.ask_discard_one(self.changes_scroll.sel),
            KeyCode::Char('X') if self.view == View::Changes => self.ask_discard_all(),
            KeyCode::Char('x') | KeyCode::Delete if self.view == View::Diff && self.wt_diff.is_some() => {
                if let Some(i) = self.changes.iter().position(|c| Some(&c.path) == self.wt_diff.as_ref()) {
                    self.ask_discard_one(i);
                }
            }
            KeyCode::Char('n') if self.view == View::Diff && self.wt_diff.is_some() => self.step_change(1),
            KeyCode::Char('p') if self.view == View::Diff && self.wt_diff.is_some() => self.step_change(-1),
            KeyCode::Char('o') if self.view == View::Diff && self.wt_diff.is_some() => {
                self.msg = "open the file in your editor: kuro git shows files as they were at a commit".into()
            }
            KeyCode::Char('d') if self.view == View::Changes => self.show_wt_diff(self.changes_scroll.sel),
            KeyCode::Char('o') if matches!(self.view, View::Commit | View::Diff) => self.open_file(),
            KeyCode::Char('n') if matches!(self.view, View::Commit | View::Diff) => self.step_commit(1),
            KeyCode::Char('p') if matches!(self.view, View::Commit | View::Diff) => self.step_commit(-1),
            _ if self.view == View::Diff => self.diff_key(k),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Home | KeyCode::Char('g') => self.move_selection(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_selection(isize::MAX / 2),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.open_selected(),
            KeyCode::Char('a') | KeyCode::Tab if self.view == View::Tree => self.show_repo_history(),
            KeyCode::Tab | KeyCode::Char('t') if self.view == View::History => {
                self.view = View::Tree;
                self.back.clear();
            }
            KeyCode::Char('d') if self.view == View::History => {
                let i = self.history.sel;
                self.enter_commit(i);
                // a file history shows the followed file's diff, the repository history the whole commit
                if matches!(self.scope, Scope::File(_)) {
                    self.show_diff(false);
                } else {
                    self.show_diff(true);
                }
            }
            KeyCode::Char('d') if self.view == View::Commit => self.show_diff(true),
            _ => {}
        }
    }

    fn diff_key(&mut self, k: KeyEvent) {
        let h = self.list_height();
        let max_top = self.diff.len().saturating_sub(h);
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.diff_top = self.diff_top.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.diff_top = (self.diff_top + 1).min(max_top),
            KeyCode::Home | KeyCode::Char('g') => self.diff_top = 0,
            KeyCode::End | KeyCode::Char('G') => self.diff_top = max_top,
            KeyCode::Left | KeyCode::Char('h') => self.diff_left = self.diff_left.saturating_sub(8),
            KeyCode::Right | KeyCode::Char('l') => self.diff_left = (self.diff_left + 8).min(self.max_left()),
            _ => {}
        }
    }

    pub fn widest_diff_line(&self) -> usize {
        self.diff.iter().map(|l| crate::table::display_width(l)).max().unwrap_or(0)
    }

    fn max_left(&self) -> usize {
        let w = ui::layout(self, self.area).list.width as usize;
        self.widest_diff_line().saturating_sub(w)
    }

    // ---------------------------------------------------------------------------- mouse

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if let Some(v) = &mut self.viewer {
            v.handle_mouse(m);
            return;
        }
        if self.confirm.is_some() {
            return; // the question comes first
        }
        let area = self.area;
        let (x, y) = (m.column, m.row);
        // an ongoing scrollbar drag follows the pointer wherever it goes
        if let Some(d) = self.drag {
            match m.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some((r, bar)) = self.bar_geometry(d.vertical) {
                        let pointer = if d.vertical { y as isize - r.y as isize } else { x as isize - r.x as isize };
                        let off = if pointer == d.origin { d.offset } else { bar.offset_for_pointer(pointer, d.grab) };
                        self.set_offset(d.vertical, off);
                    }
                }
                _ => self.drag = None,
            }
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown => self.wheel(3),
            MouseEventKind::ScrollUp => self.wheel(-3),
            MouseEventKind::ScrollRight if self.view == View::Diff => self.diff_left = (self.diff_left + 6).min(self.max_left()),
            MouseEventKind::ScrollLeft if self.view == View::Diff => self.diff_left = self.diff_left.saturating_sub(6),
            MouseEventKind::Down(MouseButton::Left) => {
                for vertical in [true, false] {
                    if let Some((r, bar)) = self.bar_geometry(vertical)
                        && x >= r.x
                        && x < r.right()
                        && y >= r.y
                        && y < r.bottom()
                    {
                        let pos = if vertical { (y - r.y) as usize } else { (x - r.x) as usize };
                        let page = (if vertical { self.list_height() } else { r.width as usize }).saturating_sub(1).max(1) as isize;
                        match bar.part_at(pos) {
                            Part::BeforeThumb => self.step_offset(vertical, -page),
                            Part::AfterThumb => self.step_offset(vertical, page),
                            Part::Thumb { grab } => {
                                let offset = self.offset(vertical);
                                self.drag = Some(BarDrag { vertical, grab, origin: pos as isize, offset });
                            }
                        }
                        return;
                    }
                }
                // a row
                if let Some(row) = ui::row_at(self, area, x, y) {
                    let (view, already) = (self.view, self.last_click == Some((self.view, row)));
                    self.select_row(row);
                    if already || view == View::Diff {
                        self.last_click = None;
                        self.open_selected();
                    } else {
                        self.last_click = Some((view, row));
                    }
                }
            }
            _ => {}
        }
    }

    fn wheel(&mut self, d: isize) {
        let (len, h) = (self.list_len(), self.list_height());
        if self.view == View::Diff {
            let max = self.diff.len().saturating_sub(h) as isize;
            self.diff_top = (self.diff_top as isize + d).clamp(0, max.max(0)) as usize;
            return;
        }
        let max = len.saturating_sub(h) as isize;
        let s = self.scroll_of();
        s.top = (s.top as isize + d).clamp(0, max.max(0)) as usize;
        if self.view == View::History && self.history.top + h + 20 >= self.commits.len() && !self.all_loaded {
            let _ = self.load_more();
        }
    }

    /// The scrollbar strips of the current view and their geometry.
    pub fn bar_geometry(&self, vertical: bool) -> Option<(Rect, scrollbar::Bar)> {
        let (vbar, hbar) = ui::bars(self, self.area);
        if vertical {
            let r = vbar?;
            Some((r, scrollbar::bar(r.height as usize, self.list_len(), self.list_height(), self.offset(true))?))
        } else {
            let r = hbar?;
            Some((r, scrollbar::bar(r.width as usize, self.widest_diff_line(), r.width as usize, self.diff_left)?))
        }
    }

    fn offset(&self, vertical: bool) -> usize {
        if !vertical {
            return self.diff_left;
        }
        match self.view {
            View::Tree => self.tree.top,
            View::History => self.history.top,
            View::Commit => self.files_scroll.top,
            View::Diff => self.diff_top,
            View::Changes => self.changes_scroll.top,
        }
    }

    fn set_offset(&mut self, vertical: bool, off: usize) {
        if !vertical {
            self.diff_left = off.min(self.max_left());
            return;
        }
        let max = self.list_len().saturating_sub(self.list_height());
        let off = off.min(max);
        match self.view {
            View::Diff => self.diff_top = off,
            _ => self.scroll_of().top = off,
        }
    }

    fn step_offset(&mut self, vertical: bool, d: isize) {
        let cur = self.offset(vertical) as isize;
        self.set_offset(vertical, (cur + d).max(0) as usize);
    }
}

impl crate::screen::Screen for GitApp {
    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        self.area = area;
        if let Some(v) = &mut self.viewer {
            return v.render(buf, area);
        }
        ui::render(self, buf, area);
        None
    }
    fn key(&mut self, k: KeyEvent) {
        self.handle_key(k)
    }
    fn mouse(&mut self, m: MouseEvent) {
        self.handle_mouse(m)
    }
    fn paste(&mut self, _s: &str) {}
    fn tick(&mut self) -> bool {
        GitApp::tick(self)
    }
    fn busy(&self) -> bool {
        false // `tick` reports every batch of commits that arrives, which is what makes the screen update
    }
    fn quit(&self) -> bool {
        self.quit
    }
}
