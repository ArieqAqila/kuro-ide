//! Output filter that guarantees the colour rule: text colours are only the terminal's default
//! and ANSI palette colours, written as the classic `30-37`/`90-97` codes.
//!
//! crossterm writes even the 16 named colours as `ESC[38;5;Nm` (the 256-colour form). Terminals
//! treat slots 0-15 of that like the palette, but "never emit 256-colour sequences" is stricter, so
//! every SGR sequence passing through is rewritten: `38;5;N` / `48;5;N` with N < 16 become
//! `3x`/`9x` / `4x`/`10x`. Everything else is passed through untouched.

use std::io::{self, Write};

/// SGR codes for palette slot `n` (0-15) as foreground or background.
fn palette_code(n: u32, background: bool) -> u32 {
    match (n < 8, background) {
        (true, false) => 30 + n,
        (false, false) => 90 + (n - 8),
        (true, true) => 40 + n,
        (false, true) => 100 + (n - 8),
    }
}

/// Rewrite the parameter list of one SGR sequence (the text between `ESC[` and `m`).
pub fn rewrite_sgr(params: &str) -> String {
    let parts: Vec<&str> = params.split(';').collect();
    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        let palette = if (p == "38" || p == "48") && parts.get(i + 1) == Some(&"5") {
            parts.get(i + 2).and_then(|s| s.parse::<u32>().ok()).filter(|n| *n < 16)
        } else {
            None
        };
        if let Some(n) = palette {
            out.push(palette_code(n, p == "48").to_string());
            i += 3;
            continue;
        }
        out.push(p.to_string());
        i += 1;
    }
    out.join(";")
}

/// Wraps a writer and rewrites SGR colour sequences on the fly (handles sequences split across writes).
pub struct SgrFilter<W: Write> {
    inner: W,
    pending: Vec<u8>,
}

impl<W: Write> SgrFilter<W> {
    pub fn new(inner: W) -> Self {
        SgrFilter { inner, pending: Vec::new() }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }

    /// Feed bytes, returning what should be written downstream.
    pub fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            if self.pending.is_empty() {
                if b == 0x1b {
                    self.pending.push(b);
                } else {
                    out.push(b);
                }
                continue;
            }
            self.pending.push(b);
            let n = self.pending.len();
            if n == 2 && b != b'[' {
                out.append(&mut self.pending);
            } else if n >= 3 && (0x40..=0x7e).contains(&b) {
                if b == b'm' && self.pending[2..n - 1].iter().all(|c| c.is_ascii_digit() || *c == b';') {
                    let params = std::str::from_utf8(&self.pending[2..n - 1]).unwrap_or("");
                    out.extend_from_slice(b"\x1b[");
                    out.extend_from_slice(rewrite_sgr(params).as_bytes());
                    out.push(b'm');
                } else {
                    out.extend_from_slice(&self.pending);
                }
                self.pending.clear();
            } else if n > 64 {
                out.append(&mut self.pending); // not a real sequence: give up and pass through
            }
        }
        out
    }
}

impl<W: Write> Write for SgrFilter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let out = self.filter(buf);
        self.inner.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[u8]) -> Vec<u8> {
        let mut f = SgrFilter::new(Vec::new());
        f.write_all(input).unwrap();
        f.into_inner()
    }

    #[test]
    fn rewrites_indexed_palette_colours_to_classic_codes() {
        assert_eq!(run(b"\x1b[38;5;1mx"), b"\x1b[31mx");
        assert_eq!(run(b"\x1b[38;5;7mx"), b"\x1b[37mx");
        assert_eq!(run(b"\x1b[38;5;8mx"), b"\x1b[90mx");
        assert_eq!(run(b"\x1b[38;5;15mx"), b"\x1b[97mx");
        assert_eq!(run(b"\x1b[48;5;4mx"), b"\x1b[44mx");
        assert_eq!(run(b"\x1b[48;5;12mx"), b"\x1b[104mx");
    }

    #[test]
    fn rewrites_inside_combined_parameter_lists() {
        assert_eq!(run(b"\x1b[38;5;5;49m"), b"\x1b[35;49m");
        assert_eq!(run(b"\x1b[1;38;5;2;4m"), b"\x1b[1;32;4m");
        assert_eq!(rewrite_sgr("38;5;3;48;5;9"), "33;101");
    }

    #[test]
    fn leaves_everything_else_untouched() {
        for s in [
            &b"plain text"[..],
            b"\x1b[1m\x1b[0m",
            b"\x1b[39m\x1b[49m",
            b"\x1b[2J\x1b[H",
            b"\x1b[?25h",
            b"\x1b[3;4H",
            b"\x1b]52;c;aGk=\x07",
            "日本語".as_bytes(),
        ] {
            assert_eq!(run(s), s);
        }
    }

    #[test]
    fn colours_outside_the_palette_are_not_invented() {
        // not our business to rewrite these (tests assert they are never produced)
        assert_eq!(run(b"\x1b[38;5;200m"), b"\x1b[38;5;200m");
        assert_eq!(run(b"\x1b[38;2;1;2;3m"), b"\x1b[38;2;1;2;3m");
    }

    #[test]
    fn handles_sequences_split_across_writes() {
        let mut f = SgrFilter::new(Vec::new());
        for chunk in [&b"ab\x1b"[..], b"[38;", b"5;", b"6", b"mcd\x1b[3", b"9m"] {
            f.write_all(chunk).unwrap();
        }
        assert_eq!(f.into_inner(), b"ab\x1b[36mcd\x1b[39m");
    }

    #[test]
    fn lone_escape_and_garbage_pass_through() {
        assert_eq!(run(b"\x1bx"), b"\x1bx");
        let long = [b"\x1b[".as_slice(), &[b'1'; 100], b"m"].concat();
        assert_eq!(run(&long), long);
    }
}
