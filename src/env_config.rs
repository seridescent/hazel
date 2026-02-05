use std::collections::HashMap;
use std::env;
use std::path::Path;
use tracing::warn;

/// Environment configuration loaded from dotenv-format files.
/// Supports layering: base vars can be overridden by environment-specific vars.
#[derive(Debug, Default)]
pub struct EnvConfig {
    base: HashMap<String, String>,
    staging: HashMap<String, String>,
    production: HashMap<String, String>,
}

impl EnvConfig {
    /// Returns an iterator over staging environment variables.
    /// Layering: base vars overridden by staging-specific vars.
    pub fn staging_vars(&self) -> impl Iterator<Item = (&str, &str)> + Clone {
        self.base
            .iter()
            .chain(self.staging.iter())
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Returns an iterator over production environment variables.
    /// Layering: base vars overridden by production-specific vars.
    pub fn production_vars(&self) -> impl Iterator<Item = (&str, &str)> + Clone {
        self.base
            .iter()
            .chain(self.production.iter())
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Loads environment variables from a dotenv-format file.
/// Returns an empty HashMap if the file doesn't exist or can't be read.
fn load_env_file(path: &Path) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    match dotenvy::from_path_iter(path) {
        Ok(iter) => {
            for result in iter {
                match result {
                    Ok((key, value)) => {
                        vars.insert(key, value);
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to parse line in env file");
                    }
                }
            }
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to read env file");
        }
    }

    vars
}

/// Loads environment configuration from files specified by environment variables.
/// - `HAZEL_BASE_ENV_FILE` - Path to shared env file (optional)
/// - `HAZEL_STAGING_ENV_FILE` - Path to staging-specific env file (optional)
/// - `HAZEL_PRODUCTION_ENV_FILE` - Path to production-specific env file (optional)
pub fn load_env_config() -> EnvConfig {
    let base = env::var("HAZEL_BASE_ENV_FILE")
        .ok()
        .map(|p| load_env_file(Path::new(&p)))
        .unwrap_or_default();

    let staging = env::var("HAZEL_STAGING_ENV_FILE")
        .ok()
        .map(|p| load_env_file(Path::new(&p)))
        .unwrap_or_default();

    let production = env::var("HAZEL_PRODUCTION_ENV_FILE")
        .ok()
        .map(|p| load_env_file(Path::new(&p)))
        .unwrap_or_default();

    EnvConfig {
        base,
        staging,
        production,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp_env(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_load_env_file_basic() {
        let file = write_temp_env("FOO=bar\nBAZ=qux\n");
        let vars = load_env_file(file.path());
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_load_env_file_with_comments() {
        let file = write_temp_env("# this is a comment\nFOO=bar\n# another comment\n");
        let vars = load_env_file(file.path());
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.len(), 1);
    }

    #[test]
    fn test_load_env_file_with_quotes() {
        let file = write_temp_env("FOO=\"bar baz\"\nQUX='single quotes'\n");
        let vars = load_env_file(file.path());
        assert_eq!(vars.get("FOO"), Some(&"bar baz".to_string()));
        assert_eq!(vars.get("QUX"), Some(&"single quotes".to_string()));
    }

    #[test]
    fn test_load_env_file_empty_value() {
        let file = write_temp_env("FOO=\nBAR=value\n");
        let vars = load_env_file(file.path());
        assert_eq!(vars.get("FOO"), Some(&"".to_string()));
        assert_eq!(vars.get("BAR"), Some(&"value".to_string()));
    }

    #[test]
    fn test_load_env_file_missing() {
        let vars = load_env_file(Path::new("/nonexistent/path/to/file.env"));
        assert!(vars.is_empty());
    }

    #[test]
    fn test_staging_vars_layering() {
        let config = EnvConfig {
            base: [("SHARED".into(), "base".into()), ("BASE_ONLY".into(), "value".into())]
                .into_iter()
                .collect(),
            staging: [("SHARED".into(), "staging".into()), ("STAGING_ONLY".into(), "value".into())]
                .into_iter()
                .collect(),
            production: HashMap::new(),
        };

        let vars: HashMap<&str, &str> = config.staging_vars().collect();
        // Staging overrides base for SHARED
        assert_eq!(vars.get("SHARED"), Some(&"staging"));
        assert_eq!(vars.get("BASE_ONLY"), Some(&"value"));
        assert_eq!(vars.get("STAGING_ONLY"), Some(&"value"));
    }

    #[test]
    fn test_production_vars_layering() {
        let config = EnvConfig {
            base: [("SHARED".into(), "base".into())].into_iter().collect(),
            staging: HashMap::new(),
            production: [("SHARED".into(), "production".into())].into_iter().collect(),
        };

        let vars: HashMap<&str, &str> = config.production_vars().collect();
        assert_eq!(vars.get("SHARED"), Some(&"production"));
    }

    #[test]
    fn test_production_vars_base_only() {
        let config = EnvConfig {
            base: [("SHARED".into(), "base".into())].into_iter().collect(),
            staging: HashMap::new(),
            production: HashMap::new(),
        };

        let vars: HashMap<&str, &str> = config.production_vars().collect();
        assert_eq!(vars.get("SHARED"), Some(&"base"));
    }
}
