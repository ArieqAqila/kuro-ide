//! In-memory password handling (design: memory only, never on disk, never logged, never hashed).
//!
//! A password the user types for a `password = "prompt"` profile is kept for the rest of the session so that
//! reconnecting to the *same* profile does not ask again. It lives only in this process: `Secret` zeroes its
//! memory when dropped, has no `Debug`/`Display`, and the cache is dropped (and so wiped) when kuro exits.
//!
//! Limits, stated plainly: the PostgreSQL and MariaDB client libraries keep their own copy of the password for
//! the life of a connection, which kuro cannot wipe; and a process with the same user id can still read this
//! process's memory unless the kernel forbids it, which `protect_process_memory` requests as soon as a secret is stored.

use std::collections::HashMap;
use std::sync::Arc;
use zeroize::Zeroizing;

pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(s: String) -> Secret {
        Secret(Zeroizing::new(s))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Which connection a password belongs to: the profile name and every field that identifies the server and account.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub profile: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: Option<String>,
}

#[derive(Default)]
pub struct PasswordCache {
    map: HashMap<CacheKey, Secret>,
    /// Dropped together with the cache; tests hold a `Weak` to it to see that the cache is gone after exit.
    alive: Arc<()>,
}

impl std::fmt::Debug for PasswordCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PasswordCache({} entries)", self.map.len())
    }
}

impl PasswordCache {
    pub fn get(&self, k: &CacheKey) -> Option<&Secret> {
        self.map.get(k)
    }

    pub fn insert(&mut self, k: CacheKey, s: Secret) {
        protect_process_memory();
        self.map.insert(k, s);
    }

    pub fn remove(&mut self, k: &CacheKey) -> bool {
        self.map.remove(k).is_some()
    }

    /// Forget every password, or only those of one profile.
    pub fn forget(&mut self, profile: Option<&str>) -> usize {
        let before = self.map.len();
        match profile {
            Some(p) => self.map.retain(|k, _| k.profile != p),
            None => self.map.clear(),
        }
        before - self.map.len()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// A token that stops upgrading once the cache has been dropped (used by tests).
    pub fn alive_token(&self) -> std::sync::Weak<()> {
        Arc::downgrade(&self.alive)
    }
}

/// Ask the kernel not to dump this process's memory or let other processes attach to it (Linux).
/// Called when the first secret is stored, so a process that never holds a password is unaffected.
pub fn protect_process_memory() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: prctl(PR_SET_DUMPABLE, 0) takes only integer arguments and has no memory-safety preconditions.
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(p: &str) -> CacheKey {
        CacheKey { profile: p.into(), host: "h".into(), port: 1, user: "u".into(), database: None }
    }

    #[test]
    fn debug_never_shows_the_password() {
        let s = Secret::new("hunter2-unique".into());
        assert!(!format!("{s:?}").contains("hunter2"));
        let mut c = PasswordCache::default();
        c.insert(key("a"), s);
        assert!(!format!("{c:?}").contains("hunter2"));
        assert_eq!(format!("{c:?}"), "PasswordCache(1 entries)");
    }

    #[test]
    fn entries_are_per_connection_identity() {
        let mut c = PasswordCache::default();
        c.insert(key("a"), Secret::new("one".into()));
        assert_eq!(c.get(&key("a")).unwrap().expose(), "one");
        assert!(c.get(&key("b")).is_none(), "another profile does not see it");
        let mut other_host = key("a");
        other_host.host = "elsewhere".into();
        assert!(c.get(&other_host).is_none(), "the same profile name pointing at a different server does not either");
    }

    #[test]
    fn forget_removes_one_profile_or_everything() {
        let mut c = PasswordCache::default();
        c.insert(key("a"), Secret::new("1".into()));
        c.insert(key("b"), Secret::new("2".into()));
        assert_eq!(c.forget(Some("a")), 1);
        assert_eq!(c.len(), 1);
        assert_eq!(c.forget(None), 1);
        assert!(c.is_empty());
    }

    #[test]
    fn dropping_the_cache_is_observable() {
        let c = PasswordCache::default();
        let token = c.alive_token();
        assert!(token.upgrade().is_some());
        drop(c);
        assert!(token.upgrade().is_none());
    }

    #[test]
    fn zeroizing_wipes_the_buffer() {
        // what Zeroizing guarantees, checked on a buffer we still own
        use zeroize::Zeroize;
        let mut s = String::from("super-secret");
        let ptr = s.as_ptr();
        let len = s.len();
        s.zeroize();
        assert!(s.is_empty());
        // SAFETY: the allocation is still owned by `s`; we only read `len` bytes it had.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        assert!(bytes.iter().all(|b| *b == 0), "the old contents are overwritten with zeros");
    }
}
