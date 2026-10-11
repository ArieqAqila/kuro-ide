//! Parse key specs like `"alt+enter"`, `"ctrl+shift+f5"`, `"alt+l"` (used by configurable bindings).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    /// Does the key event press this chord? (Shift on a letter is folded into the character's case.)
    pub fn matches(&self, k: &KeyEvent) -> bool {
        let norm = |code: KeyCode, mods: KeyModifiers| -> (KeyCode, KeyModifiers) {
            match code {
                KeyCode::Char(c) => {
                    // letters compare case-insensitively; SHIFT is only meaningful with the chord's own request
                    let mut m = mods;
                    if c.is_uppercase() {
                        m |= KeyModifiers::SHIFT;
                    }
                    (KeyCode::Char(c.to_ascii_lowercase()), m)
                }
                KeyCode::BackTab => (KeyCode::Tab, mods | KeyModifiers::SHIFT),
                other => (other, mods),
            }
        };
        let relevant = KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL;
        let (c1, m1) = norm(self.code, self.mods);
        let (c2, m2) = norm(k.code, k.modifiers);
        c1 == c2 && (m1 & relevant) == (m2 & relevant)
    }
}

impl std::fmt::Display for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = vec![];
        if self.mods.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl".to_string());
        }
        if self.mods.contains(KeyModifiers::ALT) {
            parts.push("Alt".to_string());
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            parts.push("Shift".to_string());
        }
        parts.push(match self.code {
            KeyCode::Enter => "Enter".into(),
            KeyCode::Tab => "Tab".into(),
            KeyCode::Esc => "Esc".into(),
            KeyCode::Backspace => "Backspace".into(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Char(' ') => "Space".into(),
            KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
            other => format!("{other:?}"),
        });
        write!(f, "{}", parts.join("+"))
    }
}

/// `None` when the spec is empty, has an unknown modifier or key name.
pub fn parse(spec: &str) -> Option<Chord> {
    let spec = spec.trim().to_ascii_lowercase();
    if spec.is_empty() {
        return None;
    }
    let mut mods = KeyModifiers::NONE;
    let mut parts: Vec<&str> = spec.split('+').collect();
    let key = parts.pop()?;
    for p in parts {
        mods |= match p.trim() {
            "ctrl" | "control" | "c" => KeyModifiers::CONTROL,
            "alt" | "meta" | "m" | "option" => KeyModifiers::ALT,
            "shift" | "s" => KeyModifiers::SHIFT,
            _ => return None,
        };
    }
    let code = match key.trim() {
        "enter" | "return" | "cr" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "esc" | "escape" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        "backspace" | "bs" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        f if f.len() >= 2 && f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => {
            let n: u8 = f[1..].parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            KeyCode::F(n)
        }
        c if c.chars().count() == 1 => KeyCode::Char(c.chars().next()?),
        _ => return None,
    };
    Some(Chord { code, mods })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    #[test]
    fn parses_common_specs() {
        assert_eq!(parse("alt+enter"), Some(Chord { code: KeyCode::Enter, mods: KeyModifiers::ALT }));
        assert_eq!(parse("Ctrl+Shift+F5"), Some(Chord { code: KeyCode::F(5), mods: KeyModifiers::CONTROL | KeyModifiers::SHIFT }));
        assert_eq!(parse("alt+l"), Some(Chord { code: KeyCode::Char('l'), mods: KeyModifiers::ALT }));
        assert_eq!(parse("f9"), Some(Chord { code: KeyCode::F(9), mods: KeyModifiers::NONE }));
        assert_eq!(parse(" ctrl + space "), parse("ctrl+space"), "spacing around + is tolerated");
        assert_eq!(parse("ctrl+space").map(|c| c.code), Some(KeyCode::Char(' ')));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "alt+", "+enter", "hyper+x", "alt+nokey", "f0", "f25", "ctrl+alt+"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn matching_ignores_letter_case_and_requires_the_exact_modifiers() {
        let c = parse("alt+l").unwrap();
        assert!(c.matches(&ev(KeyCode::Char('l'), KeyModifiers::ALT)));
        assert!(!c.matches(&ev(KeyCode::Char('l'), KeyModifiers::NONE)));
        assert!(!c.matches(&ev(KeyCode::Char('l'), KeyModifiers::ALT | KeyModifiers::CONTROL)));
        assert!(!c.matches(&ev(KeyCode::Char('k'), KeyModifiers::ALT)));
        let e = parse("alt+enter").unwrap();
        assert!(e.matches(&ev(KeyCode::Enter, KeyModifiers::ALT)));
        assert!(!e.matches(&ev(KeyCode::Enter, KeyModifiers::NONE)), "plain Enter is not the fallback");
        assert!(!e.matches(&ev(KeyCode::Enter, KeyModifiers::CONTROL)));
    }

    #[test]
    fn display_is_human_readable() {
        assert_eq!(parse("alt+enter").unwrap().to_string(), "Alt+Enter");
        assert_eq!(parse("ctrl+shift+f5").unwrap().to_string(), "Ctrl+Shift+F5");
        assert_eq!(parse("alt+l").unwrap().to_string(), "Alt+L");
    }
}
