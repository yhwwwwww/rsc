use crate::{manifest::Manifest, util};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct Bucket {
    pub name: String,
    pub path: PathBuf,
    pub manifests: usize,
    pub remote: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Resolved {
    pub name: String,
    pub bucket: Option<String>,
    pub source: String,
    pub manifest: Manifest,
}
#[derive(Debug)]
pub struct Index {
    pub packages: Vec<Resolved>,
    pub warnings: Vec<String>,
}
#[derive(Debug)]
pub struct PackageSpec {
    pub name: String,
    pub bucket: Option<String>,
    pub version: Option<String>,
}
impl PackageSpec {
    pub fn parse(input: &str) -> Result<Self> {
        let (name, version) = input
            .rsplit_once('@')
            .map(|(n, v)| (n, Some(v.to_owned())))
            .unwrap_or((input, None));
        let (bucket, name) = name
            .split_once('/')
            .map(|(b, n)| (Some(b.to_owned()), n))
            .unwrap_or((None, name));
        util::valid_name(name)?;
        if let Some(b) = &bucket {
            util::valid_name(b)?;
        }
        if let Some(v) = &version {
            util::valid_component(v)?;
        }
        Ok(Self {
            name: name.to_owned(),
            bucket,
            version,
        })
    }
}

pub fn buckets(root: &Path) -> Result<Vec<Bucket>> {
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let manifests = manifest_paths(&path)?.len();
        let remote = remote(&path);
        result.push(Bucket {
            name,
            path,
            manifests,
            remote,
        });
    }
    let known: serde_json::Value = crate::native::known::json();
    let order = known
        .as_object()
        .context("Invalid known bucket list")?
        .keys()
        .collect::<Vec<_>>();
    result.sort_by(|a, b| {
        let rank = |s: &str| {
            order
                .iter()
                .position(|n| n.eq_ignore_ascii_case(s))
                .unwrap_or(usize::MAX)
        };
        rank(&a.name)
            .cmp(&rank(&b.name))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(result)
}
pub fn manifest_paths(bucket: &Path) -> Result<Vec<PathBuf>> {
    let inner = bucket.join("bucket");
    let directory = if inner.is_dir() {
        inner.as_path()
    } else {
        bucket
    };
    let mut files = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
                pending.push(entry.path());
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}
pub fn index(root: &Path) -> Result<Index> {
    let mut packages = Vec::new();
    let mut warnings = Vec::new();
    for bucket in buckets(root)? {
        for path in manifest_paths(&bucket.path)? {
            match Manifest::read(&path) {
                Ok(manifest) => packages.push(Resolved {
                    name: path
                        .file_stem()
                        .context("Manifest has no filename")?
                        .to_string_lossy()
                        .into_owned(),
                    bucket: Some(bucket.name.clone()),
                    source: path.display().to_string(),
                    manifest,
                }),
                Err(error) => warnings.push(format!("{}: {error:#}", path.display())),
            }
        }
    }
    Ok(Index { packages, warnings })
}
pub fn resolve(root: &Path, input: &str) -> Result<Resolved> {
    if Path::new(input).is_file() {
        let path = util::absolute(input)?;
        let name = path
            .file_stem()
            .context("Manifest has no filename")?
            .to_string_lossy()
            .into_owned();
        util::valid_name(&name)?;
        return Ok(Resolved {
            name,
            bucket: None,
            source: path.display().to_string(),
            manifest: Manifest::read(&path)?,
        });
    }
    let spec = PackageSpec::parse(input)?;
    let mut matches = Vec::new();
    for bucket in buckets(root)? {
        if spec
            .bucket
            .as_ref()
            .is_some_and(|b| !b.eq_ignore_ascii_case(&bucket.name))
        {
            continue;
        }
        for path in manifest_paths(&bucket.path)? {
            if path
                .file_stem()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&spec.name))
            {
                matches.push(Resolved {
                    name: path
                        .file_stem()
                        .context("Manifest has no filename")?
                        .to_string_lossy()
                        .into_owned(),
                    bucket: Some(bucket.name.clone()),
                    source: path.display().to_string(),
                    manifest: Manifest::read(&path)?,
                });
            }
        }
    }
    if matches.is_empty() {
        bail!("Package {input} was not found. Check rsc bucket or provide a manifest file / URL.");
    }
    if matches.len() > 1 {
        eprintln!(
            "warning: multiple buckets contain {input}; using {}/{}",
            matches[0].bucket.as_deref().unwrap_or(""),
            matches[0].name
        );
    }
    let package = matches.remove(0);
    if let Some(version) = spec.version {
        if package.manifest.version()? != version {
            bail!(
                "Requested {version}; local manifest is {}. Historical manifest lookup is not implemented yet.",
                package.manifest.version()?
            );
        }
    }
    Ok(package)
}
fn remote(path: &Path) -> Option<String> {
    let git = path.join(".git");
    let config = if git.is_file() {
        let text = util::read_text(&git).ok()?;
        let location = text.trim().strip_prefix("gitdir: ")?;
        path.join(location).join("config")
    } else {
        git.join("config")
    };
    let text = util::read_text(&config).ok()?;
    let mut origin = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            origin = line.eq_ignore_ascii_case("[remote \"origin\"]");
        } else if origin {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("url") {
                    return Some(util::redact_url(value.trim()));
                }
            }
        }
    }
    None
}
