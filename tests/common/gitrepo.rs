//! A throw-away git repository for tests, isolated from the user's git configuration.
#![allow(dead_code)]

use kuro::gitview::cmd::Git;
use std::path::Path;
use std::process::Command;

pub struct Repo {
    pub dir: tempfile::TempDir,
    pub n: u32,
}

pub fn isolated_env(home: &Path) -> Vec<(String, String)> {
    vec![
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("HOME".into(), home.display().to_string()),
    ]
}

impl Repo {
    pub fn new() -> Repo {
        let r = Repo { dir: tempfile::tempdir().unwrap(), n: 0 };
        r.git(&["-c", "init.defaultBranch=main", "init", "-q", "."]);
        r.git(&["config", "user.name", "Test User"]);
        r.git(&["config", "user.email", "test@example.invalid"]);
        r.git(&["config", "commit.gpgsign", "false"]);
        r
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let mut c = Command::new("git");
        c.current_dir(self.path()).args(args);
        for (k, v) in isolated_env(self.path()) {
            c.env(k, v);
        }
        let out = c.output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// Commit everything with a fixed, increasing date.
    pub fn commit(&mut self, msg: &str) -> String {
        self.n += 1;
        let date = format!("2026-01-{:02}T10:00:00+00:00", self.n);
        self.git(&["add", "-A"]);
        let mut c = Command::new("git");
        c.current_dir(self.path()).args(["commit", "-q", "-m", msg]).env("GIT_AUTHOR_DATE", &date).env("GIT_COMMITTER_DATE", &date);
        for (k, v) in isolated_env(self.path()) {
            c.env(k, v);
        }
        let out = c.output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    pub fn open(&self) -> Git {
        Git::open(self.path(), isolated_env(self.path())).unwrap()
    }
}

pub fn lines(n: usize, tag: &str) -> String {
    (0..n).map(|i| format!("line {i} of the {tag} file\n")).collect()
}

impl Repo {
    /// `n` commits on top of whatever exists, created at once with `git fast-import` (each touches `count.txt`).
    pub fn many_commits(&mut self, n: usize) {
        let has_head = self.path().join(".git/refs/heads/main").exists();
        let mut stream = String::new();
        for i in 0..n {
            let ts = 1_700_000_000 + i * 60;
            let msg = format!("commit number {i}");
            let body = format!("{i}\n");
            stream.push_str(&format!(
                "commit refs/heads/main\ncommitter Test User <test@example.invalid> {ts} +0000\ndata {}\n{msg}\n{}M 100644 inline count.txt\ndata {}\n{body}\n",
                msg.len(),
                if i == 0 && has_head { "from refs/heads/main^0\n" } else { "" },
                body.len()
            ));
        }
        let mut c = Command::new("git");
        c.current_dir(self.path()).args(["fast-import", "--quiet"]).stdin(std::process::Stdio::piped());
        for (k, v) in isolated_env(self.path()) {
            c.env(k, v);
        }
        let mut child = c.spawn().unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), stream.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        self.git(&["checkout", "-q", "-f", "main"]);
    }
}
