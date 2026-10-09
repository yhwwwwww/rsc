//! Scoop-compatible local search without constructing complete manifest trees.
use crate::{bucket, config::Config, database, native::query, package, util};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, path::Path};

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub explicit: bool,
    pub name_only: bool,
    pub with_description: bool,
}
impl Options {
    pub fn is_default(self) -> bool {
        !self.explicit && !self.name_only && !self.with_description
    }
}
#[derive(Debug, Serialize)]
pub struct Installation {
    pub scope: String,
    pub version: String,
    pub state: String,
    pub held: bool,
    pub source_unknown: bool,
}
#[derive(Debug)]
pub struct SearchIndex {
    pub packages: Vec<SearchMatch>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct SearchMatch {
    pub name: String,
    pub bucket: String,
    pub source: String,
    pub version: String,
    pub binaries: Vec<String>,
    pub description: String,
}
#[derive(Debug)]
pub struct Report {
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Row {
    pub package: String,
    pub bucket: String,
    pub version: String,
    pub binaries: String,
    pub installed: String,
    pub state: String,
    pub installations: Vec<Installation>,
    pub description: String,
}
#[derive(Deserialize)]
struct SearchManifest {
    #[serde(alias = "Version")]
    version: String,
    #[serde(default, alias = "Bin")]
    bin: Value,
    #[serde(default, alias = "Description")]
    description: Value,
}

/// Only top-level bin entries participate in uncached Scoop search. Match the
/// executable's basename first, then its alias; retain the original filename.
pub fn matching_binaries(bin: &Value, re: &fancy_regex::Regex) -> Result<Vec<String>> {
    fn entry(bin: &Value, re: &fancy_regex::Regex) -> Result<Option<String>> {
        let (exe, alias) = if let Some(exe) = bin.as_str() {
            (Some(exe), None)
        } else if let Some(parts) = bin.as_array() {
            (
                parts.first().and_then(Value::as_str),
                parts.get(1).and_then(Value::as_str),
            )
        } else {
            (None, None)
        };
        if let Some(exe) = exe {
            let leaf = exe.rsplit(['/', '\\']).next().unwrap_or(exe);
            let stem = Path::new(leaf)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(leaf);
            if query::matches(re, stem)? {
                return Ok(Some(leaf.to_owned()));
            }
        }
        if let Some(alias) = alias {
            if query::matches(re, alias)? {
                return Ok(Some(alias.to_owned()));
            }
        }
        Ok(None)
    }
    let mut matches = Vec::new();
    if let Some(entries) = bin.as_array() {
        for bin in entries {
            if let Some(found) = entry(bin, re)? {
                matches.push(found);
            }
        }
    } else if let Some(found) = entry(bin, re)? {
        matches.push(found);
    }
    Ok(matches)
}

pub fn scan(root: &Path, input: &str) -> Result<SearchIndex> {
    scan_with_options(root, input, Options::default())
}
pub fn scan_with_options(root: &Path, input: &str, options: Options) -> Result<SearchIndex> {
    let expression = if options.explicit {
        regex::escape(input)
    } else {
        input.to_owned()
    };
    let re = query::matcher(&expression)?;
    let buckets = bucket::inventory(root)?;
    let catalogues = util::parallel_map(&buckets, |b| {
        bucket::manifest_paths(&b.path).map(|paths| {
            paths
                .into_iter()
                .map(|path| (b.name.clone(), path))
                .collect::<Vec<_>>()
        })
    });
    let paths = catalogues
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    // A matcher per worker avoids sharing a regex scratch-space pool.
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .min(16);
    let chunks = paths
        .chunks(paths.len().div_ceil(workers).max(1))
        .collect::<Vec<_>>();
    let results = util::parallel_map(&chunks, |chunk| -> Result<SearchIndex> {
        let re = re.clone();
        let mut packages = Vec::new();
        let mut warnings = Vec::new();
        for (bucket, path) in *chunk {
            let name = path
                .file_stem()
                .context("Manifest has no filename")?
                .to_string_lossy();
            let named = query::matches(&re, &name)?;
            if options.name_only && !named {
                continue;
            }
            let text = match util::read_text(path) {
                Ok(text) => text,
                Err(e) => {
                    warnings.push(format!("{}: {e:#}", path.display()));
                    continue;
                }
            };
            // An unnamed result needs a top-level bin. Skip files that cannot
            // contain one; escaped JSON property names still take the parser path.
            if !named
                && !options.with_description
                && !text.contains("\"bin\"")
                && !text.contains("\"Bin\"")
                && !text.contains("\"\\u")
            {
                continue;
            }
            // Scoop prefilters by name or raw content before parsing JSON.
            if !named
                && !options.explicit
                && !options.with_description
                && !query::matches(&re, &text)?
            {
                continue;
            }
            let manifest: SearchManifest = match serde_json::from_str(&text) {
                Ok(manifest) => manifest,
                Err(e) => {
                    warnings.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let binaries = if named {
                Vec::new()
            } else {
                matching_binaries(&manifest.bin, &re)?
            };
            let description = manifest.description.as_str().unwrap_or("");
            let described = options.with_description && query::matches(&re, description)?;
            if !named && binaries.is_empty() && !described {
                continue;
            }
            packages.push(SearchMatch {
                name: name.into_owned(),
                bucket: bucket.clone(),
                source: path.display().to_string(),
                version: manifest.version,
                binaries,
                description: if options.with_description {
                    description.to_owned()
                } else {
                    String::new()
                },
            });
        }
        Ok(SearchIndex { packages, warnings })
    });
    let mut found = SearchIndex {
        packages: Vec::new(),
        warnings: Vec::new(),
    };
    for result in results {
        let result = result?;
        found.packages.extend(result.packages);
        found.warnings.extend(result.warnings);
    }
    Ok(found)
}

pub fn local(config: &Config, input: &str) -> Result<Report> {
    local_with_options(config, input, Options::default())
}
pub fn local_with_options(config: &Config, input: &str, options: Options) -> Result<Report> {
    let (mut rows, warnings) = if database::enabled(config) {
        let cached = database::search_with_options(config, input, options)?;
        let rows = cached
            .as_array()
            .context("Invalid cached search result")?
            .iter()
            .map(|p| {
                let text = |key| p[key].as_str().unwrap_or("").to_owned();
                Row {
                    package: text("package"),
                    bucket: text("bucket"),
                    version: text("version"),
                    binaries: text("bins"),
                    installed: String::new(),
                    state: String::new(),
                    installations: Vec::new(),
                    description: if options.with_description {
                        text("description")
                    } else {
                        String::new()
                    },
                }
            })
            .collect();
        (rows, Vec::new())
    } else {
        let found = scan_with_options(&config.layout.buckets(), input, options)?;
        (
            found
                .packages
                .into_iter()
                .map(|p| Row {
                    package: p.name,
                    bucket: p.bucket,
                    version: p.version,
                    binaries: p.binaries.join(" | "),
                    installed: String::new(),
                    state: String::new(),
                    installations: Vec::new(),
                    description: p.description,
                })
                .collect::<Vec<_>>(),
            found.warnings,
        )
    };
    let names = rows
        .iter()
        .map(|r| r.package.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let installed = package::list_matching(&config.layout, &names)?;
    let mut unknown_sources = HashSet::new();
    for row in &mut rows {
        let mut versions = Vec::new();
        let mut states = Vec::new();
        for p in installed
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(&row.package))
        {
            if let Some(bucket) = &p.bucket {
                if !bucket.eq_ignore_ascii_case(&row.bucket) {
                    continue;
                }
            } else if !unknown_sources.insert((p.name.to_ascii_lowercase(), p.scope.clone())) {
                continue;
            }
            let version = p.version.as_deref().unwrap_or("?");
            versions.push(format!("{} {version}", p.scope));
            let state = installation_state(p, &row.version);
            let held = if p.held { ", held" } else { "" };
            let source = if p.bucket.is_none() {
                ", source unknown"
            } else {
                ""
            };
            states.push(format!("{}: {state}{held}{source}", p.scope));
            row.installations.push(Installation {
                scope: p.scope.clone(),
                version: version.to_owned(),
                state: state.to_owned(),
                held: p.held,
                source_unknown: p.bucket.is_none(),
            });
        }
        row.installed = versions.join(" | ");
        row.state = states.join(", ");
    }
    Ok(Report { rows, warnings })
}

/// State compares against this result's local bucket version, never another
/// bucket with the same package name. Unversioned nightlies remain unknown.
pub fn installation_state(p: &package::Installed, available: &str) -> &'static str {
    if p.error.is_some() || p.version.is_none() {
        return "broken";
    }
    if available.eq_ignore_ascii_case("nightly") {
        return "unknown (nightly)";
    }
    match query::compare(available, p.version.as_deref().unwrap()) {
        std::cmp::Ordering::Greater => "outdated",
        std::cmp::Ordering::Equal => "current",
        std::cmp::Ordering::Less => "newer",
    }
}
