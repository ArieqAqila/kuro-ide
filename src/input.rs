//! Terminal input parser: bytes in, key / mouse / paste events out.
//!
//! A pure state machine (no I/O, no clock) so it can be tested exhaustively:
//! * legacy keys (control bytes, `ESC [ ...`, `ESC O ...`, Alt = ESC prefix)
//! * the kitty keyboard protocol (`CSI code ; mods u`), used for Ctrl+Enter, Shift+Tab, Esc, Alt+letter…
//! * SGR mouse reports (`CSI < b ; x ; y M/m`) with modifiers, drag and wheel
//! * bracketed paste
//! * sequences split across reads are reassembled; an incomplete sequence never yields a key
//!
//! A lone `ESC` is ambiguous (Esc key vs start of a sequence): it stays pending until more bytes arrive,
//! and the caller resolves it with [`InputParser::flush_timeout`] after a short quiet period.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    /// Reply to our `CSI ? u` query: the terminal supports the kitty keyboard protocol.
    KittyKeyboard,
    /// Reply to the primary device attributes query (`CSI c`): the terminal answered *something*.
    DeviceAttributes,
    /// Reply to an `OSC 4 ; n ; ?`, `OSC 10 ; ?` or `OSC 11 ; ?` colour query: the payload (`4;1;rgb:…`).
    ColorReply(String),
}

#[derive(Debug, Default)]
pub struct InputParser {
    buf: Vec<u8>,
    in_paste: bool,
    paste: Vec<u8>,
}

const PASTE_END: &[u8] = b"\x1b[201~";
/// Longest sequence we are willing to buffer before declaring it garbage.
const MAX_SEQ: usize = 256;

fn key(code: KeyCode, mods: KeyModifiers) -> InputEvent {
    InputEvent::Key(KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: KeyEventState::NONE })
}

fn mods_from_param(m: u32) -> KeyModifiers {
    // CSI modifier parameter = 1 + bitmask (shift 1, alt 2, ctrl 4, super 8)
    let bits = m.saturating_sub(1);
    let mut k = KeyModifiers::NONE;
    if bits & 1 != 0 {
        k |= KeyModifiers::SHIFT;
    }
    if bits & 2 != 0 {
        k |= KeyModifiers::ALT;
    }
    if bits & 4 != 0 {
        k |= KeyModifiers::CONTROL;
    }
    if bits & 8 != 0 {
        k |= KeyModifiers::SUPER;
    }
    k
}

/// Outcome of trying to parse one sequence at the start of the buffer.
enum Parsed {
    /// Consumed `n` bytes, producing these events.
    Done(usize, Vec<InputEvent>),
    /// Need more bytes.
    Incomplete,
}

