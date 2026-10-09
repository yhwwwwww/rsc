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
    let mut result = inventory(root)?;
    for b in &mut result {
        b.manifests = manifest_paths(&b.path)?.len();
        b.remote = remote(&b.path);
    }
    Ok(result)
}
/// Bucket order without opening every manifest or Git configuration.
pub(crate) fn inventory(root: &Path) -> Result<Vec<Bucket>> {
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() && !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let manifests = 0;
        let remote = None;
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
    collect(root, |_| Ok(true))
}
/// Read each manifest once, concurrently, and retain only actual search matches.
pub fn search(root: &Path, query: &str) -> Result<Index> {
    let re = crate::native::query::matcher(query)?;
    collect(root, |p| {
        if crate::native::query::matches(&re, &p.name)? {
            return Ok(true);
        }
        for a in [
            crate::manifest::Architecture::X64,
            crate::manifest::Architecture::X86,
            crate::manifest::Architecture::Arm64,
        ] {
            for bin in p.manifest.bins(a) {
                if crate::native::query::matches(&re, &bin)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    })
}
fn collect(root: &Path, filter: impl Fn(&Resolved) -> Result<bool> + Sync) -> Result<Index> {
    let mut paths = Vec::new();
    for bucket in inventory(root)? {
        for path in manifest_paths(&bucket.path)? {
            paths.push((bucket.name.clone(), path));
        }
    }
    let results = util::parallel_map(
        &paths,
        |(bucket, path)| -> Result<(Option<Resolved>, Option<String>)> {
            let manifest = match Manifest::read(path) {
                Ok(m) => m,
                Err(e) => return Ok((None, Some(format!("{}: {e:#}", path.display())))),
            };
            let p = Resolved {
                name: path
                    .file_stem()
                    .context("Manifest has no filename")?
                    .to_string_lossy()
                    .into_owned(),
                bucket: Some(bucket.clone()),
                source: path.display().to_string(),
                manifest,
            };
            if filter(&p)? {
                Ok((Some(p), None))
            } else {
                Ok((None, None))
            }
        },
    );
    let mut packages = Vec::new();
    let mut warnings = Vec::new();
    for result in results {
        let (package, warning) = result?;
        if let Some(p) = package {
            packages.push(p);
        }
        if let Some(w) = warning {
            warnings.push(w);
        }
    }
    Ok(Index { packages, warnings })
}
/// One directory catalogue per command, reused for every installed package.
pub(crate) struct Resolver {
    files: std::collections::HashMap<String, Vec<(String, PathBuf)>>,
}
impl Resolver {
    pub(crate) fn new(root: &Path) -> Result<Self> {
        let mut files = std::collections::HashMap::<String, Vec<(String, PathBuf)>>::new();
        for b in inventory(root)? {
            for path in manifest_paths(&b.path)? {
                if let Some(name) = path.file_stem() {
                    files
                        .entry(name.to_string_lossy().to_ascii_lowercase())
                        .or_default()
                        .push((b.name.clone(), path));
                }
            }
        }
        Ok(Self { files })
    }
    pub(crate) fn resolve(&self, input: &str) -> Result<Resolved> {
        let (package, warning) = self.resolve_report(input)?;
        if let Some(warning) = warning {
            crate::presentation::warning(&warning);
        }
        Ok(package)
    }
    pub(crate) fn resolve_report(&self, input: &str) -> Result<(Resolved, Option<String>)> {
        let spec = PackageSpec::parse(input)?;
        let candidates = self
            .files
            .get(&spec.name.to_ascii_lowercase())
            .into_iter()
            .flatten()
            .filter(|(b, _)| {
                spec.bucket
                    .as_ref()
                    .is_none_or(|name| name.eq_ignore_ascii_case(b))
            })
            .collect::<Vec<_>>();
        let (b, path) = candidates.first().with_context(|| {
            format!(
                "Package {input} was not found. Check rsc bucket or provide a manifest file / URL."
            )
        })?;
        let warning = (candidates.len() > 1)
            .then(|| format!("multiple buckets contain {input}; using {b}/{}", spec.name));
        let manifest = Manifest::read(path)?;
        if let Some(version) = spec.version {
            if manifest.version()? != version {
                bail!(
                    "Requested {version}; local manifest is {}",
                    manifest.version()?
                );
            }
        }
        Ok((
            Resolved {
                name: path
                    .file_stem()
                    .context("Manifest has no filename")?
                    .to_string_lossy()
                    .into_owned(),
                bucket: Some(b.clone()),
                source: path.display().to_string(),
                manifest,
            },
            warning,
        ))
    }
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
    Resolver::new(root)?.resolve(input)
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
