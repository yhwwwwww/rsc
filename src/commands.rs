use crate::output::{Output, Progress};
use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Subcommand};
use rsc_core::{
    config::Config,
    download::{self, Downloader, Task},
    manager::{self, InstallOptions, Outcome},
    manifest::{Architecture, DownloadFile},
    native, util,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[derive(Subcommand)]
pub enum Command {
    /// Install packages with Scoop dependencies, hooks and integration
    Install {
        #[arg(required=true,num_args=1..)]
        packages: Vec<String>,
        #[arg(short = 'i', long, help = "Install without resolving dependencies")]
        independent: bool,
        #[arg(
            short = 'k',
            long,
            help = "Use temporary downloads without reusing or storing cache files"
        )]
        no_cache: bool,
        #[arg(short = 's', long, help = "Skip manifest hash verification")]
        skip_hash_check: bool,
    },
    /// Uninstall packages; preserve persistent data unless purge is requested
    Uninstall {
        #[arg(required=true,num_args=1..)]
        packages: Vec<String>,
        #[arg(short = 'p', long, help = "Also remove persistent package data")]
        purge: bool,
    },
    /// Update buckets, or update selected installed packages
    #[command(group(ArgGroup::new("package_selection").args(["packages", "all"])))]
    Update {
        packages: Vec<String>,
        #[arg(
            short = 'f',
            long,
            requires = "package_selection",
            help = "Reinstall selected packages even when their versions are current"
        )]
        force: bool,
        #[arg(
            short = 'i',
            long,
            requires = "package_selection",
            help = "Update without installing missing dependencies"
        )]
        independent: bool,
        #[arg(
            short = 'k',
            long,
            requires = "package_selection",
            help = "Use temporary downloads without reusing or storing cache files"
        )]
        no_cache: bool,
        #[arg(
            short = 's',
            long,
            requires = "package_selection",
            help = "Skip manifest hash verification"
        )]
        skip_hash_check: bool,
        #[arg(
            short = 'a',
            long,
            conflicts_with = "packages",
            help = "Update all user packages; include global packages with -g"
        )]
        all: bool,
    },
    /// Display package and bucket update status
    Status {
        #[arg(
            short = 'l',
            long,
            help = "Skip fetching remote bucket update information"
        )]
        local: bool,
    },
    /// Rebuild commands, links and environment for an installed version
    Reset {
        packages: Vec<String>,
        #[arg(
            short = 'a',
            long,
            conflicts_with = "packages",
            help = "Reset all installed user and global packages"
        )]
        all: bool,
    },
    /// Remove old versions; optionally remove their download cache
    Cleanup {
        packages: Vec<String>,
        #[arg(
            short = 'a',
            long,
            conflicts_with = "packages",
            help = "Clean all user packages; also global packages with -g"
        )]
        all: bool,
        #[arg(
            short = 'k',
            long,
            help = "Also remove download cache for deleted versions"
        )]
        cache: bool,
    },
    /// Prevent updates of installed packages
    Hold {
        #[arg(required=true,num_args=1..)]
        packages: Vec<String>,
    },
    /// Allow updates of held packages
    Unhold {
        #[arg(required=true,num_args=1..)]
        packages: Vec<String>,
    },
    /// Write a Scoop-compatible Scoopfile to standard output
    Export {
        #[arg(
            short = 'c',
            long,
            help = "Include Scoop configuration in the exported Scoopfile"
        )]
        config: bool,
    },
    /// Import a Scoopfile from a file or URL
    Import { file: String },
    /// Open a package's homepage
    Home { package: String },
    /// Manage Scoop-compatible custom command aliases
    Alias {
        #[command(subcommand)]
        action: AliasCommand,
    },
    /// Diagnose common Scoop installation problems
    Checkup,
    /// Download, hash and interactively create a package manifest
    Create {
        url: Option<String>,
        output: Option<String>,
    },
    /// Add, remove, inspect and select shim targets
    Shim {
        #[command(subcommand)]
        action: ShimCommand,
    },
    /// Query VirusTotal reports for packages and their dependencies
    Virustotal {
        #[arg(required_unless_present = "all", num_args = 1..)]
        packages: Vec<String>,
        #[arg(
            short = 'a',
            long,
            conflicts_with = "packages",
            help = "Check all installed packages"
        )]
        all: bool,
        #[arg(
            short = 's',
            long,
            help = "Submit a URL for analysis when no report exists"
        )]
        scan: bool,
        #[arg(short = 'n', long, help = "Skip dependency reports")]
        no_depends: bool,
    },
    #[command(name = "_fetch", hide = true)]
    Fetch { request: PathBuf },
    #[command(name = "_hook", hide = true)]
    Hook { request: PathBuf },
    #[command(skip)]
    Custom(Vec<String>),
}
#[derive(Subcommand)]
pub enum AliasCommand {
    /// Add an alias with an optional one-line description
    Add {
        name: String,
        command: String,
        description: Option<String>,
    },
    /// Remove an alias
    Rm { name: String },
    /// List alias names and commands
    List {
        #[arg(short = 'v', long, help = "Include alias descriptions")]
        verbose: bool,
    },
}
impl AliasCommand {
    fn args(self) -> Vec<String> {
        match self {
            Self::Add {
                name,
                command,
                description,
            } => {
                let mut args = vec!["add".into(), name, command];
                args.extend(description);
                args
            }
            Self::Rm { name } => vec!["rm".into(), name],
            Self::List { verbose } => {
                let mut args = vec!["list".into()];
                if verbose {
                    args.push("--verbose".into());
                }
                args
            }
        }
    }
}
#[derive(Subcommand)]
pub enum ShimCommand {
    /// Create a shim; arguments after the target are forwarded to it
    Add {
        name: String,
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Remove named shims
    Rm {
        #[arg(required = true, num_args = 1..)]
        names: Vec<String>,
    },
    /// List shims, optionally filtered by name expressions
    List { filters: Vec<String> },
    /// Show one shim's target
    Info { name: String },
    /// Activate an alternative target
    Alter {
        name: String,
        alternative: Option<String>,
    },
}
impl ShimCommand {
    fn args(self) -> Vec<String> {
        match self {
            Self::Add { name, target, args } => {
                let mut result = vec!["add".into(), name, target, "--".into()];
                result.extend(args);
                result
            }
            Self::Rm { names } => {
                let mut result = vec!["rm".into()];
                result.extend(names);
                result
            }
            Self::List { filters } => {
                let mut result = vec!["list".into()];
                result.extend(filters);
                result
            }
            Self::Info { name } => vec!["info".into(), name],
            Self::Alter { name, alternative } => {
                let mut result = vec!["alter".into(), name];
                result.extend(alternative);
                result
            }
        }
    }
}
impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Install { .. } => "install",
            Self::Uninstall { .. } => "uninstall",
            Self::Update { .. } => "update",
            Self::Status { .. } => "status",
            Self::Reset { .. } => "reset",
            Self::Cleanup { .. } => "cleanup",
            Self::Hold { .. } => "hold",
            Self::Unhold { .. } => "unhold",
            Self::Export { .. } => "export",
            Self::Import { .. } => "import",
            Self::Home { .. } => "home",
            Self::Alias { .. } => "alias",
            Self::Checkup => "checkup",
            Self::Create { .. } => "create",
            Self::Shim { .. } => "shim",
            Self::Virustotal { .. } => "virustotal",
            Self::Fetch { .. } => "_fetch",
            Self::Hook { .. } => "_hook",
            Self::Custom(..) => "alias",
        }
    }
}
fn targets(mut packages: Vec<String>, all: bool) -> Result<Vec<String>> {
    if all {
        packages = vec!["*".into()];
    }
    if packages.is_empty() {
        bail!("A package name or '*' is required");
    }
    Ok(packages)
}
pub fn show(out: &Output, result: Outcome) -> Result<bool> {
    if out.json {
        out.data(&result.rows, &result.warnings, &result.errors)?;
    } else {
        let has_version = result.rows.iter().any(|row| row["version"].is_string());
        let mut headers = vec!["Package / bucket"];
        if has_version {
            headers.push("Version");
        }
        headers.push("Result");
        out.table(
            &headers,
            result
                .rows
                .iter()
                .map(|row| {
                    let mut values = vec![
                        row.get("package")
                            .or_else(|| row.get("bucket"))
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                            .to_owned(),
                    ];
                    if has_version {
                        values.push(rsc_core::presentation::version_scope(
                            row["version"].as_str().unwrap_or("-"),
                            row["scope"].as_str().unwrap_or(""),
                        ));
                    }
                    values.push(row["result"].as_str().unwrap_or("-").to_owned());
                    values
                })
                .collect(),
        );
        out.warnings(&result.warnings);
        for error in &result.errors {
            out.error(&anyhow::anyhow!("{error}"));
        }
    }
    Ok(result.errors.is_empty())
}
pub async fn run(
    command: Command,
    config: &Config,
    arch: Architecture,
    global: bool,
    out: &Output,
) -> Result<bool> {
    let report = Progress::reporter(out.json);
    let result = match command {
        Command::Install {
            packages,
            independent,
            no_cache,
            skip_hash_check,
        } => {
            manager::install(
                config,
                &packages,
                arch,
                InstallOptions {
                    global,
                    independent,
                    no_cache,
                    skip_hash: skip_hash_check,
                    ..Default::default()
                },
                report,
            )
            .await?
        }
        Command::Uninstall { packages, purge } => {
            manager::lifecycle(config, "uninstall", &packages, global, purge, false)?
        }
        Command::Update {
            packages,
            force,
            independent,
            no_cache,
            skip_hash_check,
            all,
        } => {
            if packages.is_empty() && !all && (global || no_cache) {
                bail!("--global and --no-cache require a package name");
            }
            let packages = if all { vec!["*".into()] } else { packages };
            manager::update(
                config,
                &packages,
                arch,
                InstallOptions {
                    global,
                    force,
                    updating: true,
                    independent,
                    no_cache,
                    skip_hash: skip_hash_check,
                },
                report,
            )
            .await?
        }
        Command::Reset { packages, all } => manager::lifecycle(
            config,
            "reset",
            &targets(packages, all)?,
            global,
            false,
            false,
        )?,
        Command::Cleanup {
            packages,
            all,
            cache,
        } => manager::lifecycle(
            config,
            "cleanup",
            &targets(packages, all)?,
            global,
            false,
            cache,
        )?,
        Command::Hold { packages } => manager::hold(config, &packages, global, true)?,
        Command::Unhold { packages } => manager::hold(config, &packages, global, false)?,
        Command::Export {
            config: with_config,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&manager::export(config, with_config)?)?
            );
            return Ok(true);
        }
        Command::Import { file } => manager::import(config, &file, report).await?,
        Command::Home { package } => {
            let p = manager::resolve(config, &package).await?;
            let homepage = util::ci_get(&p.manifest.raw, "homepage")
                .and_then(Value::as_str)
                .context("Manifest has no homepage")?;
            native::invoke(config, "home", json!({"url":homepage}))?;
            Outcome {
                rows: vec![json!({"package":p.name,"result":"homepage opened"})],
                ..Default::default()
            }
        }
        Command::Status { local } => {
            let result = manager::statuses(config, local)?;
            if out.json {
                out.data(
                    &json!({"packages":result.rows,"bucket_updates":result.bucket_updates}),
                    &result.warnings,
                    &result.errors,
                )?;
            } else {
                out.status_updates(&result.bucket_updates);
                out.table(
                    &[
                        "Package",
                        "Installed",
                        "Available",
                        "Scope",
                        "State",
                        "Dependencies",
                    ],
                    result
                        .rows
                        .iter()
                        .map(|v| {
                            let state = ["failed", "hold", "deprecated", "removed", "outdated"]
                                .into_iter()
                                .filter(|key| {
                                    v[*key].as_bool() == Some(true)
                                        || v[*key].as_str().is_some_and(|s| !s.is_empty())
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            vec![
                                v["package"].as_str().unwrap_or("-").into(),
                                v["version"].as_str().unwrap_or("-").into(),
                                v["latest_version"].as_str().unwrap_or("-").into(),
                                v["scope"].as_str().unwrap_or("-").into(),
                                if state.is_empty() {
                                    "current".into()
                                } else {
                                    state
                                },
                                v["missing_deps"]
                                    .as_array()
                                    .map(|a| {
                                        a.iter()
                                            .filter_map(Value::as_str)
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default(),
                            ]
                        })
                        .collect(),
                );
                out.status_diagnostics(&result.warnings, &result.errors);
            }
            return Ok(result.errors.is_empty());
        }
        Command::Hook { request } => {
            rsc_core::native::lifecycle::hook_helper(config, &util::read_json(&request)?)?;
            return Ok(true);
        }
        Command::Fetch { request } => {
            let value = util::read_json(&request)?;
            let urls = value["url"].as_str().context("Fetch request has no URL")?;
            let mut downloader = Downloader::new(config)?;
            let temporary = if value["use_cache"].as_bool() == Some(false) {
                Some(tempfile::tempdir()?)
            } else {
                None
            };
            if let Some(temp) = &temporary {
                downloader = downloader.cache_directory(temp.path().to_owned());
            }
            let manifest = rsc_core::manifest::Manifest::parse(
                json!({"version":"bridge","cookie":value["cookie"]}).to_string(),
            )?;
            let file = downloader
                .download(
                    Task {
                        id: 0,
                        label: value["app"].as_str().unwrap_or("download").into(),
                        app: value["app"].as_str().unwrap_or("rsc").into(),
                        version: value["version"].as_str().unwrap_or("bridge").into(),
                        file: DownloadFile {
                            url: urls.into(),
                            hash: None,
                        },
                        headers: download::headers(&manifest, arch)?,
                    },
                    report,
                )
                .await?;
            if let Some(target) = value["to"].as_str().filter(|s| !s.is_empty()) {
                fs::copy(file.path, target)?;
            }
            return Ok(true);
        }
        Command::Create { url, output } => {
            let mut args = Vec::new();
            args.extend(url);
            args.extend(output);
            native::commands::create(config, &args, report)?;
            return Ok(true);
        }
        other => {
            let (name, mut args) = match other {
                Command::Alias { action } => ("alias".to_owned(), action.args()),
                Command::Checkup => ("checkup".into(), Vec::new()),
                Command::Shim { action } => ("shim".into(), action.args()),
                Command::Virustotal {
                    packages,
                    all,
                    scan,
                    no_depends,
                } => {
                    let mut args = packages;
                    if all {
                        args.push("--all".into());
                    }
                    if scan {
                        args.push("--scan".into());
                    }
                    if no_depends {
                        args.push("--no-depends".into());
                    }
                    ("virustotal".into(), args)
                }
                Command::Custom(mut args) => {
                    if args.is_empty() {
                        bail!("Command missing");
                    }
                    let name = args.remove(0);
                    util::valid_name(&name)?;
                    (name, args)
                }
                _ => unreachable!(),
            };
            if out.json {
                bail!("JSON output is not yet implemented for {name}");
            }
            if global {
                args.insert(0, "--global".into());
            }
            let value = native::invoke(config, "command", json!({"command":name,"args":args}))?;
            if let Some(rows) = value.as_array().filter(|a| !a.is_empty()) {
                if let Some(first) = rows[0].as_object() {
                    let headers = first.keys().map(String::as_str).collect::<Vec<_>>();
                    out.table(
                        &headers,
                        rows.iter()
                            .map(|r| {
                                headers
                                    .iter()
                                    .map(|k| {
                                        r.get(*k)
                                            .map(|v| {
                                                v.as_str()
                                                    .map(str::to_owned)
                                                    .unwrap_or_else(|| v.to_string())
                                            })
                                            .unwrap_or_default()
                                    })
                                    .collect()
                            })
                            .collect(),
                    );
                } else {
                    for row in rows {
                        let text = row
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| row.to_string());
                        println!(
                            "{}",
                            rsc_core::presentation::paint(
                                &text,
                                rsc_core::presentation::state_tone(&text),
                                rsc_core::presentation::stdout_color()
                            )
                        );
                    }
                }
            }
            return Ok(true);
        }
    };
    show(out, result)
}
