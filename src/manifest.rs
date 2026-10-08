use crate::util::{self, ci_get};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, path::Path};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Architecture {
    #[serde(rename = "64bit")]
    X64,
    #[serde(rename = "32bit")]
    X86,
    #[serde(rename = "arm64")]
    Arm64,
}
impl Architecture {
    pub fn native() -> Self {
        let arch = std::env::var("PROCESSOR_ARCHITEW6432")
            .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
            .unwrap_or_default();
        if std::env::var_os("ProgramFiles(Arm)").is_some()
            || arch.eq_ignore_ascii_case("ARM64")
            || cfg!(target_arch = "aarch64")
        {
            Self::Arm64
        } else if arch.eq_ignore_ascii_case("AMD64") || cfg!(target_arch = "x86_64") {
            Self::X64
        } else {
            Self::X86
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "64bit" | "64" | "x64" | "amd64" | "x86_64" | "x86-64" => Ok(Self::X64),
            "32bit" | "32" | "x86" | "i386" | "386" | "i686" => Ok(Self::X86),
            "arm64" | "arm" | "aarch64" => Ok(Self::Arm64),
            _ => bail!("Unsupported architecture: {s}"),
        }
    }
}
impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::X64 => "64bit",
            Self::X86 => "32bit",
            Self::Arm64 => "arm64",
        })
    }
}
#[derive(Clone, Debug)]
pub struct Manifest {
    pub raw: Value,
    pub text: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct DownloadFile {
    pub url: String,
    pub hash: Option<String>,
}
impl Manifest {
    pub fn read(path: &Path) -> Result<Self> {
        Self::parse(util::read_text(path)?)
    }
    pub fn parse(text: String) -> Result<Self> {
        let raw: Value = serde_json::from_str(&text).context("Invalid manifest JSON")?;
        if !raw.is_object() {
            bail!("Manifest must be a JSON object");
        }
        let manifest = Self { raw, text };
        manifest.version()?;
        Ok(manifest)
    }
    pub fn version(&self) -> Result<&str> {
        ci_get(&self.raw, "version")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .context("Manifest is missing a string version")
    }
    pub fn description(&self) -> &str {
        ci_get(&self.raw, "description")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    pub fn field(&self, key: &str, arch: Architecture) -> Option<&Value> {
        ci_get(&self.raw, "architecture")
            .and_then(|a| ci_get(a, &arch.to_string()))
            .and_then(|a| ci_get(a, key))
            .filter(|v| truthy(v))
            .or_else(|| ci_get(&self.raw, key).filter(|v| truthy(v)))
    }
    pub fn strings(&self, key: &str, arch: Architecture) -> Result<Vec<String>> {
        match self.field(key, arch) {
            None => Ok(Vec::new()),
            Some(Value::String(s)) => Ok(vec![s.clone()]),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .with_context(|| format!("Manifest field {key} must contain strings"))
                })
                .collect(),
            _ => bail!("Manifest field {key} must be a string or string array"),
        }
    }
    pub fn downloads(&self, arch: Architecture) -> Result<Vec<DownloadFile>> {
        let urls = self.strings("url", arch)?;
        let hashes = self.strings("hash", arch)?;
        if !hashes.is_empty() && hashes.len() != urls.len() {
            bail!(
                "Manifest has {} URLs but {} hashes for {arch}",
                urls.len(),
                hashes.len()
            );
        }
        urls.into_iter()
            .enumerate()
            .map(|(i, url)| {
                let parsed = url::Url::parse(&url).context("Invalid download URL")?;
                if !["http", "https", "ftp", "file"].contains(&parsed.scheme()) {
                    bail!("Only HTTP, HTTPS, FTP and local file URLs are supported");
                }
                Ok(DownloadFile {
                    url,
                    hash: hashes
                        .get(i)
                        .cloned()
                        .filter(|h| !h.eq_ignore_ascii_case("skip")),
                })
            })
            .collect()
    }
    pub fn bins(&self, arch: Architecture) -> Vec<String> {
        let Some(value) = self.field("bin", arch) else {
            return Vec::new();
        };
        let items = match value {
            Value::Array(a) => a.clone(),
            v => vec![v.clone()],
        };
        items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.as_str()),
                Value::Array(a) => a.get(1).or_else(|| a.first()).and_then(Value::as_str),
                _ => None,
            })
            .map(|s| {
                Path::new(s)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(s)
                    .to_owned()
            })
            .collect()
    }
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.0),
        _ => true,
    }
}