impl InputParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while bytes are buffered waiting for the rest of an escape sequence or multi-byte character.
    /// (An unfinished bracketed paste is not "pending" in this sense: it must not be cut short by the
    /// Esc timeout, so callers do not flush it.)
    pub fn has_pending(&self) -> bool {
        !self.buf.is_empty() && !self.in_paste
    }

    /// Feed bytes read from the terminal.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<InputEvent> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.in_paste {
                if !self.feed_paste(&mut out) {
                    break;
                }
                continue;
            }
            if self.buf.is_empty() {
                break;
            }
            match self.parse_one() {
                Parsed::Done(n, evs) => {
                    self.buf.drain(..n);
                    out.extend(evs);
                }
                Parsed::Incomplete => {
                    if self.buf.len() > MAX_SEQ {
                        self.buf.clear(); // runaway garbage
                    }
                    break;
                }
            }
        }
        out
    }

    /// No more bytes arrived for a while: a lone `ESC` is the Esc key; any other incomplete
    /// sequence is dropped without producing a key.
    pub fn flush_timeout(&mut self) -> Vec<InputEvent> {
        let mut out = Vec::new();
        if self.in_paste {
            // an unterminated paste is delivered as-is rather than lost
            let mut text = std::mem::take(&mut self.paste);
            text.extend_from_slice(&std::mem::take(&mut self.buf));
            self.in_paste = false;
            out.push(InputEvent::Paste(String::from_utf8_lossy(&text).into_owned()));
            return out;
        }
        if self.buf.first() == Some(&0x1b) && self.buf.len() == 1 {
            out.push(key(KeyCode::Esc, KeyModifiers::NONE));
        }
        self.buf.clear();
        out
    }

    fn feed_paste(&mut self, out: &mut Vec<InputEvent>) -> bool {
        if let Some(i) = find(&self.buf, PASTE_END) {
            self.paste.extend_from_slice(&self.buf[..i]);
            self.buf.drain(..i + PASTE_END.len());
            let text = String::from_utf8_lossy(&std::mem::take(&mut self.paste)).into_owned();
            out.push(InputEvent::Paste(text));
            self.in_paste = false;
            return true;
        }
        // keep a possible partial terminator in the buffer, move the rest to the paste text
        let keep = (1..PASTE_END.len()).rev().find(|&n| self.buf.ends_with(&PASTE_END[..n])).unwrap_or(0);
        let take = self.buf.len() - keep;
        self.paste.extend_from_slice(&self.buf[..take]);
        self.buf.drain(..take);
        false
    }

    fn parse_one(&mut self) -> Parsed {
        let b = self.buf[0];
        match b {
            0x1b => self.parse_escape(),
            0x0d => Parsed::Done(1, vec![key(KeyCode::Enter, KeyModifiers::NONE)]),
            // Some terminals send LF for Ctrl+Enter; treat it as such.
            0x0a => Parsed::Done(1, vec![key(KeyCode::Enter, KeyModifiers::CONTROL)]),
            0x09 => Parsed::Done(1, vec![key(KeyCode::Tab, KeyModifiers::NONE)]),
            0x7f => Parsed::Done(1, vec![key(KeyCode::Backspace, KeyModifiers::NONE)]),
            0x08 => Parsed::Done(1, vec![key(KeyCode::Char('h'), KeyModifiers::CONTROL)]),
            0x00 => Parsed::Done(1, vec![key(KeyCode::Char(' '), KeyModifiers::CONTROL)]),
            0x01..=0x1a => Parsed::Done(1, vec![key(KeyCode::Char((b + 0x60) as char), KeyModifiers::CONTROL)]),
            0x1c..=0x1f => Parsed::Done(1, vec![key(KeyCode::Char((b + 0x40) as char), KeyModifiers::CONTROL)]),
            _ => self.parse_utf8(),
        }
    }

    fn parse_utf8(&mut self) -> Parsed {
        let b = self.buf[0];
        let need = match b {
            0x20..=0x7e => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Parsed::Done(1, vec![]), // stray continuation / invalid byte: skip it
        };
        if self.buf.len() < need {
            return Parsed::Incomplete;
        }
        match std::str::from_utf8(&self.buf[..need]) {
            Ok(s) => {
                let c = s.chars().next().unwrap();
                let mods = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
                Parsed::Done(need, vec![key(KeyCode::Char(c), mods)])
            }
            Err(_) => Parsed::Done(1, vec![]),
        }
    }

    fn parse_escape(&mut self) -> Parsed {
        if self.buf.len() == 1 {
            return Parsed::Incomplete; // Esc key or the start of a sequence
        }
        match self.buf[1] {
            b'[' => self.parse_csi(),
            b'O' => self.parse_ss3(),
            b']' => self.osc(),
            b'P' | b'_' | b'^' | b'X' => self.skip_string(), // DCS / APC...: replies we do not use
            0x1b => Parsed::Done(1, vec![key(KeyCode::Esc, KeyModifiers::NONE)]), // ESC ESC: first is a real Esc
            _ => {
                // ESC + X = Alt+X: parse X on its own and add the modifier
                let mut sub = InputParser { buf: self.buf[1..].to_vec(), ..Default::default() };
                match sub.parse_one() {
                    Parsed::Done(n, mut evs) => {
                        for e in &mut evs {
                            if let InputEvent::Key(k) = e {
                                k.modifiers |= KeyModifiers::ALT;
                            }
                        }
                        Parsed::Done(n + 1, evs)
                    }
                    Parsed::Incomplete => Parsed::Incomplete,
                }
            }
        }
    }

    /// An operating-system command, ended by BEL or ST. Colour replies become events; everything else is dropped.
    fn osc(&mut self) -> Parsed {
        for i in 2..self.buf.len() {
            let end = if self.buf[i] == 0x07 {
                Some(i + 1)
            } else if self.buf[i] == 0x1b && self.buf.get(i + 1) == Some(&b'\\') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(len) = end {
                let payload = String::from_utf8_lossy(&self.buf[2..i]).into_owned();
                let wanted = payload.starts_with("4;") || payload.starts_with("10;") || payload.starts_with("11;");
                return Parsed::Done(len, if wanted && payload.len() < 200 { vec![InputEvent::ColorReply(payload)] } else { vec![] });
            }
        }
        Parsed::Incomplete
    }

    fn skip_string(&mut self) -> Parsed {
        // terminated by BEL or ST (ESC \)
        for i in 2..self.buf.len() {
            if self.buf[i] == 0x07 {
                return Parsed::Done(i + 1, vec![]);
            }
            if self.buf[i] == 0x1b && self.buf.get(i + 1) == Some(&b'\\') {
                return Parsed::Done(i + 2, vec![]);
            }
        }
        Parsed::Incomplete
    }

    fn parse_ss3(&mut self) -> Parsed {
        let Some(&c) = self.buf.get(2) else { return Parsed::Incomplete };
        let code = match c {
            b'A' => KeyCode::Up,
            b'B' => KeyCode::Down,
            b'C' => KeyCode::Right,
            b'D' => KeyCode::Left,
            b'H' => KeyCode::Home,
            b'F' => KeyCode::End,
            b'P' => KeyCode::F(1),
            b'Q' => KeyCode::F(2),
            b'R' => KeyCode::F(3),
            b'S' => KeyCode::F(4),
            b'M' => KeyCode::Enter,
            b' ' => KeyCode::Char(' '),
            _ => return Parsed::Done(3, vec![]),
        };
        Parsed::Done(3, vec![key(code, KeyModifiers::NONE)])
    }

    fn parse_csi(&mut self) -> Parsed {
        // ESC [ <params 0x30-0x3f>* <intermediates 0x20-0x2f>* <final 0x40-0x7e>
        let mut i = 2;
        while i < self.buf.len() && (0x30..=0x3f).contains(&self.buf[i]) {
            i += 1;
        }
        let params_end = i;
        while i < self.buf.len() && (0x20..=0x2f).contains(&self.buf[i]) {
            i += 1;
        }
        let Some(&fin) = self.buf.get(i) else { return Parsed::Incomplete };
        if !(0x40..=0x7e).contains(&fin) {
            // malformed: drop the introducer so we resynchronise on the next byte
            return Parsed::Done(2, vec![]);
        }
        let len = i + 1;
        // X10 / normal mouse report: `CSI M` followed by three raw bytes (button, column, row; each + 32)
        if fin == b'M' && params_end == 2 {
            if self.buf.len() < len + 3 {
                return Parsed::Incomplete;
            }
            let (cb, cx, cy) = (self.buf[len], self.buf[len + 1], self.buf[len + 2]);
            let ev = legacy_mouse(cb as u32, (cx as u16).saturating_sub(32), (cy as u16).saturating_sub(32));
            return Parsed::Done(len + 3, ev.into_iter().collect());
        }
        let params = String::from_utf8_lossy(&self.buf[2..params_end]).into_owned();
        let has_intermediate = params_end != i;
        if has_intermediate {
            return Parsed::Done(len, vec![]);
        }
        Parsed::Done(len, self.interpret_csi(&params, fin))
    }

    fn interpret_csi(&mut self, params: &str, fin: u8) -> Vec<InputEvent> {
        // SGR mouse
        if let Some(rest) = params.strip_prefix('<') {
            if fin == b'M' || fin == b'm' {
                return sgr_mouse(rest, fin == b'm').into_iter().collect();
            }
            return vec![];
        }
        // urxvt mouse (1015): CSI Cb ; Cx ; Cy M
        if fin == b'M' {
            let nums: Vec<u32> = params.split(';').filter_map(|n| n.parse().ok()).collect();
            if let [cb, x, y] = nums[..] {
                return legacy_mouse(cb, x as u16, y as u16).into_iter().collect();
            }
            return vec![];
        }
        // replies to our queries
        if let Some(rest) = params.strip_prefix('?') {
            return match fin {
                b'u' if rest.chars().all(|c| c.is_ascii_digit()) => vec![InputEvent::KittyKeyboard],
                b'c' => vec![InputEvent::DeviceAttributes],
                _ => vec![],
            };
        }
        // parameters as numbers; sub-parameters after ':' are kept for kitty
        let nums: Vec<Vec<u32>> = params.split(';').map(|p| p.split(':').map(|n| n.parse().unwrap_or(0)).collect()).collect();
        let first = nums.first().and_then(|n| n.first()).copied().unwrap_or(0);
        let mod_param = |idx: usize| nums.get(idx).and_then(|n| n.first()).copied().unwrap_or(1).max(1);

        match fin {
            b'A' | b'B' | b'C' | b'D' | b'H' | b'F' => {
                let code = match fin {
                    b'A' => KeyCode::Up,
                    b'B' => KeyCode::Down,
                    b'C' => KeyCode::Right,
                    b'D' => KeyCode::Left,
                    b'H' => KeyCode::Home,
                    _ => KeyCode::End,
                };
                vec![key(code, mods_from_param(mod_param(1)))]
            }
            b'Z' => vec![key(KeyCode::BackTab, KeyModifiers::SHIFT)],
            b'P' | b'Q' | b'S' if nums.len() >= 2 => {
                let n = match fin {
                    b'P' => 1,
                    b'Q' => 2,
                    _ => 4,
                };
                vec![key(KeyCode::F(n), mods_from_param(mod_param(1)))]
            }
            b'R' if nums.len() >= 2 && first == 1 => vec![key(KeyCode::F(3), mods_from_param(mod_param(1)))],
            b'~' => {
                let mods = mods_from_param(mod_param(1));
                if first == 200 {
                    self.in_paste = true;
                    self.paste.clear();
                    return vec![];
                }
                let code = match first {
                    1 | 7 => KeyCode::Home,
                    2 => KeyCode::Insert,
                    3 => KeyCode::Delete,
                    4 | 8 => KeyCode::End,
                    5 => KeyCode::PageUp,
                    6 => KeyCode::PageDown,
                    11..=15 => KeyCode::F((first - 10) as u8),
                    17..=21 => KeyCode::F((first - 11) as u8),
                    23 | 24 => KeyCode::F((first - 12) as u8),
                    _ => return vec![],
                };
                vec![key(code, mods)]
            }
            b'u' => kitty_key(&nums).into_iter().collect(),
            // focus in/out, cursor position reports, and everything else: ignored
            _ => vec![],
        }
    }
}

