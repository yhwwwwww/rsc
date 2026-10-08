use crate::{
    layout::Layout,
    util::{self, FileLock, object_get},
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Config {
    pub scoop_file: PathBuf,
    pub rsc_file: PathBuf,
    scoop: Map<String, Value>,
    rsc: Map<String, Value>,
    pub layout: Layout,
}
#[derive(Serialize)]
pub struct Setting {
    pub key: String,
    pub value: Value,
    pub source: String,
}
impl Config {
    pub fn load() -> Result<Self> {
        let profile = env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .map(PathBuf::from)
            .context("USERPROFILE is not set; cannot locate Scoop configuration")?;
        let config_home = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| profile.join(".config"));
        let portable = portable_root();
        let scoop_file = portable
            .as_ref()
            .map(|p| p.join("config.json"))
            .unwrap_or_else(|| config_home.join("scoop").join("config.json"));
        let rsc_file = config_home.join("rsc").join("config.json");
        let scoop = load_object(&scoop_file)?;
        let rsc = load_object(&rsc_file)?;
        let root = resolve_path(
            "SCOOP",
            &scoop,
            "root_path",
            portable.unwrap_or_else(|| profile.join("scoop")),
        )?;
        let global_default = env_path("ProgramData")
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("scoop");
        let global_root = resolve_path("SCOOP_GLOBAL", &scoop, "global_path", global_default)?;
        let cache = resolve_path("SCOOP_CACHE", &scoop, "cache_path", root.join("cache"))?;
        let path_variable = match object_get(&scoop, "use_isolated_path") {
            Some(Value::String(s)) if !s.is_empty() => s.to_uppercase(),
            Some(Value::Bool(true)) => "SCOOP_PATH".into(),
            _ => "PATH".into(),
        };
        let no_junction = object_get(&scoop, "no_junction")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(Self {
            scoop_file,
            rsc_file,
            scoop,
            rsc,
            layout: Layout {
                root,
                global_root,
                cache,
                path_variable,
                no_junction,
            },
        })
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        object_get(if is_rsc(key) { &self.rsc } else { &self.scoop }, key)
    }
    pub fn text(&self, key: &str) -> Result<Option<String>> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            _ => bail!("Configuration {key} must be a string"),
        }
    }
    pub fn number(&self, key: &str, default: u64) -> Result<u64> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(v) => v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .with_context(|| format!("Configuration {key} must be a positive integer")),
        }
    }
    pub fn settings(&self) -> Vec<Setting> {
        let mut settings: Vec<Setting> = self
            .scoop
            .iter()
            .map(|(k, v)| Setting {
                key: k.clone(),
                value: redact(k, v),
                source: self.scoop_file.display().to_string(),
            })
            .chain(self.rsc.iter().map(|(k, v)| Setting {
                key: k.clone(),
                value: redact(k, v),
                source: self.rsc_file.display().to_string(),
            }))
            .collect();
        for (key, default) in [
            ("download.threads", json!(4)),
            ("download.concurrent", json!(4)),
            ("download.per_host", json!(8)),
            ("download.split_size", json!(4194304)),
            ("download.retries", json!(3)),
            ("download.timeout", json!(30)),
        ] {
            if !settings.iter().any(|s| s.key.eq_ignore_ascii_case(key)) {
                settings.push(Setting {
                    key: key.into(),
                    value: default,
                    source: "default".into(),
                });
            }
        }
        for (key, variable, path) in [
            ("root_path", "SCOOP", &self.layout.root),
            ("global_path", "SCOOP_GLOBAL", &self.layout.global_root),
            ("cache_path", "SCOOP_CACHE", &self.layout.cache),
        ] {
            let source = if env_path(variable).is_some() {
                format!("environment: {variable}")
            } else if object_get(&self.scoop, key).is_some() {
                self.scoop_file.display().to_string()
            } else {
                "default / detected layout".into()
            };
            settings.retain(|s| !s.key.eq_ignore_ascii_case(key));
            settings.push(Setting {
                key: key.into(),
                value: json!(path),
                source,
            });
        }
        settings.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));
        settings
    }
    pub fn set(&self, key: &str, input: Option<&str>) -> Result<PathBuf> {
        if key.trim().is_empty() {
            bail!("A configuration key is required");
        }
        let file = if is_rsc(key) {
            &self.rsc_file
        } else {
            &self.scoop_file
        };
        let actual_file = if file.try_exists()? {
            std::fs::canonicalize(file)?
        } else {
            file.to_owned()
        };
        let file = &actual_file;
        let lock_path = file.with_extension("json.lock");
        let _lock = FileLock::acquire(&lock_path)?;
        let mut object = load_object(file)?;
        let canonical = object
            .keys()
            .find(|k| k.eq_ignore_ascii_case(key))
            .cloned()
            .unwrap_or_else(|| key.to_lowercase());
        if let Some(input) = input {
            let value = if input.eq_ignore_ascii_case("true") {
                json!(true)
            } else if input.eq_ignore_ascii_case("false") {
                json!(false)
            } else {
                serde_json::from_str(input).unwrap_or_else(|_| json!(input))
            };
            if is_rsc(key) {
                let allowed = [
                    "download.threads",
                    "download.concurrent",
                    "download.per_host",
                    "download.split_size",
                    "download.retries",
                    "download.timeout",
                ];
                if !allowed.iter().any(|k| k.eq_ignore_ascii_case(key)) {
                    bail!("Unknown rsc setting: {key}");
                }
                let n = value
                    .as_u64()
                    .context("Download settings require nonnegative integers")?;
                let limit = match key.to_lowercase().as_str() {
                    "download.threads" | "download.concurrent" | "download.per_host" => 64,
                    "download.retries" => 10,
                    "download.timeout" => 3600,
                    _ => u64::MAX,
                };
                if n > limit || (n == 0 && !key.eq_ignore_ascii_case("download.retries")) {
                    bail!(
                        "Invalid value for {key}; allowed range is {}..={limit}",
                        if key.eq_ignore_ascii_case("download.retries") {
                            0
                        } else {
                            1
                        }
                    );
                }
            }
            if ["root_path", "global_path", "cache_path"]
                .iter()
                .any(|k| k.eq_ignore_ascii_case(key))
                && !value.as_str().is_some_and(|s| !s.is_empty())
            {
                bail!("{key} must be a nonempty path string");
            }
            object.insert(canonical, value);
        } else {
            object.remove(&canonical);
        }
        if key.eq_ignore_ascii_case("use_isolated_path") {
            crate::native::invoke(
                self,
                "config",
                json!({"key":key,"value":input.unwrap_or("")}),
            )?;
        }
        if key.eq_ignore_ascii_case("use_sqlite_cache")
            && input.is_some_and(|s| s.eq_ignore_ascii_case("true"))
        {
            crate::database::refresh(self)?;
        }
        util::write_json(file, &object)?;
        Ok(file.clone())
    }
}

