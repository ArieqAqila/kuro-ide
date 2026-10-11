//! System clipboard access behind a trait so the editor can be tested with a fake.
//!
//! Backends: wl-clipboard (Wayland), xclip / xsel (X11), OSC 52 (write-only
//! escape sequence understood by most modern terminals; also works over SSH).

use base64::Engine;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub trait Clipboard {
    fn get(&mut self) -> Result<String, String>;
    fn set(&mut self, text: &str) -> Result<(), String>;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    WlClipboard,
    Xclip,
    Xsel,
    Osc52,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::WlClipboard => "wl-clipboard",
            Backend::Xclip => "xclip",
            Backend::Xsel => "xsel",
            Backend::Osc52 => "osc52",
        }
    }
}

/// Pure backend choice: `have(cmd)` says whether an executable exists.
pub fn choose_backend(setting: &str, wayland: bool, x11: bool, have: &dyn Fn(&str) -> bool) -> Backend {
    match setting {
        "wl-clipboard" => return Backend::WlClipboard,
        "xclip" => return Backend::Xclip,
        "xsel" => return Backend::Xsel,
        "osc52" => return Backend::Osc52,
        _ => {}
    }
    if wayland && have("wl-copy") && have("wl-paste") {
        Backend::WlClipboard
    } else if x11 && have("xclip") {
        Backend::Xclip
    } else if x11 && have("xsel") {
        Backend::Xsel
    } else {
        Backend::Osc52
    }
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

pub fn detect(setting: &str) -> Box<dyn Clipboard> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    match choose_backend(setting, wayland, x11, &which) {
        Backend::Osc52 => Box::new(Osc52Clipboard::default()),
        b => Box::new(CommandClipboard { backend: b }),
    }
}

pub struct CommandClipboard {
    pub backend: Backend,
}

impl CommandClipboard {
    fn copy_cmd(&self) -> (&'static str, &'static [&'static str]) {
        match self.backend {
            Backend::WlClipboard => ("wl-copy", &[]),
            Backend::Xclip => ("xclip", &["-selection", "clipboard", "-in"]),
            _ => ("xsel", &["--clipboard", "--input"]),
        }
    }
    fn paste_cmd(&self) -> (&'static str, &'static [&'static str]) {
        match self.backend {
            Backend::WlClipboard => ("wl-paste", &["--no-newline"]),
            Backend::Xclip => ("xclip", &["-selection", "clipboard", "-out"]),
            _ => ("xsel", &["--clipboard", "--output"]),
        }
    }
}

impl Clipboard for CommandClipboard {
    fn set(&mut self, text: &str) -> Result<(), String> {
        let (cmd, args) = self.copy_cmd();
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{cmd}: {e}"))?;
        child.stdin.take().unwrap().write_all(text.as_bytes()).map_err(|e| format!("{cmd}: {e}"))?;
        let st = child.wait().map_err(|e| format!("{cmd}: {e}"))?;
        if st.success() { Ok(()) } else { Err(format!("{cmd} exited with {st}")) }
    }

    fn get(&mut self) -> Result<String, String> {
        let (cmd, args) = self.paste_cmd();
        let out = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("{cmd}: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            // wl-paste fails with a non-zero status on an empty clipboard.
            Ok(String::new())
        }
    }

    fn name(&self) -> &'static str {
        self.backend.label()
    }
}

/// OSC 52: terminals accept copies but generally refuse reads, so paste falls
/// back to the last text this process copied.
#[derive(Default)]
pub struct Osc52Clipboard {
    last: String,
}

pub fn osc52_sequence(text: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\x1b]52;c;{b64}\x07")
}

impl Clipboard for Osc52Clipboard {
    fn set(&mut self, text: &str) -> Result<(), String> {
        let mut out = std::io::stdout();
        out.write_all(osc52_sequence(text).as_bytes()).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        self.last = text.to_string();
        Ok(())
    }
    fn get(&mut self) -> Result<String, String> {
        Ok(self.last.clone())
    }
    fn name(&self) -> &'static str {
        "osc52"
    }
}

/// In-memory clipboard for tests; clones share the same storage.
#[derive(Clone, Default)]
pub struct MemoryClipboard(pub Arc<Mutex<String>>);

impl Clipboard for MemoryClipboard {
    fn set(&mut self, text: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = text.to_string();
        Ok(())
    }
    fn get(&mut self) -> Result<String, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn name(&self) -> &'static str {
        "memory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_preferred_when_tools_exist() {
        let have = |c: &str| ["wl-copy", "wl-paste", "xclip"].contains(&c);
        assert_eq!(choose_backend("auto", true, true, &have), Backend::WlClipboard);
    }

    #[test]
    fn x11_fallbacks() {
        assert_eq!(choose_backend("auto", false, true, &|c| c == "xclip"), Backend::Xclip);
        assert_eq!(choose_backend("auto", false, true, &|c| c == "xsel"), Backend::Xsel);
    }

    #[test]
    fn wayland_without_tools_falls_back_to_osc52() {
        assert_eq!(choose_backend("auto", true, false, &|_| false), Backend::Osc52);
        assert_eq!(choose_backend("auto", false, false, &|_| true), Backend::Osc52);
    }

    #[test]
    fn explicit_setting_wins() {
        assert_eq!(choose_backend("xsel", true, true, &|_| false), Backend::Xsel);
        assert_eq!(choose_backend("osc52", true, true, &|_| true), Backend::Osc52);
    }

    #[test]
    fn osc52_encoding() {
        assert_eq!(osc52_sequence("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn memory_clipboard_roundtrip_with_buffer() {
        let mut clip = MemoryClipboard::default();
        let mut buf = crate::buffer::Buffer::from_str("hello world");
        buf.anchor = Some(0);
        buf.cursor = 5;
        // copy
        clip.set(&buf.selected_text().unwrap()).unwrap();
        // cut
        let cut = buf.delete_selection().unwrap();
        clip.set(&cut).unwrap();
        assert_eq!(buf.text(), " world");
        // paste at end
        buf.cursor = buf.len_chars();
        let t = clip.get().unwrap();
        buf.insert_at_cursor(&t);
        assert_eq!(buf.text(), " worldhello");
    }

    #[test]
    fn memory_clipboard_unicode_and_multiline() {
        let mut clip = MemoryClipboard::default();
        clip.set("é😀\nline2").unwrap();
        assert_eq!(clip.get().unwrap(), "é😀\nline2");
    }
}