/// Where in `buf` is the sequence `pat`?
fn find(buf: &[u8], pat: &[u8]) -> Option<usize> {
    buf.windows(pat.len()).position(|w| w == pat)
}

fn sgr_mouse(rest: &str, release: bool) -> Option<InputEvent> {
    let mut it = rest.split(';');
    let b: u32 = it.next()?.parse().ok()?;
    let x: u16 = it.next()?.parse().ok()?;
    let y: u16 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    decode_mouse(b, x, y, release)
}

/// The older mouse encodings, which carry the release as button number 3 instead of a final `m`:
/// X10 / normal `CSI M Cb Cx Cy` (three raw bytes, each offset by 32) and urxvt `CSI Cb ; Cx ; Cy M` (decimal, Cb offset by 32).
fn legacy_mouse(cb: u32, x: u16, y: u16) -> Option<InputEvent> {
    let b = cb.checked_sub(32)?;
    let release = b & 3 == 3 && b & 64 == 0;
    // a release does not say which button it was: report the left one
    decode_mouse(if release { b & !3 } else { b }, x, y, release)
}

fn decode_mouse(b: u32, x: u16, y: u16, release: bool) -> Option<InputEvent> {
    if x == 0 || y == 0 {
        return None;
    }
    let mut mods = KeyModifiers::NONE;
    if b & 4 != 0 {
        mods |= KeyModifiers::SHIFT;
    }
    if b & 8 != 0 {
        mods |= KeyModifiers::ALT;
    }
    if b & 16 != 0 {
        mods |= KeyModifiers::CONTROL;
    }
    let motion = b & 32 != 0;
    let button = match b & 3 {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        _ => MouseButton::Right,
    };
    let kind = if b & 64 != 0 {
        match b & 3 {
            0 => MouseEventKind::ScrollUp,
            1 => MouseEventKind::ScrollDown,
            2 => MouseEventKind::ScrollLeft,
            _ => MouseEventKind::ScrollRight,
        }
    } else if motion {
        if b & 3 == 3 { MouseEventKind::Moved } else { MouseEventKind::Drag(button) }
    } else if release {
        MouseEventKind::Up(button)
    } else {
        MouseEventKind::Down(button)
    };
    Some(InputEvent::Mouse(MouseEvent { kind, column: x - 1, row: y - 1, modifiers: mods }))
}

