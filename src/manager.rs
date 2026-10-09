use crate::{
    bucket::{self, PackageSpec, Resolved},
    config::Config,
    database,
    download::{self, Downloader, Reporter, Task},
    manifest::{Architecture, Manifest},
    native,
    package::{self, Installed},
    util::{self, FileLock},
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command, sync::Arc};
use tokio::{sync::Semaphore, task::JoinSet};
#[derive(Clone, Copy, Default)]
pub struct InstallOptions {
    pub global: bool,
    pub independent: bool,
    pub no_cache: bool,
    pub skip_hash: bool,
    pub force: bool,
    pub updating: bool,
}
#[derive(Default)]
pub struct Outcome {
    pub rows: Vec<Value>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}
/// Status notices are report data; bucket updates are expected, not diagnostics.
#[derive(Default)]
pub struct StatusOutcome {
    pub rows: Vec<Value>,
    pub bucket_updates: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}
pub fn lock(config: &Config, name: &str, global: bool) -> Result<FileLock> {
    util::valid_name(name)?;
    FileLock::acquire(
        &config
            .layout
            .base(global)
            .join(".rsc/locks")
            .join(format!("{}.lock", name.to_lowercase())),
    )
}
pub fn require_admin(config: &Config, global: bool) -> Result<()> {
    if global {
        native::invoke(config, "admin", json!({}))?;
    }
    Ok(())
}
pub async fn resolve(config: &Config, input: &str) -> Result<Resolved> {
    let mut historical = Value::Null;
    if let Ok(spec) = PackageSpec::parse(input) {
        if let Some(version) = &spec.version {
            let base = if let Some(bucket) = &spec.bucket {
                format!("{bucket}/{}", spec.name)
            } else {
                spec.name.clone()
            };
            if let Ok(p) = bucket::resolve(&config.layout.buckets(), &base) {
                if let Some(bucket) = p.bucket {
                    if let Some(text) = database::historical(config, &spec.name, &bucket, version)?
                    {
                        historical = json!({"ManifestText":text,"version":version,"source":"sqlite_exact_match"});
                    }
                }
            }
        }
    }
    let value = native::invoke(
        config,
        "resolve",
        json!({"input":input,"historical":historical}),
    )?;
    let name = value["name"]
        .as_str()
        .context("Resolved manifest has no name")?
        .to_owned();
    util::valid_name(&name)?;
    let bucket = value["bucket"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    let source = value["source"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_default();
    Ok(Resolved {
        name,
        bucket,
        source: source.clone(),
        manifest: if std::path::Path::new(&source).is_file() {
            Manifest::read(std::path::Path::new(&source))?
        } else {
            Manifest::parse(serde_json::to_string_pretty(&value["manifest"])?)?
        },
    })
}
pub fn prepare(
    config: &Config,
    p: &Resolved,
    arch: Architecture,
) -> Result<(Architecture, Vec<String>, String)> {
    let value = native::invoke(
        config,
        "prepare",
        json!({"name":p.name,"manifest":p.manifest.raw,"architecture":arch}),
    )?;
    let arch = Architecture::parse(
        value["architecture"]
            .as_str()
            .context("No supported architecture")?,
    )?;
    let helpers = value["helpers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok((
        arch,
        helpers,
        value["nightly"].as_str().unwrap_or("nightly").into(),
    ))
}
pub async fn plan(
    config: &Config,
    inputs: &[String],
    arch: Architecture,
    independent: bool,
) -> Result<Vec<(Resolved, Architecture, String)>> {
    let mut pending: Vec<(String, bool)> =
        inputs.iter().rev().map(|s| (s.clone(), false)).collect();
    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut prepared = std::collections::BTreeMap::new();
    let mut result = Vec::new();
    while let Some((input, exiting)) = pending.pop() {
        if exiting {
            let (p, arch, version) = prepared.remove(&input).context("Missing dependency plan")?;
            active.remove(&input);
            done.insert(input);
            result.push((p, arch, version));
            continue;
        }
        let p = resolve(config, &input).await?;
        let key = format!(
            "{}/{}@{}",
            p.bucket.as_deref().unwrap_or(""),
            p.name,
            p.manifest.version()?
        );
        if done.contains(&key) {
            continue;
        }
        if !active.insert(key.clone()) {
            bail!("Dependency cycle at {key}");
        }
        if active.len() > 128 {
            bail!("Dependency graph is too deep");
        }
        let (arch, helpers, nightly) = prepare(config, &p, arch)?;
        let version = if p.manifest.version()? == "nightly" {
            nightly
        } else {
            p.manifest.version()?.into()
        };
        util::valid_component(&version)?;
        let deps = if independent {
            Vec::new()
        } else {
            helpers
                .into_iter()
                .chain(p.manifest.strings("depends", arch)?)
                .collect::<Vec<_>>()
        };
        prepared.insert(key.clone(), (p, arch, version));
        pending.push((key, true));
        for dependency in deps.into_iter().rev() {
            pending.push((dependency, false));
        }
    }
    Ok(result)
}
pub async fn install(
    config: &Config,
    inputs: &[String],
    arch: Architecture,
    options: InstallOptions,
    report: Reporter,
) -> Result<Outcome> {
    require_admin(config, options.global)?;
    if bucket::inventory(&config.layout.buckets())?.is_empty()
        && inputs.iter().any(|s| PackageSpec::parse(s).is_ok())
    {
        add_bucket(config, "main", None)?;
    }
    let requested_versions = inputs
        .iter()
        .filter_map(|s| PackageSpec::parse(s).ok())
        .filter_map(|s| s.version.map(|v| (s.name.to_lowercase(), v)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let plans = plan(config, inputs, arch, options.independent).await?;
    let mut outcome = Outcome::default();
    let mut failed = BTreeSet::new();
    for (p, arch, version) in plans {
        let item = async {
            let mut app_lock = Some(lock(config,&p.name,options.global)?);
            let find_existing = || -> Result<Option<Installed>> {
                Ok(package::list(&config.layout,options.global)?.into_iter().find(|x|
                    x.name.eq_ignore_ascii_case(&p.name) && x.scope == if options.global {"global"} else {"user"}))
            };
            let mut existing=find_existing()?;
            if let Some(old)=existing.as_ref().filter(|old|old.error.is_some()) {
                let complete=package::metadata_file(&old.path,"scoop-install.json","install.json").is_file();
                // Lifecycle acquires the same package lock itself.
                drop(app_lock.take());
                let action=if complete {"reset"} else {"uninstall"};
                outcome.warnings.push(format!("{}: {} previous failed installation",p.name,
                    if complete {"repairing"} else {"removing"}));
                let recovery=lifecycle(config,action,&[p.name.clone()],options.global,false,false)?;
                if !recovery.errors.is_empty() {bail!("{}",recovery.errors.join("\n"));}
                if complete {
                    let marker=old.path.join(".rsc-installing.json");
                    if marker.is_file() {fs::remove_file(marker)?;}
                }
                outcome.rows.extend(recovery.rows);
                outcome.warnings.extend(recovery.warnings);
                app_lock=Some(lock(config,&p.name,options.global)?);
                existing=find_existing()?;
            }
            let _app_lock=app_lock;
            if let Some(old) = existing {
                if old.error.is_some() { bail!("{} still has an incomplete installation after recovery",p.name); }
                if !options.force && !requested_versions.get(&p.name.to_lowercase()).is_some_and(|v|old.version.as_ref()!=Some(v)) {
                    outcome.rows.push(json!({"package":p.name,"version":old.version,"scope":old.scope,"result":"already installed"}));
                    return Ok::<_,anyhow::Error>(());
                }
            }
            for dep in p.manifest.strings("depends",arch)? {
                let name = PackageSpec::parse(&dep)?.name;
                if failed.contains(&name.to_ascii_lowercase()) { bail!("Dependency {dep} failed"); }
            }
            install_one(config,&p,arch,&version,options,report.clone(),&mut outcome).await
        }.await;
        if let Err(error) = item {
            failed.insert(p.name.to_ascii_lowercase());
            outcome.errors.push(format!("{}: {error:#}", p.name));
        }
    }
    Ok(outcome)
}
async fn install_one(
    config: &Config,
    p: &Resolved,
    arch: Architecture,
    version: &str,
    options: InstallOptions,
    report: Reporter,
    outcome: &mut Outcome,
) -> Result<()> {
    let app_root = config.layout.apps(options.global).join(&p.name);
    let dir = app_root.join(version);
    if dir.exists() {
        bail!(
            "Version directory already exists: {}. Use reset or cleanup before reinstalling.",
            dir.display()
        );
    }
    if !options.updating
        && config.get("show_manifest").and_then(Value::as_bool) == Some(true)
        && native::invoke(
            config,
            "confirm_install",
            json!({"name":p.name,"manifest":p.manifest.raw}),
        )?
        .as_bool()
            != Some(true)
    {
        outcome
            .rows
            .push(json!({"package":p.name,"version":version,"result":"skipped"}));
        return Ok(());
    }
    let staged = stage_files(config, p, arch, version, options, report, outcome).await?;
    commit_install(config, p, arch, version, options, staged, outcome).await
}

struct StagedFiles {
    names: Vec<String>,
    downloaded: Vec<(usize, download::Downloaded)>,
    _temporary: Option<tempfile::TempDir>,
}
async fn stage_files(
    config: &Config,
    p: &Resolved,
    arch: Architecture,
    version: &str,
    options: InstallOptions,
    report: Reporter,
    outcome: &mut Outcome,
) -> Result<StagedFiles> {
    let temporary = if options.no_cache {
        Some(tempfile::tempdir()?)
    } else {
        None
    };
    let mut downloader = Downloader::new(config)?;
    if let Some(temp) = &temporary {
        downloader = downloader.cache_directory(temp.path().to_owned());
    }
    let mut files = p.manifest.downloads(arch)?;
    if options.skip_hash || p.manifest.version()? == "nightly" {
        for file in &mut files {
            file.hash = None;
        }
    }
    if options.skip_hash {
        outcome.warnings.push(format!(
            "{}: hash validation was explicitly skipped",
            p.name
        ));
    }
    let names = native::invoke(
        config,
        "filename",
        json!({"urls":files.iter().map(|f|f.url.clone()).collect::<Vec<_>>()}),
    )?;
    let names: Vec<String> = match names {
        Value::Array(a) => a
            .into_iter()
            .map(|v| v.as_str().context("Invalid filename").map(str::to_owned))
            .collect::<Result<_>>()?,
        Value::String(s) => vec![s],
        _ => bail!("No download filenames"),
    };
    if names.len() != files.len() {
        bail!("Download filename count mismatch");
    }
    let mut unique = BTreeSet::new();
    for name in &names {
        util::valid_component(name)?;
        if !unique.insert(name.to_lowercase()) {
            bail!("Duplicate download filename: {name}");
        }
    }
    let headers = download::headers(&p.manifest, arch)?;
    let semaphore = Arc::new(Semaphore::new(downloader.concurrency()));
    let mut jobs = JoinSet::new();
    for (i, file) in files.into_iter().enumerate() {
        let downloader = downloader.clone();
        let semaphore = semaphore.clone();
        let report = report.clone();
        let task = Task {
            id: i,
            label: format!("{} [{}/{}]", p.name, i + 1, names.len()),
            app: p.name.clone(),
            version: version.into(),
            file,
            headers: headers.clone(),
        };
        jobs.spawn(async move {
            let _permit = semaphore.acquire_owned().await?;
            Ok::<_, anyhow::Error>((i, downloader.download(task, report).await?))
        });
    }
    let mut downloaded = Vec::new();
    while let Some(result) = jobs.join_next().await {
        match result {
            Ok(Ok(value)) => downloaded.push(value),
            Ok(Err(error)) => {
                jobs.abort_all();
                while jobs.join_next().await.is_some() {}
                return Err(error);
            }
            Err(error) => {
                jobs.abort_all();
                return Err(error.into());
            }
        }
    }
    Ok(StagedFiles {
        names,
        downloaded,
        _temporary: temporary,
    })
}

async fn commit_install(
    config: &Config,
    p: &Resolved,
    arch: Architecture,
    version: &str,
    options: InstallOptions,
    staged: StagedFiles,
    outcome: &mut Outcome,
) -> Result<()> {
    let app_root = config.layout.apps(options.global).join(&p.name);
    let dir = app_root.join(version);
    let StagedFiles {
        names,
        downloaded,
        _temporary,
    } = staged;
    fs::create_dir_all(&app_root)?;
    fs::create_dir(&dir)?;
    util::atomic_write(&dir.join("scoop-manifest.json"), p.manifest.text.as_bytes())?;
    util::write_json(
        &dir.join(".rsc-installing.json"),
        &json!({"package":p.name,"version":version,"architecture":arch}),
    )?;
    for (index, file) in downloaded {
        fs::copy(file.path, dir.join(&names[index]))?;
    }
    if let Err(error) = native::invoke(
        config,
        "install",
        json!({"name":p.name,"global":options.global,"architecture":arch,"version":version,"bucket":p.bucket,"manifest":p.manifest.raw,"dir":dir,"files":names}),
    ) {
        bail!(
            "{error:#}\nFiles remain at {}. Run rsc uninstall {} to remove the partial installation.",
            dir.display(),
            p.name
        );
    }
    let mut info = json!({"architecture":arch});
    if let Some(bucket) = &p.bucket {
        info["bucket"] = json!(bucket);
    } else {
        info["url"] = json!(p.source);
    }
    util::write_json(&dir.join("scoop-install.json"), &info)?;
    fs::remove_file(dir.join(".rsc-installing.json"))?;
    outcome.rows.push(json!({"package":p.name,"version":version,"scope":if options.global {"global"} else {"user"},"result":"installed"}));
    Ok(())
}

pub fn lifecycle(
    config: &Config,
    action: &str,
    inputs: &[String],
    global: bool,
    purge: bool,
    cache: bool,
) -> Result<Outcome> {
    require_admin(config, global)?;
    let mut outcome = Outcome::default();
    if action == "uninstall" && inputs.iter().any(|s| s == "*") {
        bail!("uninstall requires explicit package names");
    }
    let installed = package::list(&config.layout, false)?;
    let targets: Vec<(String, Option<bool>)> = if inputs.iter().any(|s| s == "*") {
        installed
            .into_iter()
            .filter(|p| {
                action == "reset"
                    || (action == "cleanup" && global)
                    || p.scope == if global { "global" } else { "user" }
            })
            .map(|p| (p.name, Some(p.scope == "global")))
            .collect()
    } else {
        inputs.iter().map(|s| (s.clone(), None)).collect()
    };
    for (input, target_scope) in targets {
        let operation = (|| -> Result<Value> {
            let spec = PackageSpec::parse(&input)?;
            let p = package::list(&config.layout, false)?
                .into_iter()
                .find(|p| {
                    p.name.eq_ignore_ascii_case(&spec.name)
                        && (if let Some(scope) = target_scope {
                            p.scope == if scope { "global" } else { "user" }
                        } else {
                            action == "reset" || p.scope == if global { "global" } else { "user" }
                        })
                })
                .with_context(|| format!("{input} is not installed in the requested scope"))?;
            let scope = p.scope == "global";
            require_admin(config, scope)?;
            let _lock = lock(config, &p.name, scope)?;
            let app_root = config.layout.apps(scope).join(&p.name);
            let version = if let Some(v) = spec.version {
                v
            } else {
                fs::canonicalize(&p.path)
                    .ok()
                    .and_then(|p| p.file_name().map(|x| x.to_string_lossy().into_owned()))
                    .filter(|v| v != &p.name)
                    .or(p.version.clone())
                    .or_else(|| {
                        fs::read_dir(&app_root)
                            .ok()?
                            .filter_map(|e| e.ok())
                            .find(|e| e.path().join(".rsc-installing.json").is_file())
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                    })
                    .or_else(|| {
                        if action != "uninstall" {
                            return None;
                        }
                        // Scoop can leave a version directory before writing either metadata file.
                        let mut versions: Vec<_> = fs::read_dir(&app_root)
                            .ok()?
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                let name = e.file_name().to_string_lossy().into_owned();
                                e.path().is_dir()
                                    && name != "current"
                                    && !(name.starts_with('_') && name.contains(".old"))
                            })
                            .collect();
                        versions.sort_by_key(|e| {
                            (
                                e.metadata().ok().and_then(|m| m.modified().ok()),
                                e.file_name(),
                            )
                        });
                        Some(
                            versions
                                .pop()
                                .map(|e| e.file_name().to_string_lossy().into_owned())
                                .unwrap_or_else(|| "current".to_owned()),
                        )
                    })
                    .context("No installed version directory found")?
            };
            util::valid_component(&version)?;
            let dir = app_root.join(&version);
            let manifest_path =
                package::metadata_file(&dir, "scoop-manifest.json", "manifest.json");
            let manifest = if manifest_path.is_file() {
                Some(Manifest::read(&manifest_path)?)
            } else {
                None
            };
            if action != "uninstall" && manifest.is_none() {
                bail!("Installation manifest missing");
            }
            let metadata = package::metadata_file(&dir, "scoop-install.json", "install.json");
            let pending = dir.join(".rsc-installing.json");
            let info = if metadata.is_file() {
                util::read_json(&metadata)?
            } else if pending.is_file() {
                util::read_json(&pending)?
            } else {
                json!({})
            };
            let arch = util::ci_get(&info, "architecture")
                .and_then(Value::as_str)
                .or(p.architecture.as_deref())
                .unwrap_or("64bit");
            let value = native::invoke(
                config,
                action,
                json!({"name":p.name,"global":scope,"architecture":arch,"version":version,"dir":dir,"manifest":manifest.map(|m|m.raw),"purge":purge}),
            )?;
            if action == "cleanup" && cache {
                clean_cache(config, &regex::escape(&p.name), Some(&version))?;
            }
            Ok(
                json!({"package":p.name,"version":version,"scope":p.scope,"result":action,"details":value}),
            )
        })();
        match operation {
            Ok(row) => outcome.rows.push(row),
            Err(error) => outcome.errors.push(format!("{input}: {error:#}")),
        }
    }
    Ok(outcome)
}
pub fn hold(config: &Config, inputs: &[String], global: bool, held: bool) -> Result<Outcome> {
    require_admin(config, global)?;
    let mut outcome = Outcome::default();
    for input in inputs {
        let operation = (|| -> Result<Value> {
            let p = package::find(&config.layout, input, global)?;
            if p.scope != if global { "global" } else { "user" } {
                bail!("{input} is not installed in this scope");
            }
            let _lock = lock(config, &p.name, global)?;
            let path = package::metadata_file(&p.path, "scoop-install.json", "install.json");
            let mut info = util::read_json(&path)?;
            let object = info.as_object_mut().context("Invalid install metadata")?;
            let key = object
                .keys()
                .find(|k| k.eq_ignore_ascii_case("hold"))
                .cloned()
                .unwrap_or_else(|| "hold".into());
            if held {
                object.insert(key, json!(true));
            } else {
                object.remove(&key);
            }
            util::write_json(&path, &info)?;
            Ok(
                json!({"package":p.name,"version":p.version,"scope":p.scope,"result":if held {"held"} else {"unheld"}}),
            )
        })();
        match operation {
            Ok(row) => outcome.rows.push(row),
            Err(error) => outcome.errors.push(format!("{input}: {error:#}")),
        }
    }
    Ok(outcome)
}
pub fn git(config: &Config, directory: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    if let Some(path) = directory {
        command.arg("-C").arg(path);
    }
    if let Some(proxy) = config.text("proxy")?.filter(|s| s != "none") {
        if proxy != "default" {
            let proxy = proxy.replace("currentuser@", ":@");
            command.env("HTTPS_PROXY", &proxy).env("HTTP_PROXY", &proxy);
        }
    }
    let output = command
        .args(args)
        .output()
        .context("Git is required for buckets; install Git or use existing local buckets")?;
    if !output.status.success() {
        bail!(
            "Git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}
pub fn add_bucket(config: &Config, name: &str, repo: Option<&str>) -> Result<Value> {
    util::valid_name(name)?;
    let _lock = FileLock::acquire(&config.layout.root.join(".rsc/locks/buckets.lock"))?;
    let known: Value = crate::native::known::json();
    let repo = repo
        .or_else(|| util::ci_get(&known, name).and_then(Value::as_str))
        .with_context(|| format!("Unknown bucket {name}; specify a repository"))?;
    let dir = config.layout.buckets().join(name);
    if dir.exists() {
        bail!("Bucket {name} already exists");
    }
    for bucket in bucket::buckets(&config.layout.buckets())? {
        if let Some(remote) = bucket.remote {
            if remote
                .trim_end_matches(".git")
                .eq_ignore_ascii_case(repo.trim_end_matches(".git"))
            {
                bail!("Repository already belongs to bucket {}", bucket.name);
            }
        }
    }
    git(config, None, &["ls-remote", "--", repo])?;
    fs::create_dir_all(config.layout.buckets())?;
    let stage = tempfile::Builder::new()
        .prefix(".rsc-bucket-")
        .tempdir_in(config.layout.buckets())?;
    let clone = stage.path().join("clone");
    git(
        config,
        None,
        &[
            "clone",
            "-q",
            "--",
            repo,
            clone.to_str().context("Non-Unicode clone path")?,
        ],
    )?;
    fs::rename(clone, &dir)?;
    if database::enabled(config) {
        database::refresh(config)?;
    }
    Ok(json!({"bucket":name,"repository":util::redact_url(repo),"result":"added"}))
}
pub fn remove_bucket(config: &Config, name: &str) -> Result<Value> {
    util::valid_name(name)?;
    let _lock = FileLock::acquire(&config.layout.root.join(".rsc/locks/buckets.lock"))?;
    let root = fs::canonicalize(config.layout.buckets())?;
    let dir = root.join(name);
    if fs::canonicalize(&dir)?.parent() != Some(root.as_path()) {
        bail!("Bucket path points outside buckets");
    }
    util::remove_tree(&dir)?;
    database::remove_bucket(config, name)?;
    Ok(json!({"bucket":name,"result":"removed"}))
}
pub fn sync(config: &Config) -> Result<Outcome> {
    let _lock = FileLock::acquire(&config.layout.root.join(".rsc/locks/buckets.lock"))?;
    let mut outcome = Outcome::default();
    for b in bucket::buckets(&config.layout.buckets())? {
        if !b.path.join(".git").exists() {
            outcome
                .warnings
                .push(format!("{} is not a Git repository; skipped", b.name));
            continue;
        }
        let operation = (|| -> Result<Value> {
            let manifest_names = |path: &Path| -> Result<BTreeSet<String>> {
                Ok(bucket::manifest_paths(path)?
                    .iter()
                    .filter_map(|p| p.file_stem().map(|n| n.to_string_lossy().to_lowercase()))
                    .collect())
            };
            let old_names = manifest_names(&b.path)?;
            let before = git(config, Some(&b.path), &["rev-parse", "HEAD"])?;
            git(config, Some(&b.path), &["pull", "-q"])?;
            let after = git(config, Some(&b.path), &["rev-parse", "HEAD"])?;
            let new_names = manifest_names(&b.path)?;
            database::remove_names(
                config,
                &b.name,
                &old_names
                    .difference(&new_names)
                    .cloned()
                    .collect::<Vec<_>>(),
            )?;
            Ok(json!({"bucket":b.name,"result":if before==after {"current"} else {"updated"}}))
        })();
        match operation {
            Ok(row) => outcome.rows.push(row),
            Err(error) => outcome.errors.push(format!("{}: {error:#}", b.name)),
        }
    }
    if database::enabled(config) {
        database::refresh(config)?;
    }
    Ok(outcome)
}
pub fn statuses(config: &Config, local: bool) -> Result<StatusOutcome> {
    let mut outcome = StatusOutcome::default();
    if !local {
        let buckets = bucket::inventory(&config.layout.buckets())?
            .into_iter()
            .filter(|b| b.path.join(".git").exists())
            .collect::<Vec<_>>();
        let checks = util::parallel_map(&buckets, |b| {
            let check = (|| -> Result<bool> {
                git(config, Some(&b.path), &["fetch", "-q", "origin"])?;
                Ok(!git(
                    config,
                    Some(&b.path),
                    &["log", "HEAD..@{upstream}", "--oneline"],
                )?
                .is_empty())
            })();
            (b.name.clone(), check)
        });
        for (name, check) in checks {
            match check {
                Ok(true) => outcome.bucket_updates.push(name),
                Err(e) => outcome.warnings.push(format!("{name}: {e:#}")),
                _ => {}
            }
        }
    }
    let installed = package::list(&config.layout, false)?;
    let states = native::query::statuses(config, &installed)?;
    for (p, status) in installed.iter().zip(&states) {
        if let Some(warning) = status["source_warning"].as_str() {
            outcome
                .warnings
                .push(format!("{} ({}): {warning}", p.name, p.scope));
        }
        let mut row = status.clone();
        row["package"] = json!(p.name);
        row["scope"] = json!(p.scope);
        if p.error.is_some() {
            row["failed"] = json!(true);
        }
        outcome.rows.push(row);
    }
    Ok(outcome)
}

pub async fn update(
    config: &Config,
    inputs: &[String],
    arch: Architecture,
    options: InstallOptions,
    report: Reporter,
) -> Result<Outcome> {
    require_admin(config, options.global)?;
    let mut outcome = sync(config)?;
    if inputs.is_empty() || !outcome.errors.is_empty() {
        return Ok(outcome);
    }
    let statuses = statuses(config, true)?.rows;
    let all = inputs.iter().any(|s| s == "*");
    let installed = package::list(&config.layout, false)?;
    let selected = installed
        .into_iter()
        .filter(|p| {
            (if all {
                p.scope == "user" || options.global
            } else {
                p.scope == if options.global { "global" } else { "user" }
            }) && (all
                || inputs.iter().any(|x| {
                    PackageSpec::parse(x)
                        .ok()
                        .is_some_and(|s| s.name.eq_ignore_ascii_case(&p.name))
                }))
        })
        .collect::<Vec<_>>();
    for input in inputs.iter().filter(|s| s.as_str() != "*") {
        if !selected.iter().any(|p| {
            PackageSpec::parse(input)
                .ok()
                .is_some_and(|s| s.name.eq_ignore_ascii_case(&p.name))
        }) {
            outcome.errors.push(format!("{input} is not installed"));
        }
    }
    for old in selected {
        let operation = async {
            let global = old.scope == "global";
            require_admin(config, global)?;
            if old.held {
                outcome
                    .warnings
                    .push(format!("{} is held; skipped", old.name));
                return Ok::<_, anyhow::Error>(());
            }
            let status = statuses.iter().find(|s| {
                s["package"].as_str() == Some(&old.name) && s["scope"].as_str() == Some(&old.scope)
            });
            if !options.force && !status.is_some_and(|s| s["outdated"].as_bool() == Some(true)) {
                outcome
                    .rows
                    .push(json!({"package":old.name,"version":old.version,"scope":old.scope,"result":"current"}));
                return Ok(());
            }
            let info = util::read_json(&package::metadata_file(
                &old.path,
                "scoop-install.json",
                "install.json",
            ))?;
            let mut input = if let Some(bucket) = &old.bucket {
                format!("{bucket}/{}", old.name)
            } else {
                util::ci_get(&info, "url")
                    .and_then(Value::as_str)
                    .context("Missing source URL")?
                    .into()
            };
            if options.force
                && old.bucket.is_none()
                && input.replace('/', r"\").eq_ignore_ascii_case(
                    &config
                        .layout
                        .root
                        .join("workspace")
                        .join(format!("{}.json", old.name))
                        .display()
                        .to_string()
                        .replace('/', r"\"),
                )
            {
                if let Ok(head) = bucket::resolve(&config.layout.buckets(), &old.name) {
                    if let Some(bucket) = head.bucket {
                        input = format!("{bucket}/{}", old.name);
                    }
                }
            }
            let p = resolve(config, &input).await?;
            let old_arch = old
                .architecture
                .as_deref()
                .map(Architecture::parse)
                .transpose()?
                .unwrap_or(arch);
            let plans = plan(config, &[input], old_arch, options.independent).await?;
            let actual_options = InstallOptions {
                global,
                updating: true,
                ..options
            };
            for (dep, arch, version) in plans.into_iter().filter(|(p, _, _)| p.name != old.name) {
                if package::find(&config.layout, &dep.name, global).is_err() {
                    let _lock = lock(config, &dep.name, global)?;
                    install_one(
                        config,
                        &dep,
                        arch,
                        &version,
                        actual_options,
                        report.clone(),
                        &mut outcome,
                    )
                    .await?;
                }
            }
            let (new_arch, _, nightly) = prepare(config, &p, old_arch)?;
            let version = if p.manifest.version()? == "nightly" {
                nightly
            } else {
                p.manifest.version()?.into()
            };
            util::valid_component(&version)?;
            let _lock = lock(config, &old.name, global)?;
            let mut old_dir = util::canonical_path(&old.path)?;
            let new_dir = config.layout.apps(global).join(&old.name).join(&version);
            let same_directory = new_dir.exists() && util::canonical_path(&new_dir)? == old_dir;
            if new_dir.exists() && !options.force {
                bail!(
                    "Target version is already installed; use reset {}@{version}",
                    old.name
                );
            }
            // Keep verified files (and any no-cache temporary directory) alive until commit.
            let staged = stage_files(
                config,
                &p,
                new_arch,
                &version,
                actual_options,
                report.clone(),
                &mut outcome,
            )
            .await?;
            let old_data = lifecycle_data(&old, &old_dir, &info, global)?;
            native::invoke(config, "unlink", old_data.clone())?;
            if new_dir.exists() {
                let mut backup = new_dir.with_file_name(format!("_{version}.old"));
                let mut n = 1;
                while backup.exists() {
                    backup = new_dir.with_file_name(format!("_{version}.old({n})"));
                    n += 1;
                }
                fs::rename(&new_dir, &backup)?;
                if same_directory {
                    old_dir = backup;
                }
            }
            let result = commit_install(
                config,
                &p,
                new_arch,
                &version,
                actual_options,
                staged,
                &mut outcome,
            )
            .await;
            if let Err(error) = result {
                if same_directory && old_dir != new_dir {
                    if new_dir.exists() {
                        let mut failed =
                            new_dir.with_file_name(format!("_{version}.old(rsc-failed)"));
                        let mut n = 1;
                        while failed.exists() {
                            failed =
                                new_dir.with_file_name(format!("_{version}.old(rsc-failed-{n})"));
                            n += 1;
                        }
                        fs::rename(&new_dir, &failed)?;
                    }
                    fs::rename(&old_dir, &new_dir)?;
                    old_dir = new_dir.clone();
                }
                let mut restore = old_data;
                restore["dir"] = json!(old_dir);
                if let Err(recovery) = native::invoke(config, "reset", restore) {
                    bail!("{error:#}\nRestoring the previous version failed: {recovery:#}");
                }
                return Err(error);
            }
            Ok(())
        }
        .await;
        if let Err(error) = operation {
            outcome.errors.push(format!("{}: {error:#}", old.name));
        }
    }
    Ok(outcome)
}
fn lifecycle_data(p: &Installed, dir: &Path, info: &Value, global: bool) -> Result<Value> {
    let manifest = Manifest::read(&package::metadata_file(
        dir,
        "scoop-manifest.json",
        "manifest.json",
    ))?;
    Ok(
        json!({"name":p.name,"dir":dir,"manifest":manifest.raw,"version":p.version,"architecture":util::ci_get(info,"architecture"),"global":global}),
    )
}
pub fn export(config: &Config, with_config: bool) -> Result<Value> {
    let buckets = bucket::buckets(&config.layout.buckets())?
        .into_iter()
        .map(|b| {
            let source = git(config, Some(&b.path), &["config", "remote.origin.url"])
                .unwrap_or_else(|_| b.path.display().to_string());
            json!({"Name":b.name,"Source":source,"Manifests":b.manifests})
        })
        .collect::<Vec<_>>();
    let mut apps = Vec::new();
    for p in package::list(&config.layout, false)? {
        let metadata = package::metadata_file(&p.path, "scoop-install.json", "install.json");
        let info = if metadata.is_file() {
            util::read_json(&metadata)?
        } else {
            json!({})
        };
        let mut source = p.bucket.clone().or_else(|| {
            util::ci_get(&info, "url")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        if source.as_deref().is_some_and(|s| {
            s.replace('/', r"\").eq_ignore_ascii_case(
                &config
                    .layout
                    .root
                    .join("workspace")
                    .join(format!("{}.json", p.name))
                    .display()
                    .to_string()
                    .replace('/', r"\"),
            )
        }) {
            source = Some("<auto-generated>".into());
        }
        let mut flags = Vec::new();
        if p.scope == "global" {
            flags.push("Global install".to_owned());
        }
        if p.held {
            flags.push("Held package".to_owned());
        }
        if let Some(arch) = p.architecture {
            flags.push(arch);
        }
        apps.push(
            json!({"Name":p.name,"Version":p.version,"Source":source,"Info":flags.join(", ")}),
        );
    }
    let mut result = json!({"buckets":buckets,"apps":apps});
    if with_config {
        let mut cfg = if config.scoop_file.is_file() {
            util::read_json(&config.scoop_file)?
        } else {
            json!({})
        };
        if let Some(object) = cfg.as_object_mut() {
            object.retain(|k, _| {
                ![
                    "last_update",
                    "root_path",
                    "global_path",
                    "cache_path",
                    "alias",
                ]
                .contains(&k.to_lowercase().as_str())
            });
        }
        result["config"] = cfg;
    }
    Ok(result)
}
pub async fn import(config: &Config, input: &str, report: Reporter) -> Result<Outcome> {
    let text = if input.starts_with("https://") || input.starts_with("http://") {
        Downloader::new(config)?.text(input).await?
    } else {
        util::read_text(Path::new(input))?
    };
    let document: Value = serde_json::from_str(&text)?;
    let mut outcome = Outcome::default();
    if let Some(settings) = util::ci_get(&document, "config").and_then(Value::as_object) {
        for (key, value) in settings {
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            config.set(key, Some(&text))?;
        }
    }
    let config = Config::load()?;
    if let Some(buckets) = util::ci_get(&document, "buckets").and_then(Value::as_array) {
        for b in buckets {
            let name = util::ci_get(b, "Name")
                .and_then(Value::as_str)
                .context("Imported bucket missing Name")?;
            util::valid_name(name)?;
            if config.layout.buckets().join(name).exists() {
                continue;
            }
            add_bucket(
                &config,
                name,
                util::ci_get(b, "Source").and_then(Value::as_str),
            )?;
        }
    }
    let apps = util::ci_get(&document, "apps")
        .and_then(Value::as_array)
        .context("Scoopfile missing apps array")?;
    for app in apps {
        let name = util::ci_get(app, "Name")
            .and_then(Value::as_str)
            .context("Imported app missing Name")?;
        let source = util::ci_get(app, "Source")
            .and_then(Value::as_str)
            .unwrap_or(name);
        let info = util::ci_get(app, "Info")
            .and_then(Value::as_str)
            .unwrap_or("");
        let global = info.split(", ").any(|v| v == "Global install");
        let input =
            if util::valid_name(source).is_ok() && config.layout.buckets().join(source).is_dir() {
                format!("{source}/{name}")
            } else if source == "<auto-generated>" {
                format!(
                    "{name}@{}",
                    util::ci_get(app, "Version")
                        .and_then(Value::as_str)
                        .context("Missing pinned version")?
                )
            } else {
                source.into()
            };
        let arch = ["64bit", "32bit", "arm64"]
            .into_iter()
            .find(|a| info.split(", ").any(|s| s == *a))
            .map(Architecture::parse)
            .transpose()?
            .unwrap_or_else(Architecture::native);
        let result = install(
            &config,
            &[input],
            arch,
            InstallOptions {
                global,
                ..Default::default()
            },
            report.clone(),
        )
        .await?;
        if result.errors.is_empty() && info.contains("Held package") {
            let held = hold(&config, &[name.into()], global, true)?;
            outcome.errors.extend(held.errors);
        }
        outcome.rows.extend(result.rows);
        outcome.warnings.extend(result.warnings);
        outcome.errors.extend(result.errors);
    }
    Ok(outcome)
}
pub fn clean_cache(config: &Config, pattern: &str, keep: Option<&str>) -> Result<Vec<Value>> {
    if !config.layout.cache.is_dir() {
        return Ok(Vec::new());
    }
    let all = pattern == "*";
    let query = format!("^(?:{})#", if all { ".*" } else { pattern });
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&config.layout.cache)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            candidates.push((entry.path(), name, false));
        }
    }
    let state_root = config.layout.cache.join(".rsc-downloads");
    if state_root.is_dir() {
        let parent = fs::canonicalize(&state_root)?;
        for entry in fs::read_dir(&state_root)? {
            let entry = entry?;
            let key = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type()?.is_dir()
                || key.len() != 64
                || !key.chars().all(|c| c.is_ascii_hexdigit())
            {
                continue;
            }
            if fs::canonicalize(entry.path())?.parent() != Some(parent.as_path()) {
                bail!("Download state path points outside the cache");
            }
            let metadata = util::read_json(&entry.path().join("package.json")).ok();
            let name = metadata
                .as_ref()
                .and_then(|v| {
                    Some(format!(
                        "{}#{}#partial",
                        v["package"].as_str()?,
                        v["version"].as_str()?
                    ))
                })
                .unwrap_or_else(|| key.clone());
            candidates.push((entry.path(), name, true));
        }
    }
    let matching = native::invoke(
        config,
        "match",
        json!({
            "query":query,"values":candidates.iter().map(|(_,name,_)|name).collect::<Vec<_>>()
        }),
    )?;
    let matches = matching
        .as_array()
        .context("Invalid cache matching result")?;
    let mut removed = Vec::new();
    for ((path, name, directory), matched) in candidates.into_iter().zip(matches) {
        if !(all || matched.as_bool() == Some(true))
            || keep.is_some_and(|v| name.split('#').nth(1) == Some(v))
        {
            continue;
        }
        let key = if directory {
            path.file_name()
                .context("Download state has no key")?
                .to_string_lossy()
                .into_owned()
        } else {
            download::cache_key(&path)?
        };
        let _lock = FileLock::acquire(
            &config
                .layout
                .cache
                .join(".rsc-locks")
                .join(format!("{key}.lock")),
        )?;
        if !path.try_exists()? {
            continue;
        }
        if directory {
            util::remove_tree(&path)?;
        } else {
            fs::remove_file(&path)?;
            if keep.is_none() {
                if let Some((app, _)) = name.split_once('#') {
                    util::valid_name(app)?;
                    let sidecar = config.layout.cache.join(format!("{app}.txt"));
                    if sidecar.is_file() {
                        fs::remove_file(sidecar)?;
                    }
                }
            }
        }
        removed.push(json!({"file":name,"result":"removed"}));
    }
    Ok(removed)
}
