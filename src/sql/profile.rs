//! Connection profiles. Secrets never live in the repo: a profile may only
//! reference `env:NAME` or `prompt` for the password, literal passwords are rejected.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Postgres,
    MariaDb,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PortSpec {
    Num(u16),
    Str(String),
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct Profile {
    pub driver: String,
    pub host: Option<String>,
    pub port: Option<PortSpec>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ProfileFile {
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

#[derive(Clone, PartialEq)]
pub struct Resolved {
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: Option<String>,
}

// never print the password
impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved")
            .field("engine", &self.engine)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("database", &self.database)
            .finish()
    }
}

pub fn parse_profiles(text: &str) -> Result<BTreeMap<String, Profile>, String> {
    toml::from_str::<ProfileFile>(text).map(|f| f.profiles).map_err(|e| e.message().to_string())
}

pub fn load_profiles(path: &Path) -> Result<BTreeMap<String, Profile>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => parse_profiles(&t).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "no connection profiles at {} (copy config/default/connections.example.toml there)",
            path.display()
        )),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn expand(field: &str, v: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    match v.strip_prefix("env:") {
        Some(name) => env(name).ok_or_else(|| format!("{field}: environment variable {name} is not set")),
        None => Ok(v.to_string()),
    }
}

impl Profile {
    pub fn engine(&self) -> Result<Engine, String> {
        match self.driver.as_str() {
            "postgres" => Ok(Engine::Postgres),
            "mariadb" => Ok(Engine::MariaDb),
            other => Err(format!("unsupported driver {other:?} (use \"postgres\" or \"mariadb\")")),
        }
    }

    pub fn needs_password_prompt(&self) -> bool {
        self.password.as_deref() == Some("prompt")
    }

    pub fn resolve(&self, env: &dyn Fn(&str) -> Option<String>, prompted: Option<String>) -> Result<Resolved, String> {
        let engine = self.engine()?;
        let host = match &self.host {
            Some(h) => expand("host", h, env)?,
            None => "127.0.0.1".into(),
        };
        let port = match &self.port {
            Some(PortSpec::Num(n)) => *n,
            Some(PortSpec::Str(s)) => expand("port", s, env)?.parse::<u16>().map_err(|_| format!("port: {s:?} is not a valid port"))?,
            None => match engine {
                Engine::Postgres => 5432,
                Engine::MariaDb => 3306,
            },
        };
        let user = match &self.user {
            Some(u) => expand("user", u, env)?,
            None => return Err("user is required".into()),
        };
        let password = match self.password.as_deref() {
            None => None,
            Some("prompt") => Some(prompted.ok_or("password required")?),
            Some(p) if p.starts_with("env:") => Some(expand("password", p, env)?),
            Some(_) => {
                return Err("password: literal passwords are not allowed in profiles; use \"env:VARNAME\" or \"prompt\"".into());
            }
        };
        let database = match &self.database {
            Some(d) => Some(expand("database", d, env)?),
            None => None,
        };
        Ok(Resolved { engine, host, port, user, password, database })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    const FILE: &str = r#"
[profiles.pg]
driver = "postgres"
host = "env:PGHOST"
user = "env:PGUSER"
password = "env:PGPASSWORD"
database = "app"

[profiles.maria]
driver = "mariadb"
port = 3307
user = "root"
password = "prompt"
"#;

    #[test]
    fn parses_profiles() {
        let p = parse_profiles(FILE).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p["maria"].port, Some(PortSpec::Num(3307)));
        assert!(p["maria"].needs_password_prompt());
        assert!(!p["pg"].needs_password_prompt());
    }

    #[test]
    fn env_references_are_resolved() {
        let p = parse_profiles(FILE).unwrap();
        let r = p["pg"].resolve(&env_of(&[("PGHOST", "db.local"), ("PGUSER", "me"), ("PGPASSWORD", "s3cret")]), None).unwrap();
        assert_eq!((r.host.as_str(), r.user.as_str(), r.port), ("db.local", "me", 5432));
        assert_eq!(r.password.as_deref(), Some("s3cret"));
        assert_eq!(r.database.as_deref(), Some("app"));
    }

    #[test]
    fn missing_env_is_a_clear_error() {
        let p = parse_profiles(FILE).unwrap();
        let e = p["pg"].resolve(&env_of(&[]), None).unwrap_err();
        assert!(e.contains("PGHOST"), "{e}");
    }

    #[test]
    fn prompt_password_comes_from_the_prompt() {
        let p = parse_profiles(FILE).unwrap();
        assert!(p["maria"].resolve(&env_of(&[]), None).is_err());
        let r = p["maria"].resolve(&env_of(&[]), Some("typed".into())).unwrap();
        assert_eq!(r.password.as_deref(), Some("typed"));
        assert_eq!(r.port, 3307);
    }

    #[test]
    fn literal_passwords_are_rejected() {
        let p = parse_profiles("[profiles.x]\ndriver=\"postgres\"\nuser=\"u\"\npassword=\"hunter2\"\n").unwrap();
        let e = p["x"].resolve(&env_of(&[]), None).unwrap_err();
        assert!(e.contains("literal passwords"), "{e}");
        assert!(!e.contains("hunter2"), "must not echo the secret");
    }

    #[test]
    fn only_mariadb_and_postgres_are_supported() {
        let p = parse_profiles("[profiles.x]\ndriver=\"sqlite\"\nuser=\"u\"\n").unwrap();
        assert!(p["x"].resolve(&env_of(&[]), None).unwrap_err().contains("unsupported driver"));
    }

    #[test]
    fn defaults_and_port_from_env() {
        let p = parse_profiles("[profiles.x]\ndriver=\"mariadb\"\nuser=\"u\"\nport=\"env:P\"\n").unwrap();
        let r = p["x"].resolve(&env_of(&[("P", "3999")]), None).unwrap();
        assert_eq!((r.host.as_str(), r.port, r.password), ("127.0.0.1", 3999, None));
        let p = parse_profiles("[profiles.y]\ndriver=\"postgres\"\n").unwrap();
        assert!(p["y"].resolve(&env_of(&[]), None).unwrap_err().contains("user"));
    }

    #[test]
    fn debug_output_hides_password() {
        let p = parse_profiles(FILE).unwrap();
        let r = p["maria"].resolve(&env_of(&[]), Some("topsecret".into())).unwrap();
        assert!(!format!("{r:?}").contains("topsecret"));
    }

    #[test]
    fn example_file_parses_and_has_no_literal_secrets() {
        let text = include_str!("../../config/default/connections.example.toml");
        let p = parse_profiles(text).unwrap();
        assert!(!p.is_empty());
        for (name, prof) in &p {
            if let Some(pw) = &prof.password {
                assert!(pw == "prompt" || pw.starts_with("env:"), "profile {name} has a literal password");
            }
        }
    }

    #[test]
    fn bad_toml_is_an_error_string() {
        assert!(parse_profiles("[profiles.x\n").is_err());
    }
}