/// `CSI code[:shifted[:base]] ; mods[:event] [; text] u`
fn kitty_key(nums: &[Vec<u32>]) -> Option<InputEvent> {
    let code = *nums.first()?.first()?;
    let shifted = nums.first().and_then(|n| n.get(1)).copied().filter(|c| *c != 0);
    let mods_raw = nums.get(1).and_then(|n| n.first()).copied().unwrap_or(1).max(1);
    let event = nums.get(1).and_then(|n| n.get(1)).copied().unwrap_or(1);
    let mut mods = mods_from_param(mods_raw);
    let kind = match event {
        3 => KeyEventKind::Release,
        2 => KeyEventKind::Repeat,
        _ => KeyEventKind::Press,
    };
    let mut kc = match code {
        13 => KeyCode::Enter,
        9 => {
            if mods.contains(KeyModifiers::SHIFT) {
                return Some(InputEvent::Key(KeyEvent { code: KeyCode::BackTab, modifiers: mods, kind, state: KeyEventState::NONE }));
            }
            KeyCode::Tab
        }
        27 => KeyCode::Esc,
        127 | 8 => KeyCode::Backspace,
        57344..=63743 => return None, // private-use codes (keypad, media, modifier keys…)
        c => {
            let ch = char::from_u32(shifted.filter(|_| mods.contains(KeyModifiers::SHIFT)).unwrap_or(c))?;
            if ch.is_control() {
                return None;
            }
            // a shifted letter is delivered as the uppercase character with SHIFT set
            let ch = if mods.contains(KeyModifiers::SHIFT) && shifted.is_none() && ch.is_lowercase() {
                ch.to_uppercase().next().unwrap_or(ch)
            } else {
                ch
            };
            KeyCode::Char(ch)
        }
    };
    if let KeyCode::Char(c) = kc {
        kc = KeyCode::Char(if mods.contains(KeyModifiers::CONTROL) { c.to_ascii_lowercase() } else { c });
    }
    mods &= !(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META);
    Some(InputEvent::Key(KeyEvent { code: kc, modifiers: mods, kind, state: KeyEventState::NONE }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(bytes: &[u8]) -> Vec<KeyEvent> {
        let mut p = InputParser::new();
        let mut evs = p.feed(bytes);
        evs.extend(p.flush_timeout());
        evs.into_iter()
            .filter_map(|e| match e {
                InputEvent::Key(k) => Some(k),
                _ => None,
            })
            .collect()
    }

    fn one(bytes: &[u8]) -> KeyEvent {
        let k = keys(bytes);
        assert_eq!(k.len(), 1, "{bytes:?} -> {k:?}");
        k[0]
    }

    fn is(k: KeyEvent, code: KeyCode, mods: KeyModifiers) {
        assert_eq!((k.code, k.modifiers), (code, mods));
    }

    const N: KeyModifiers = KeyModifiers::NONE;
    const S: KeyModifiers = KeyModifiers::SHIFT;
    const A: KeyModifiers = KeyModifiers::ALT;
    const C: KeyModifiers = KeyModifiers::CONTROL;

    // ---- legacy terminals -------------------------------------------------------------

    #[test]
    fn legacy_plain_and_control_keys() {
        is(one(b"a"), KeyCode::Char('a'), N);
        is(one(b"A"), KeyCode::Char('A'), S);
        is(one(b" "), KeyCode::Char(' '), N);
        is(one(b"\r"), KeyCode::Enter, N);
        is(one(b"\t"), KeyCode::Tab, N);
        is(one(&[0x7f]), KeyCode::Backspace, N);
        is(one(&[0x03]), KeyCode::Char('c'), C);
        is(one(&[0x16]), KeyCode::Char('v'), C);
        is(one(&[0x08]), KeyCode::Char('h'), C);
        is(one(&[0x00]), KeyCode::Char(' '), C);
        is(one("é".as_bytes()), KeyCode::Char('é'), N);
        is(one("日".as_bytes()), KeyCode::Char('日'), N);
        is(one("😀".as_bytes()), KeyCode::Char('😀'), N);
    }

    #[test]
    fn legacy_ctrl_enter_is_the_same_byte_as_enter_or_lf() {
        // The core limitation: most terminals send CR for both, so Ctrl+Enter == Enter without the kitty protocol.
        assert_eq!(keys(b"\r"), keys(b"\r"));
        is(one(b"\r"), KeyCode::Enter, N);
        // some send LF for Ctrl+Enter, which we honour
        is(one(b"\n"), KeyCode::Enter, C);
        // Alt+Enter works everywhere: this is the documented fallback
        is(one(b"\x1b\r"), KeyCode::Enter, A);
    }

    #[test]
    fn legacy_escape_sequences() {
        is(one(b"\x1b[A"), KeyCode::Up, N);
        is(one(b"\x1b[B"), KeyCode::Down, N);
        is(one(b"\x1b[C"), KeyCode::Right, N);
        is(one(b"\x1b[D"), KeyCode::Left, N);
        is(one(b"\x1b[H"), KeyCode::Home, N);
        is(one(b"\x1b[F"), KeyCode::End, N);
        is(one(b"\x1b[1~"), KeyCode::Home, N);
        is(one(b"\x1b[4~"), KeyCode::End, N);
        is(one(b"\x1b[2~"), KeyCode::Insert, N);
        is(one(b"\x1b[3~"), KeyCode::Delete, N);
        is(one(b"\x1b[5~"), KeyCode::PageUp, N);
        is(one(b"\x1b[6~"), KeyCode::PageDown, N);
        is(one(b"\x1b[Z"), KeyCode::BackTab, S);
        is(one(b"\x1bOP"), KeyCode::F(1), N);
        is(one(b"\x1bOS"), KeyCode::F(4), N);
        is(one(b"\x1bOA"), KeyCode::Up, N);
        is(one(b"\x1b[15~"), KeyCode::F(5), N);
        is(one(b"\x1b[17~"), KeyCode::F(6), N);
        is(one(b"\x1b[18~"), KeyCode::F(7), N);
        is(one(b"\x1b[24~"), KeyCode::F(12), N);
    }

    #[test]
    fn legacy_modifiers_in_csi_parameters() {
        is(one(b"\x1b[1;2A"), KeyCode::Up, S);
        is(one(b"\x1b[1;5C"), KeyCode::Right, C);
        is(one(b"\x1b[1;3D"), KeyCode::Left, A);
        is(one(b"\x1b[1;6B"), KeyCode::Down, S | C);
        is(one(b"\x1b[3;5~"), KeyCode::Delete, C);
        is(one(b"\x1b[1;2R"), KeyCode::F(3), S);
        is(one(b"\x1b[1;2P"), KeyCode::F(1), S);
        is(one(b"\x1b[15;2~"), KeyCode::F(5), S);
    }

    #[test]
    fn alt_is_an_escape_prefix() {
        is(one(b"\x1bc"), KeyCode::Char('c'), A);
        is(one(b"\x1bx"), KeyCode::Char('x'), A);
        is(one(b"\x1bC"), KeyCode::Char('C'), A | S);
        is(one(b"\x1b\x7f"), KeyCode::Backspace, A);
        is(one(b"\x1b\r"), KeyCode::Enter, A);
        is(one("\x1bé".as_bytes()), KeyCode::Char('é'), A);
        is(one(b"\x1b\x03"), KeyCode::Char('c'), A | C);
    }

    #[test]
    fn a_lone_escape_is_the_esc_key_only_after_the_timeout() {
        let mut p = InputParser::new();
        assert!(p.feed(b"\x1b").is_empty(), "could still become a sequence");
        assert!(p.has_pending());
        let evs = p.flush_timeout();
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], InputEvent::Key(k) if k.code == KeyCode::Esc && k.modifiers == N));
        assert!(!p.has_pending());
    }

    #[test]
    fn escape_followed_by_a_sequence_is_not_an_esc_key() {
        let mut p = InputParser::new();
        p.feed(b"\x1b");
        let evs = p.feed(b"[A");
        assert_eq!(evs.len(), 1, "the ESC became the start of Up");
        assert!(matches!(&evs[0], InputEvent::Key(k) if k.code == KeyCode::Up));
    }

    #[test]
    fn double_escape_is_an_esc_then_whatever_follows() {
        let k = keys(b"\x1b\x1b[A");
        assert_eq!(k.len(), 2);
        is(k[0], KeyCode::Esc, N);
        is(k[1], KeyCode::Up, N);
    }

    // ---- kitty keyboard protocol -------------------------------------------------------

    #[test]
    fn kitty_ctrl_enter_is_distinguishable_from_enter() {
        is(one(b"\x1b[13;5u"), KeyCode::Enter, C);
        is(one(b"\x1b[13u"), KeyCode::Enter, N);
        is(one(b"\x1b[13;2u"), KeyCode::Enter, S);
        is(one(b"\x1b[13;3u"), KeyCode::Enter, A);
        is(one(b"\x1b[13;6u"), KeyCode::Enter, C | S);
    }

    #[test]
    fn kitty_shift_tab_esc_and_alt_letters() {
        is(one(b"\x1b[9;2u"), KeyCode::BackTab, S);
        is(one(b"\x1b[9u"), KeyCode::Tab, N);
        is(one(b"\x1b[27u"), KeyCode::Esc, N);
        is(one(b"\x1b[97;3u"), KeyCode::Char('a'), A);
        is(one(b"\x1b[99;3u"), KeyCode::Char('c'), A);
        is(one(b"\x1b[97;5u"), KeyCode::Char('a'), C);
        is(one(b"\x1b[122;6u"), KeyCode::Char('z'), C | S); // Ctrl+Shift+Z: the char stays lower-case, the SHIFT flag marks it
    }

    #[test]
    fn kitty_shifted_keys_use_the_alternate_code_when_given() {
        is(one(b"\x1b[97:65;2u"), KeyCode::Char('A'), S);
        is(one(b"\x1b[49:33;2u"), KeyCode::Char('!'), S);
    }

    #[test]
    fn kitty_backspace_and_events() {
        is(one(b"\x1b[127u"), KeyCode::Backspace, N);
        is(one(b"\x1b[127;3u"), KeyCode::Backspace, A);
        let mut p = InputParser::new();
        let evs = p.feed(b"\x1b[97;1:3u");
        assert!(matches!(&evs[0], InputEvent::Key(k) if k.kind == KeyEventKind::Release));
        let evs = p.feed(b"\x1b[97;1:2u");
        assert!(matches!(&evs[0], InputEvent::Key(k) if k.kind == KeyEventKind::Repeat));
    }

    #[test]
    fn kitty_private_use_and_unknown_codes_produce_no_key() {
        assert!(keys(b"\x1b[57441;2u").is_empty(), "a bare Shift key press");
        assert!(keys(b"\x1b[0u").is_empty());
        assert!(keys(b"\x1b[1114112u").is_empty(), "beyond Unicode");
    }

    #[test]
    fn query_replies_are_recognised_and_not_keys() {
        let mut p = InputParser::new();
        assert_eq!(p.feed(b"\x1b[?0u"), vec![InputEvent::KittyKeyboard]);
        assert_eq!(p.feed(b"\x1b[?1u"), vec![InputEvent::KittyKeyboard]);
        assert_eq!(p.feed(b"\x1b[?62;c"), vec![InputEvent::DeviceAttributes]);
        assert_eq!(p.feed(b"\x1b[?1;2c"), vec![InputEvent::DeviceAttributes]);
    }

    // ---- mouse -------------------------------------------------------------------------

    fn mouse(bytes: &[u8]) -> MouseEvent {
        let mut p = InputParser::new();
        let evs = p.feed(bytes);
        assert_eq!(evs.len(), 1, "{evs:?}");
        match &evs[0] {
            InputEvent::Mouse(m) => *m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn mouse_press_release_drag() {
        let m = mouse(b"\x1b[<0;10;5M");
        assert_eq!((m.kind, m.column, m.row, m.modifiers), (MouseEventKind::Down(MouseButton::Left), 9, 4, N));
        let m = mouse(b"\x1b[<0;10;5m");
        assert_eq!(m.kind, MouseEventKind::Up(MouseButton::Left));
        let m = mouse(b"\x1b[<32;11;5M");
        assert_eq!(m.kind, MouseEventKind::Drag(MouseButton::Left));
        assert_eq!((m.column, m.row), (10, 4));
        assert_eq!(mouse(b"\x1b[<2;1;1M").kind, MouseEventKind::Down(MouseButton::Right));
        assert_eq!(mouse(b"\x1b[<1;1;1M").kind, MouseEventKind::Down(MouseButton::Middle));
        assert_eq!(mouse(b"\x1b[<35;3;3M").kind, MouseEventKind::Moved);
    }

    #[test]
    fn mouse_wheel_with_modifiers() {
        assert_eq!(mouse(b"\x1b[<64;1;1M").kind, MouseEventKind::ScrollUp);
        assert_eq!(mouse(b"\x1b[<65;1;1M").kind, MouseEventKind::ScrollDown);
        assert_eq!(mouse(b"\x1b[<66;1;1M").kind, MouseEventKind::ScrollLeft);
        assert_eq!(mouse(b"\x1b[<67;1;1M").kind, MouseEventKind::ScrollRight);
        let m = mouse(b"\x1b[<69;1;1M"); // 64 + 1 + shift(4)
        assert_eq!((m.kind, m.modifiers), (MouseEventKind::ScrollDown, S));
        let m = mouse(b"\x1b[<16;4;4M"); // ctrl+click
        assert_eq!((m.kind, m.modifiers), (MouseEventKind::Down(MouseButton::Left), C));
        let m = mouse(b"\x1b[<28;4;4M"); // shift+alt+ctrl (4+8+16)
        assert_eq!(m.modifiers, S | A | C);
    }

    /// X10 / normal reporting: `CSI M` and three bytes, each 32 + value (column and row are 1-based).
    fn x10(button: u8, x: u8, y: u8) -> Vec<u8> {
        vec![0x1b, b'[', b'M', 32 + button, 32 + x, 32 + y]
    }

    #[test]
    fn colour_replies_become_events_and_other_osc_strings_are_dropped() {
        let mut p = InputParser::new();
        assert_eq!(p.feed(b"\x1b]4;1;rgb:cdcd/0000/0000\x07"), vec![InputEvent::ColorReply("4;1;rgb:cdcd/0000/0000".into())]);
        assert_eq!(p.feed(b"\x1b]11;rgb:0000/0000/0000\x1b\\"), vec![InputEvent::ColorReply("11;rgb:0000/0000/0000".into())]);
        assert_eq!(p.feed(b"\x1b]10;rgb:ffff/ffff/ffff\x07"), vec![InputEvent::ColorReply("10;rgb:ffff/ffff/ffff".into())]);
        assert!(p.feed(b"\x1b]0;a window title\x07").is_empty());
        assert!(p.feed(b"\x1b]52;c;aGk=\x07").is_empty());
        // split anywhere
        let bytes = b"\x1b]4;7;rgb:e5e5/e5e5/e5e5\x1b\\";
        for cut in 1..bytes.len() {
            let mut p = InputParser::new();
            assert!(p.feed(&bytes[..cut]).is_empty(), "cut {cut}");
            assert_eq!(p.feed(&bytes[cut..]).len(), 1, "cut {cut}");
        }
        // an absurdly long payload is not turned into an event
        let long = format!("\x1b]4;1;{}\x07", "a".repeat(500));
        assert!(InputParser::new().feed(long.as_bytes()).is_empty());
    }

    #[test]
    fn horizontal_wheel_in_x10_and_urxvt_reporting() {
        // buttons 6 and 7 are 64 + 2 and 64 + 3
        let m = mouse(&x10(66, 5, 3));
        assert_eq!((m.kind, m.column, m.row), (MouseEventKind::ScrollLeft, 4, 2));
        assert_eq!(mouse(&x10(67, 1, 1)).kind, MouseEventKind::ScrollRight);
        assert_eq!(mouse(&x10(64, 1, 1)).kind, MouseEventKind::ScrollUp);
        assert_eq!(mouse(&x10(65, 1, 1)).kind, MouseEventKind::ScrollDown);
        assert_eq!(mouse(&x10(67 + 4, 1, 1)).modifiers, S, "shift is bit 4 here too");
        // urxvt (1015): decimal, button offset by 32
        assert_eq!(mouse(b"\x1b[98;5;3M").kind, MouseEventKind::ScrollLeft);
        assert_eq!(mouse(b"\x1b[99;5;3M").kind, MouseEventKind::ScrollRight);
    }

    #[test]
    fn x10_clicks_drags_and_releases() {
        assert_eq!(mouse(&x10(0, 3, 4)).kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(mouse(&x10(2, 3, 4)).kind, MouseEventKind::Down(MouseButton::Right));
        assert_eq!(mouse(&x10(3, 3, 4)).kind, MouseEventKind::Up(MouseButton::Left), "button 3 is a release");
        assert_eq!(mouse(&x10(32, 3, 4)).kind, MouseEventKind::Drag(MouseButton::Left));
        assert_eq!(mouse(&x10(16, 3, 4)).modifiers, C);
        // urxvt press: button 0 + 32 = 32
        assert_eq!(mouse(b"\x1b[32;3;4M").kind, MouseEventKind::Down(MouseButton::Left));
    }

    #[test]
    fn an_x10_report_split_across_reads_is_reassembled_and_never_partial() {
        let bytes = x10(67, 10, 10);
        for cut in 1..bytes.len() {
            let mut p = InputParser::new();
            let first = p.feed(&bytes[..cut]);
            assert!(first.is_empty(), "a strict prefix gives nothing: {first:?}");
            let rest = p.feed(&bytes[cut..]);
            assert_eq!(rest.len(), 1);
            assert!(matches!(&rest[0], InputEvent::Mouse(m) if m.kind == MouseEventKind::ScrollRight));
        }
    }

    #[test]
    fn malformed_mouse_reports_never_become_mouse_events() {
        for bad in [&b"\x1b[<0;0;0M"[..], b"\x1b[<x;1;1M", b"\x1b[<0;1M", b"\x1b[<0;1;1;9M", b"\x1b[<;;M", b"\x1b[<99999999999;1;1M"] {
            let mut p = InputParser::new();
            let mut evs = p.feed(bad);
            evs.extend(p.flush_timeout());
            assert!(!evs.iter().any(|e| matches!(e, InputEvent::Mouse(_))), "{bad:?} -> {evs:?}");
        }
    }

    // ---- paste ---------------------------------------------------------------------------

    #[test]
    fn bracketed_paste_is_one_event_and_keys_inside_are_not_interpreted() {
        let mut p = InputParser::new();
        let evs = p.feed(b"\x1b[200~line1\r\nline2\x1b[A\tx\x1b[201~");
        assert_eq!(evs, vec![InputEvent::Paste("line1\r\nline2\x1b[A\tx".into())]);
        assert!(!p.has_pending());
        is(one(b"q"), KeyCode::Char('q'), N);
    }

    #[test]
    fn an_unfinished_paste_is_not_cut_short_by_the_escape_timeout() {
        let mut p = InputParser::new();
        assert!(p.feed(b"\x1b[200~half a pa").is_empty());
        assert!(!p.has_pending(), "callers only flush on has_pending, so the paste survives a pause");
        assert_eq!(p.feed(b"ste\x1b[201~"), vec![InputEvent::Paste("half a paste".into())]);
    }

    #[test]
    fn paste_split_anywhere_is_reassembled() {
        let full: &[u8] = b"\x1b[200~hello \xc3\xa9 world\x1b[201~";
        for split in 0..=full.len() {
            let mut p = InputParser::new();
            let mut evs = p.feed(&full[..split]);
            evs.extend(p.feed(&full[split..]));
            assert_eq!(evs, vec![InputEvent::Paste("hello é world".into())], "split at {split}");
        }
    }

    #[test]
    fn paste_followed_by_keys() {
        let mut p = InputParser::new();
        let evs = p.feed(b"\x1b[200~a\x1b[201~b");
        assert_eq!(evs.len(), 2);
        assert!(matches!(&evs[1], InputEvent::Key(k) if k.code == KeyCode::Char('b')));
    }

    // ---- reassembly and incompleteness ----------------------------------------------------

    /// A representative set of complete sequences.
    fn corpus() -> Vec<&'static [u8]> {
        vec![
            b"a",
            "é".as_bytes(),
            "😀".as_bytes(),
            b"\r",
            b"\x1b[A",
            b"\x1b[1;5C",
            b"\x1b[3~",
            b"\x1b[15~",
            b"\x1bOP",
            b"\x1b[13;5u",
            b"\x1b[9;2u",
            b"\x1b[27u",
            b"\x1b[97;3u",
            b"\x1b[<0;10;5M",
            b"\x1b[<65;1;1M",
            b"\x1b[<16;4;4m",
            b"\x1bc",
            b"\x1b[200~paste\x1b[201~",
            b"\x1b[?62;c",
        ]
    }

    fn all_events(chunks: &[&[u8]]) -> Vec<InputEvent> {
        let mut p = InputParser::new();
        let mut out = vec![];
        for c in chunks {
            out.extend(p.feed(c));
        }
        out.extend(p.flush_timeout());
        out
    }

    #[test]
    fn sequences_split_across_reads_are_reassembled_at_every_split_point() {
        for seq in corpus() {
            let whole = all_events(&[seq]);
            assert!(!whole.is_empty(), "{seq:?} must parse to something");
            for split in 1..seq.len() {
                let parts = all_events(&[&seq[..split], &seq[split..]]);
                assert_eq!(parts, whole, "{seq:?} split at {split}");
            }
            // one byte at a time
            let singles: Vec<&[u8]> = seq.chunks(1).collect();
            assert_eq!(all_events(&singles), whole, "{seq:?} byte by byte");
        }
    }

    #[test]
    fn an_incomplete_sequence_never_produces_a_key() {
        let multi: Vec<&[u8]> = vec![
            b"\x1b[1;5C",
            b"\x1b[15~",
            b"\x1b[13;5u",
            b"\x1b[<0;10;5M",
            b"\x1b[200~paste\x1b[201~",
            "😀".as_bytes(),
            b"\x1b]52;c;aGk=\x07",
        ];
        for seq in multi {
            for cut in 1..seq.len() {
                let prefix = &seq[..cut];
                let mut p = InputParser::new();
                let evs = p.feed(prefix);
                let key_or_mouse = |e: &InputEvent| matches!(e, InputEvent::Key(_) | InputEvent::Mouse(_));
                assert!(!evs.iter().any(key_or_mouse), "prefix {prefix:?} of {seq:?} produced {evs:?}");
                // the timeout must not turn the leftover into a key either (a lone ESC excepted)
                let after = p.flush_timeout();
                let bad: Vec<_> = after.iter().filter(|e| key_or_mouse(e)).collect();
                if prefix == b"\x1b" {
                    assert!(matches!(bad.as_slice(), [InputEvent::Key(k)] if k.code == KeyCode::Esc));
                } else {
                    assert!(bad.is_empty(), "flush of {prefix:?} (from {seq:?}) produced {bad:?}");
                }
                assert!(!p.has_pending(), "flush leaves a clean parser");
            }
        }
    }

    #[test]
    fn invalid_utf8_and_garbage_are_skipped_without_panicking() {
        let k = keys(&[0xff, 0xfe, b'a', 0x80, 0xc2, b'b']);
        assert_eq!(k.iter().filter_map(|k| if let KeyCode::Char(c) = k.code { Some(c) } else { None }).collect::<String>(), "ab");
    }

    #[test]
    fn runaway_sequences_are_bounded() {
        let mut p = InputParser::new();
        let mut junk = vec![0x1b, b'['];
        junk.extend(std::iter::repeat_n(b'1', 2000));
        p.feed(&junk);
        assert!(p.buf.len() <= MAX_SEQ + 1 || p.buf.is_empty(), "buffer is bounded");
        p.flush_timeout();
        is(one(b"x"), KeyCode::Char('x'), N);
    }

    #[test]
    fn mixed_stream_parses_in_order() {
        let k = keys(b"ab\x1b[1;5A\r\x1bx\x1b[13;5u");
        let got: Vec<(KeyCode, KeyModifiers)> = k.iter().map(|k| (k.code, k.modifiers)).collect();
        assert_eq!(
            got,
            vec![
                (KeyCode::Char('a'), N),
                (KeyCode::Char('b'), N),
                (KeyCode::Up, C),
                (KeyCode::Enter, N),
                (KeyCode::Char('x'), A),
                (KeyCode::Enter, C),
            ]
        );
    }
}
