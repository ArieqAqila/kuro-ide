//! Scan terminal output for colour-rule violations (truecolor, 256-colour, background colours).
#![allow(dead_code)]

/// Every SGR parameter list (`ESC [ ... m`) in `bytes`.
pub fn sgr_lists(bytes: &[u8]) -> Vec<Vec<u32>> {
    let mut out = vec![];
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == 0x1b && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';' || bytes[j] == b':') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'm' {
                let body = std::str::from_utf8(&bytes[i + 2..j]).unwrap();
                let params: Vec<u32> =
                    if body.is_empty() { vec![0] } else { body.split([';', ':']).map(|p| p.parse().unwrap_or(0)).collect() };
                out.push(params);
            }
            i = j;
        }
        i += 1;
    }
    out
}

/// Violations of the colour rule found in `bytes`.
pub fn violations(bytes: &[u8]) -> Vec<String> {
    let mut v = vec![];
    for p in sgr_lists(bytes) {
        let mut i = 0;
        while i < p.len() {
            match p[i] {
                38 | 48 | 58 => {
                    v.push(format!("extended colour (truecolor/256) in {p:?}"));
                    break; // the rest are colour operands
                }
                40..=47 | 100..=107 => v.push(format!("background colour {} in {p:?}", p[i])),
                _ => {}
            }
            i += 1;
        }
    }
    v
}
