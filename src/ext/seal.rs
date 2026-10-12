//! The seal that makes an installed extension loadable only by the kuro that installed it: HMAC-SHA256 over the extension's files,
//! keyed with a random secret that lives in the user's state directory (mode 0600) and is created on first install.
//!
//! This is tamper-evidence and "only kuro's installer can make a loadable extension", not secrecy: the definitions are readable text,
//! and another program running as the same user could read the key. See docs/design/extensions.md.

use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::path::Path;

pub const KEY_LEN: usize = 32;

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// Compare without stopping at the first difference.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The bytes that are sealed: for every file, sorted by path, its path and the hash of its content (so renames and edits both change it).
pub fn canonical(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (path, content) in sorted {
        out.extend_from_slice(&(path.len() as u64).to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&sha256(content));
    }
    out
}

pub fn seal(key: &[u8], files: &[(String, Vec<u8>)]) -> String {
    hex(&hmac(key, &canonical(files)))
}

pub fn verify(key: &[u8], files: &[(String, Vec<u8>)], seal_hex: &str) -> bool {
    constant_time_eq(seal(key, files).as_bytes(), seal_hex.as_bytes())
}

/// Read the key, or `None` when there is none (nothing can be verified then).
pub fn read_key(path: &Path) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.read_to_end(&mut buf).ok()?;
    (buf.len() == KEY_LEN).then_some(buf)
}

/// Read the key, creating it (random, mode 0600, atomically) when it does not exist yet.
pub fn key_or_create(path: &Path) -> io::Result<Vec<u8>> {
    if let Some(k) = read_key(path) {
        return Ok(k);
    }
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut key = vec![0u8; KEY_LEN];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut key)?;
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // unique per call, not just per process: two threads may create the key at once
    let tmp = path.with_extension(format!("tmp{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(&key)?;
        f.sync_all()?;
    }
    match std::fs::hard_link(&tmp, path) {
        // somebody else created it first: use theirs
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&tmp);
            return read_key(path).ok_or_else(|| io::Error::other("the extension key is unreadable"));
        }
        other => other?,
    }
    let _ = std::fs::remove_file(&tmp);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_the_rfc_4231_vectors() {
        let k = [0x0bu8; 20];
        assert_eq!(hex(&hmac(&k, b"Hi There")), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // a key longer than the block size is hashed first
        let long = [0xaau8; 131];
        assert_eq!(
            hex(&hmac(&long, b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn the_seal_changes_with_content_path_and_key() {
        let files = vec![("a.toml".to_string(), b"x".to_vec()), ("b.txt".to_string(), b"y".to_vec())];
        let key = [7u8; 32];
        let s = seal(&key, &files);
        assert!(verify(&key, &files, &s));
        assert!(verify(&key, &[files[1].clone(), files[0].clone()], &s), "order does not matter");
        let mut edited = files.clone();
        edited[1].1 = b"z".to_vec();
        assert!(!verify(&key, &edited, &s), "edited content");
        let mut renamed = files.clone();
        renamed[1].0 = "c.txt".into();
        assert!(!verify(&key, &renamed, &s), "renamed file");
        assert!(!verify(&[8u8; 32], &files, &s), "another key");
        assert!(!verify(&key, &files[..1], &s), "removed file");
        assert!(!verify(&key, &files, "00"), "garbage seal");
    }

    #[test]
    fn the_key_is_created_once_private_and_stable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("state/ext.key");
        assert!(read_key(&p).is_none());
        let k1 = key_or_create(&p).unwrap();
        assert_eq!(k1.len(), KEY_LEN);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(key_or_create(&p).unwrap(), k1, "the same key every time");
        std::fs::write(&p, b"short").unwrap();
        assert!(read_key(&p).is_none(), "a damaged key is not accepted");
    }
}
