//! Snapshot ("golden screen") testing: a rendered cell buffer compared with a committed text file.
//!
//! Each snapshot has three grids: the characters, the text style (R reverse, U underline, B bold, D dim,
//! I italic) and the foreground colour (r g y b m c = ANSI red/green/yellow/blue/magenta/cyan, w white/gray,
//! k black, uppercase = bright). Update after an intended change with `KURO_UPDATE_SNAPSHOTS=1`
//! and review the diff in git.
#![allow(dead_code)]

use ratatui::buffer::Buffer as Cells;
use ratatui::style::{Color, Modifier};
use std::path::PathBuf;

fn style_char(m: Modifier) -> char {
    if m.contains(Modifier::REVERSED) {
        'R'
    } else if m.contains(Modifier::UNDERLINED) {
        'U'
    } else if m.contains(Modifier::BOLD) {
        'B'
    } else if m.contains(Modifier::ITALIC) {
        'I'
    } else if m.contains(Modifier::DIM) {
        'D'
    } else {
        '.'
    }
}

fn color_char(c: Color) -> char {
    match c {
        Color::Reset => '.',
        Color::Black => 'k',
        Color::Red => 'r',
        Color::Green => 'g',
        Color::Yellow => 'y',
        Color::Blue => 'b',
        Color::Magenta => 'm',
        Color::Cyan => 'c',
        Color::Gray => 'w',
        Color::DarkGray => 'K',
        Color::LightRed => 'R',
        Color::LightGreen => 'G',
        Color::LightYellow => 'Y',
        Color::LightBlue => 'B',
        Color::LightMagenta => 'M',
        Color::LightCyan => 'C',
        Color::White => 'W',
        _ => '?', // RGB / indexed: must never appear
    }
}

/// Render `buf` as the snapshot text.
pub fn serialize(buf: &Cells) -> String {
    let a = buf.area;
    let mut text = vec![];
    let mut style = vec![];
    let mut fg = vec![];
    for y in a.y..a.bottom() {
        let (mut t, mut s, mut f) = (String::new(), String::new(), String::new());
        let mut skip = 0;
        for x in a.x..a.right() {
            let c = &buf[(x, y)];
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let sym = c.symbol();
            let w = kuro::table::display_width(sym).max(1);
            skip = w - 1;
            t.push_str(sym);
            for _ in 0..w {
                s.push(style_char(c.modifier));
                f.push(color_char(c.fg));
            }
        }
        text.push(format!("|{t}|"));
        style.push(format!("|{s}|"));
        fg.push(format!("|{f}|"));
    }
    format!("== text {}x{} ==\n{}\n== style ==\n{}\n== fg ==\n{}\n", a.width, a.height, text.join("\n"), style.join("\n"), fg.join("\n"))
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(format!("{name}.txt"))
}

pub fn assert_snapshot(name: &str, buf: &Cells) {
    let got = serialize(buf);
    let p = path(name);
    if std::env::var("KURO_UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1") {
        std::fs::write(&p, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&p).unwrap_or_else(|_| {
        panic!("snapshot {name} does not exist yet; run with KURO_UPDATE_SNAPSHOTS=1, review {} and commit it", p.display())
    });
    if want != got {
        let mut msg = format!("snapshot {name} differs (update with KURO_UPDATE_SNAPSHOTS=1 if the change is intended):\n");
        for (i, (w, g)) in want.lines().zip(got.lines()).enumerate() {
            if w != g {
                msg.push_str(&format!("line {}:\n  want {w}\n  got  {g}\n", i + 1));
                if i > 6 {
                    msg.push_str("  ...\n");
                    break;
                }
            }
        }
        if want.lines().count() != got.lines().count() {
            msg.push_str(&format!("line count: want {}, got {}\n", want.lines().count(), got.lines().count()));
        }
        panic!("{msg}");
    }
}
