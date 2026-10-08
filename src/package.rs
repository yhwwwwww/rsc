use crate::{layout::Layout, manifest::Manifest, util};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, Serialize)]
pub struct Installed {
    pub name: String,
    pub version: Option<String>,
    pub bucket: Option<String>,
    pub architecture: Option<String>,
    pub scope: String,
    pub held: bool,
    pub path: PathBuf,
    pub state: String,
    pub error: Option<String>,
}
pub fn list(layout: &Layout, only_global: bool) -> Result<Vec<Installed>> {
    let mut result = Vec::new();
    for global in [false, true] {
        if only_global && !global {
            continue;
        }
        let root = layout.apps(global);
        if !root.try_exists()? {
            continue;
        }
        for entry in
            fs::read_dir(&root).with_context(|| format!("Cannot read {}", root.display()))?
        {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case("scoop") {
                continue;
            }
            let path = entry.path();
            let mut item = Installed {
                name,
                version: None,
                bucket: None,
                architecture: None,
                scope: if global { "global" } else { "user" }.into(),
                held: false,
                path: path.clone(),
                state: "broken".into(),
                error: None,
            };
            match read_installed(&path, layout.no_junction) {
                Ok((current, manifest, info)) => {
                    item.path = current;
                    item.version = Some(if manifest.version()? == "nightly" {
                        fs::canonicalize(&item.path)?
                            .file_name()
                            .context("Nightly has no version directory")?
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        manifest.version()?.to_owned()
                    });
                    item.bucket = util::ci_get(&info, "bucket")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    item.architecture = util::ci_get(&info, "architecture")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    item.held = util::ci_get(&info, "hold")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    item.state = if item.held { "held" } else { "installed" }.into();
                }
                Err(error) => item.error = Some(format!("{error:#}")),
            }
            result.push(item);
        }
    }
    result.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.scope.cmp(&b.scope))
    });
    Ok(result)
}
fn read_installed(app: &Path, no_junction: bool) -> Result<(PathBuf, Manifest, serde_json::Value)> {
    let current = app.join("current");
    let selected = if !no_junction && current.is_dir() {
        current
    } else {
        let mut versions: Vec<(SystemTime, PathBuf)> = Vec::new();
        for entry in fs::read_dir(app)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.path().is_dir()
                || name == "current"
                || (name.starts_with('_') && name.contains(".old"))
            {
                continue;
            }
            let path = metadata_file(&entry.path(), "scoop-install.json", "install.json");
            if path.is_file() {
                versions.push((fs::metadata(path)?.modified()?, entry.path()));
            }
        }
        versions.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        versions
            .pop()
            .map(|(_, p)| p)
            .context("No complete installation metadata found")?
    };
    if selected.join(".rsc-installing.json").is_file() {
        bail!("Installation did not finish; run rsc uninstall before retrying");
    }
    let manifest = Manifest::read(&metadata_file(
        &selected,
        "scoop-manifest.json",
        "manifest.json",
    ))?;
    let info = util::read_json(&metadata_file(
        &selected,
        "scoop-install.json",
        "install.json",
    ))?;
    if !info.is_object() {
        bail!("Installation metadata must be an object");
    }
    Ok((selected, manifest, info))
}
pub fn metadata_file(dir: &Path, modern: &str, legacy: &str) -> PathBuf {
    let path = dir.join(modern);
    if path.is_file() {
        path
    } else {
        dir.join(legacy)
    }
}
pub fn find(layout: &Layout, input: &str, global: bool) -> Result<Installed> {
    let spec = crate::bucket::PackageSpec::parse(input)?;
    let matches: Vec<Installed> = list(layout, global)?
        .into_iter()
        .filter(|p| {
            p.name.eq_ignore_ascii_case(&spec.name)
                && spec
                    .bucket
                    .as_ref()
                    .is_none_or(|b| p.bucket.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(b)))
                && spec
                    .version
                    .as_ref()
                    .is_none_or(|v| p.version.as_ref() == Some(v))
        })
        .collect();
    let selected = matches
        .iter()
        .find(|p| p.scope == if global { "global" } else { "user" })
        .or_else(|| matches.first())
        .with_context(|| format!("{input} is not installed"))?;
    if let Some(error) = &selected.error {
        bail!("{} has an incomplete installation: {error}", selected.name);
    }
    Ok(selected.clone())
}
