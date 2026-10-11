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
