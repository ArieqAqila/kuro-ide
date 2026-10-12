//! The optional extension system (see docs/EXTENSIONS.md and docs/design/extensions.md).
//!
//! An extension is **data only**: language definitions in a TOML manifest. kuro never executes anything from an extension. Installed
//! extensions are *sealed* (HMAC with a per-user key), so only extensions installed by kuro itself load; there is deliberately no way
//! to load one from a path.

pub mod lang;
pub mod manifest;
pub mod names;
pub mod seal;
pub mod store;
pub mod version;

/// Turn a file-name glob (`*`, `?`, `[a-z]`) into an anchored regular expression.
pub fn glob_to_regex(glob: &str) -> String {
    let mut out = String::from("^");
    let mut in_class = false;
    for c in glob.chars() {
        match c {
            '*' if !in_class => out.push_str(".*"),
            '?' if !in_class => out.push('.'),
            '[' if !in_class => {
                in_class = true;
                out.push('[');
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            c if in_class && (c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) => out.push(c),
            c if in_class => out.push_str(&format!("\\x{{{:x}}}", c as u32)),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    if in_class {
        out.push(']');
    }
    out.push('$');
    out
}
