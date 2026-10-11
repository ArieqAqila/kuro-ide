//! Opt-in check against the machine's real clipboard (Wayland or X11).
//! `env KURO_TEST_REAL_CLIPBOARD=1 cargo test --test clipboard_real`
//! It saves a plain-text clipboard first and restores it afterwards; if the clipboard holds
//! something that is not plain text it refuses to run rather than lose it.

use kuro::clipboard::{Clipboard, detect};
use std::process::Command;

fn types() -> String {
    let out = Command::new("wl-paste").arg("--list-types").output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
}

#[test]
fn real_clipboard_roundtrip() {
    if std::env::var("KURO_TEST_REAL_CLIPBOARD").ok().as_deref() != Some("1") {
        eprintln!("SKIPPED: set KURO_TEST_REAL_CLIPBOARD=1 to test the real system clipboard");
        return;
    }
    let t = types();
    let is_text = |l: &str| l.starts_with("text/") || matches!(l, "TEXT" | "STRING" | "UTF8_STRING" | "COMPOUND_TEXT");
    if t.lines().any(|l| !l.trim().is_empty() && !is_text(l.trim())) {
        eprintln!("SKIPPED: clipboard holds non-text data ({t:?}); not touching it");
        return;
    }
    let mut clip: Box<dyn Clipboard> = detect("auto");
    let saved = clip.get().unwrap_or_default();
    let probe = "kuro-clipboard-probe é😀\nline2";
    clip.set(probe).unwrap();
    let got = clip.get().unwrap();
    // restore first, assert after
    let _ = clip.set(&saved);
    eprintln!("backend = {}", clip.name());
    assert_eq!(got, probe);
}
