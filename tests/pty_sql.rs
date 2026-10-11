//! End-to-end SQL in the real binary: connect to a throwaway PostgreSQL container, run a
//! query, scroll the table, trigger a syntax error. Skipped when Docker is unavailable.

mod common;
use common::pty::*;
use common::*;

#[test]
fn run_scroll_and_error_in_a_real_terminal() {
    if docker_unavailable().is_some() {
        return;
    }
    let c = Container::start("ptypg", "postgres:16-alpine", 5432, &[("POSTGRES_PASSWORD", "{pw}")]);
    c.wait_log("database system is ready to accept connections", 2);
    let dir = tempfile::tempdir().unwrap();
    // credentials come from the environment, never from a file
    std::fs::write(
        dir.path().join("connections.toml"),
        format!(
            "[profiles.t]\ndriver = \"postgres\"\nhost = \"127.0.0.1\"\nport = {}\nuser = \"postgres\"\npassword = \"env:KURO_TEST_PW\"\ndatabase = \"postgres\"\n",
            c.port
        ),
    )
    .unwrap();
    let query = dir.path().join("q.sql");
    std::fs::write(&query, "SELECT g AS n, md5(g::text) AS h, repeat('z', 80) AS wide FROM generate_series(1, 200) g;\n").unwrap();
    let mut pty = Pty::spawn(dir.path(), "[clipboard]\nbackend = \"osc52\"\n", Some(&query), &[("KURO_TEST_PW", &c.password)]);
    pty.wait_for("start", |s| s[0].contains("SELECT"));

    // the container may still be finishing startup: retry until the connection is established
    let t = std::time::Instant::now();
    loop {
        pty.send(":sql connect t\r");
        let s = pty
            .wait_for_secs("connect result", 20, |s| s[28].contains("● postgres://") || s.iter().any(|r| r.contains("could not connect")));
        if s[28].contains("● postgres://") {
            break;
        }
        assert!(t.elapsed().as_secs() < 60, "never connected:\n{}", s.join("\n"));
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    pty.wait_for("connected", |s| s[28].contains("● postgres://"));

    pty.send(F6); // run whole buffer
    let s = pty.wait_for("result table", |s| s.iter().any(|r| r.contains("200 rows")));
    assert!(s.iter().any(|r| r.contains("n int4") && r.contains("h text")), "header with column types:\n{}", s.join("\n"));
    assert!(s.iter().any(|r| r.contains("ms")), "execution time shown");
    let hdr = s.iter().position(|r| r.contains("n int4")).unwrap();
    assert!(s[hdr + 2].trim_start().starts_with('1'), "first row: {:?}", s[hdr + 2]);
    assert!(s[hdr + 2..].iter().filter(|r| r.contains("│")).count() <= 25, "at most 25 rows visible");
    assert!(s.iter().any(|r| r.contains('…')), "the 80-char column is truncated with an ellipsis");
    pty.assert_default_background();

    // keyboard scrolling vertically and horizontally; header stays
    pty.send(F4);
    for _ in 0..5 {
        pty.send("j");
    }
    let s = pty.wait_for("scrolled", |s| s.iter().any(|r| r.contains("rows 6-")));
    assert!(s.iter().any(|r| r.contains("n int4")), "header still visible");
    pty.send(PGDN);
    pty.wait_for("page down", |s| s.iter().any(|r| r.contains("rows 19-")));
    pty.send("l");
    pty.send("l");
    pty.wait_for("col 3", |s| s.iter().any(|r| r.contains("cols 3/3")));
    pty.send("e");
    pty.wait_for("expanded", |s| s.iter().any(|r| r.contains(&"z".repeat(70))));
    // mouse wheel over the table
    pty.mouse("wheel_down", 10, hdr as u16 + 3);
    pty.wait_for("wheel", |s| s.iter().any(|r| r.contains("rows ")));
    pty.send(ESC);

    // syntax error: shown with category, position and the server's message; editor still alive
    std::fs::write(&query, "").unwrap();
    pty.send("ggVGd"); // clear the buffer
    pty.send(&format!("iSELEC 1{ESC}")); // insert a typo'd statement
    pty.send(CTRL_ENTER); // runs the statement under the cursor
    let s = pty.wait_for("error", |s| s.iter().any(|r| r.contains("syntax error at line 1, col 1")));
    assert!(s.iter().any(|r| r.contains("syntax error at or near \"SELEC\"")), "{}", s.join("\n"));
    assert!(s.iter().any(|r| r.contains("✖ syntax error")), "status line");
    assert!(s[0].contains("SELEC 1"), "editor stays visible and usable");

    // runtime and execution errors
    pty.send("ggVGd");
    pty.send(&format!("iSELECT 1/0{ESC}"));
    pty.send(CTRL_ENTER);
    pty.wait_for("runtime", |s| s.iter().any(|r| r.contains("runtime error")) && s.iter().any(|r| r.contains("division by zero")));
    pty.send("ggVGd");
    pty.send(&format!("iSELECT * FROM nope{ESC}"));
    pty.send(ALT_ENTER); // the fallback key does the same
    pty.wait_for("execution", |s| s.iter().any(|r| r.contains("execution error")) && s.iter().any(|r| r.contains("does not exist")));

    // cancel a long-running query
    pty.send("ggVGd");
    pty.send(&format!("iSELECT pg_sleep(30){ESC}"));
    pty.send(F6);
    pty.wait_for("running", |s| s.iter().any(|r| r.contains("running")));
    pty.send("\x1b[18~"); // F7
    let s = pty.wait_for("cancelled", |s| s.iter().any(|r| r.contains("canceling statement")));
    assert!(s.iter().any(|r| r.contains("execution error")));
}

#[test]
fn prompted_password_is_reused_dropped_when_wrong_and_never_written_to_disk() {
    if docker_unavailable().is_some() {
        return;
    }
    let c = Container::start("ptypw", "postgres:16-alpine", 5432, &[("POSTGRES_PASSWORD", "{pw}")]);
    c.wait_log("database system is ready to accept connections", 2);
    let dir = tempfile::tempdir().unwrap();
    // every place the program could write to lives under `dir`, so one scan covers them all
    let sub = |n: &str| {
        let p = dir.path().join(n);
        std::fs::create_dir_all(&p).unwrap();
        p
    };
    let (home, state, data, runtime) = (sub("home"), sub("state"), sub("data"), sub("run"));
    std::fs::write(
        dir.path().join("connections.toml"),
        format!(
            "[profiles.good]\ndriver = \"postgres\"\nhost = \"127.0.0.1\"\nport = {p}\nuser = \"postgres\"\npassword = \"prompt\"\ndatabase = \"postgres\"\n\n[profiles.other]\ndriver = \"postgres\"\nhost = \"127.0.0.1\"\nport = {p}\nuser = \"postgres\"\npassword = \"prompt\"\ndatabase = \"template1\"\n",
            p = c.port
        ),
    )
    .unwrap();
    let env = [
        ("HOME", home.to_str().unwrap()),
        ("XDG_STATE_HOME", state.to_str().unwrap()),
        ("XDG_DATA_HOME", data.to_str().unwrap()),
        ("XDG_RUNTIME_DIR", runtime.to_str().unwrap()),
    ];
    let mut pty = Pty::spawn(dir.path(), "[clipboard]\nbackend = \"osc52\"\n", None, &env);
    pty.wait_for("start", |s| s[28].contains("NORMAL"));
    let bottom = |s: &[String]| s[29].trim_end().to_string();

    // first connect: prompt, typed password is never echoed
    let t = std::time::Instant::now();
    loop {
        pty.send(":connect good\r");
        pty.wait_for("password prompt", |s| bottom(s).contains("password for good"));
        pty.send(&c.password);
        let s = pty.wait_for("masked", |s| bottom(s).contains('•'));
        assert!(!s.iter().any(|r| r.contains(&c.password)), "password echoed on screen");
        pty.send("\r");
        let s = pty.wait_for_secs("connected or failed", 20, |s| {
            s[28].contains("● postgres://") || s.iter().any(|r| r.contains("could not connect"))
        });
        if s[28].contains("● postgres://") {
            break;
        }
        assert!(t.elapsed().as_secs() < 60, "never connected:\n{}", s.join("\n"));
    }

    // reconnecting to the same profile does not ask again
    pty.send(":disconnect\r");
    pty.wait_for("disconnected", |s| !s[28].contains("● postgres://"));
    pty.send(":connect good\r");
    let s = pty.wait_for("reconnected without a prompt", |s| s[28].contains("● postgres://"));
    assert!(!s.iter().any(|r| r.contains("password for")), "it must not ask again:\n{}", s.join("\n"));

    // a different profile asks; a wrong password is rejected and not remembered
    pty.send(":connect other\r");
    pty.wait_for("prompt for the other profile", |s| bottom(s).contains("password for other"));
    pty.send("not-the-password\r");
    let s = pty.wait_for("rejected", |s| s.iter().any(|r| r.contains("could not connect") || r.contains("password authentication failed")));
    assert!(s.iter().any(|r| r.contains("wrong password") || r.contains("authentication failed")), "{}", s.join("\n"));
    pty.send(ESC);
    pty.send(":connect other\r");
    pty.wait_for("asks again after a wrong password", |s| bottom(s).contains("password for other"));
    pty.send(ESC);

    // quit, then look at every file under every directory the program could have used
    pty.send(CTRL_Q);
    assert!(pty.wait_exit());
    let raw = String::from_utf8_lossy(&pty.raw_output()).into_owned();
    assert!(!raw.contains(&c.password), "the password appeared in the terminal output stream");
    let mut hits = vec![];
    fn scan(dir: &std::path::Path, needle: &[u8], hits: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                scan(&p, needle, hits);
            } else if std::fs::read(&p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle)) {
                hits.push(p.display().to_string());
            }
        }
    }
    scan(dir.path(), c.password.as_bytes(), &mut hits);
    assert!(hits.is_empty(), "the password was written to disk: {hits:?}");
}