fn is_rsc(key: &str) -> bool {
    key.to_ascii_lowercase().starts_with("download.")
}
fn load_object(path: &Path) -> Result<Map<String, Value>> {
    if !path.try_exists()? {
        return Ok(Map::new());
    }
    let value = util::read_json(path)?;
    if value.is_null() {
        return Ok(Map::new());
    }
    value
        .as_object()
        .cloned()
        .with_context(|| format!("Configuration must be a JSON object: {}", path.display()))
}
fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
fn resolve_path(
    var: &str,
    cfg: &Map<String, Value>,
    key: &str,
    fallback: PathBuf,
) -> Result<PathBuf> {
    if let Some(path) = env_path(var) {
        return util::absolute(path);
    }
    match object_get(cfg, key) {
        Some(Value::String(s)) if !s.is_empty() => util::absolute(s),
        None | Some(Value::Null) => util::absolute(fallback),
        _ => bail!("Configuration {key} must be a nonempty path string"),
    }
}
fn portable_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(p) = env_path("SCOOP") {
        candidates.push(p);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            for ancestor in parent.ancestors().take(5) {
                if ancestor == parent
                    || parent == ancestor.join("shims")
                    || parent.starts_with(ancestor.join("apps").join("rsc"))
                {
                    candidates.push(ancestor.to_owned());
                }
            }
        }
    }
    candidates.into_iter().find(|p| {
        p.join("config.json").is_file() && p.join("apps").is_dir() && p.join("buckets").is_dir()
    })
}
fn redact(key: &str, value: &Value) -> Value {
    let lower = key.to_ascii_lowercase();
    if ["token", "password", "secret", "credential"]
        .iter()
        .any(|s| lower.contains(s))
    {
        return json!("***");
    }
    match value {
        Value::String(s) if lower == "proxy" && s.contains('@') => json!(format!(
            "***@{}",
            s.rsplit_once('@').map(|(_, host)| host).unwrap_or("***")
        )),
        Value::Array(a) => Value::Array(a.iter().map(|v| redact(key, v)).collect()),
        Value::String(s) if s.contains("://") => json!(util::redact_url(s)),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), redact(k, v))).collect())
        }
        _ => value.clone(),
    }
}
