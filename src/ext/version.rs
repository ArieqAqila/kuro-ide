//! Just enough of semantic-version ranges for `kuro = ">=0.1, <0.2"` in a manifest: comma-separated comparators
//! (`>=`, `>`, `<=`, `<`, `=`, or a bare version meaning `=`) over `major[.minor[.patch]]`. Missing parts count as 0.

use std::cmp::Ordering;

fn parse_version(s: &str) -> Result<[u64; 3], String> {
    let core = s.trim().split(['-', '+', ' ']).next().unwrap_or("");
    if core.is_empty() {
        return Err(format!("`{s}` is not a version"));
    }
    let mut out = [0u64; 3];
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 {
        return Err(format!("`{s}` has too many parts (expected major[.minor[.patch]])"));
    }
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.parse().map_err(|_| format!("`{s}` is not a version (`{p}` is not a number)"))?;
    }
    Ok(out)
}

/// Does `version` satisfy the requirement `req`? An error describes what is wrong with the requirement text.
pub fn satisfies(req: &str, version: &str) -> Result<bool, String> {
    let v = parse_version(version)?;
    if req.trim().is_empty() {
        return Err("the kuro version requirement is empty".into());
    }
    for part in req.split(',') {
        let part = part.trim();
        let (op, rest) = ["<=", ">=", "<", ">", "="].iter().find_map(|op| part.strip_prefix(op).map(|r| (*op, r))).unwrap_or(("=", part));
        let want = parse_version(rest).map_err(|e| format!("in the kuro version requirement `{req}`: {e}"))?;
        let ord = v.cmp(&want);
        let ok = match op {
            ">=" => ord != Ordering::Less,
            ">" => ord == Ordering::Greater,
            "<=" => ord != Ordering::Greater,
            "<" => ord == Ordering::Less,
            // a bare "0.1" means any 0.1.x
            _ => {
                let given = rest.trim().split('.').count();
                v[..given.min(3)] == want[..given.min(3)]
            }
        };
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(satisfies(">=0.1, <0.2", "0.1.0"), Ok(true));
        assert_eq!(satisfies(">=0.1, <0.2", "0.1.9"), Ok(true));
        assert_eq!(satisfies(">=0.1, <0.2", "0.2.0"), Ok(false));
        assert_eq!(satisfies(">=0.2", "0.1.0"), Ok(false));
        assert_eq!(satisfies(">0.1.0", "0.1.0"), Ok(false));
        assert_eq!(satisfies("<=0.1.0", "0.1.0"), Ok(true));
        assert_eq!(satisfies("0.1", "0.1.7"), Ok(true), "a bare 0.1 is any 0.1.x");
        assert_eq!(satisfies("0.1.0", "0.1.7"), Ok(false));
        assert_eq!(satisfies(">=0.1.0-beta", "0.1.0"), Ok(true));
        assert_eq!(satisfies(">=1", "1.0.0"), Ok(true));
    }

    #[test]
    fn bad_requirements_say_what_is_wrong() {
        for bad in ["", ">=", ">=x.y", ">=1.2.3.4", "~~", ">=0.1,"] {
            assert!(satisfies(bad, "0.1.0").is_err(), "{bad:?}");
        }
        assert!(satisfies(">=0.1", "not-a-version").is_err());
        assert!(satisfies("^0.1", "0.1.0").is_err(), "carets are not supported (say >=0.1, <0.2)");
    }
}
