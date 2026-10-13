//! `kuro --show-input`: what the terminal sent, as kuro parsed it (the tool for "my wheel does nothing").

mod common;
use common::pty::*;

#[test]
fn it_shows_the_raw_bytes_and_the_parsed_events_of_the_wheel_and_modifiers_and_quits_on_ctrl_c() {
    let dir = tempfile::tempdir().unwrap();
    let mut pty = Pty::spawn_args(dir.path(), "", &["--show-input"], dir.path(), &[]);
    pty.wait_for("the banner", |s| s.iter().any(|r| r.contains("--show-input")));
    pty.mouse("wheel_down", 10, 5);
    let s = pty.wait_for("the wheel", |s| s.iter().any(|r| r.contains("ScrollDown")));
    assert!(s.iter().any(|r| r.contains("bytes \\x1b[<65;11;6M")), "{}", s.join("\n"));
    pty.send("\x1b[<69;3;3M"); // wheel down with Shift (the bit 4)
    let s = pty.wait_for("shift wheel", |s| s.iter().any(|r| r.contains("SHIFT")));
    assert!(s.iter().any(|r| r.contains("ScrollDown")));
    pty.send("\x1b[<66;3;3M"); // a horizontal wheel button
    pty.wait_for("a horizontal wheel", |s| s.iter().any(|r| r.contains("ScrollLeft")));
    pty.send(CTRL_C);
    assert!(pty.wait_exit(), "Ctrl+C leaves");
}
