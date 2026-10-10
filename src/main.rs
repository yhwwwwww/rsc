mod commands;
mod output;
use anyhow::{Context, Result, bail};
use clap::{Arg, ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand};
use output::{CacheEntry, Output, Progress};
use rsc_core::{
    bucket::{self, Resolved},
    config::Config,
    download::{self, Downloader, Task},
    manifest::Architecture,
    package, util,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};
use tokio::{sync::Semaphore, task::JoinSet};

#[derive(Parser)]
#[command(
    name = "rsc",
    version = rsc_core::VERSION,
    about = "A single-binary package manager for your Scoop directories",
    after_help = "Scoop-compatible package operations. Built-in parallel downloads."
)]
struct Cli {
    #[arg(skip)]
    global: bool,
    #[arg(skip)]
    arch: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Manage(commands::Command),
    /// Search local bucket names and executable aliases
    #[command(
        long_about = "Search names and top-level executable aliases, using a case-insensitive regex by default (Scoop behavior). Installed version colors: blue = current, yellow = outdated/unknown, magenta = newer, red = broken.",
        after_help = "Examples:\n  rsc search git\n  rsc search '^git$'\n  rsc search -N git\n  rsc search -e 'c++'\n  rsc search -D editor\n\nSearch options apply to local buckets. Omit QUERY to list all packages."
    )]
    Search {
        #[arg(help = "Search expression (case-insensitive regex; literal with -e)")]
        query: Option<String>,
        #[arg(
            short = 'e',
            long,
            help = "Match literal text instead of a regular expression"
        )]
        explicit: bool,
        #[arg(
            short = 'N',
            long,
            help = "Search package names only; skip binary and alias matching",
            conflicts_with = "with_description"
        )]
        name_only: bool,
        #[arg(
            short = 'D',
            long,
            help = "Also search descriptions and show them in the results"
        )]
        with_description: bool,
    },
    /// List installed packages and identify incomplete installations
    List { filter: Option<String> },
    /// Show manifest, dependency and installation details
    Info {
        package: String,
        #[arg(
            short = 'v',
            long,
            help = "Include binaries, architecture, dependencies and notes"
        )]
        verbose: bool,
    },
    /// Print the original manifest
    Cat { package: String },
    /// Print the active installation directory
    Prefix { package: String },
    /// Find a command and resolve Scoop executable shims
    Which { command: String },
    /// Show settings and their sources, or set a value
    Config {
        key: Option<String>,

        value: Option<String>,
    },
    /// List local buckets or known Scoop bucket endpoints
    Bucket {
        #[command(subcommand)]
        action: Option<BucketCommand>,
    },
    /// List dependencies in installation order
    Depends { package: String },
    /// Download package files into the Scoop cache without installing
    Download {
        #[arg(required=true,num_args=1..)]
        packages: Vec<String>,
        #[arg(
            short = 'f',
            long,
            help = "Download again instead of reusing validated cache files"
        )]
        force: bool,
        #[arg(short = 's', long, help = "Skip manifest hash verification")]
        skip_hash_check: bool,
    },
    #[command(external_subcommand)]
    Custom(Vec<String>),
    /// Inspect or remove cached downloads
    Cache {
        #[command(subcommand)]
        action: Option<CacheCommand>,
    },
}
#[derive(Subcommand)]
enum CacheCommand {
    /// List cached downloads, optionally filtered by package
    Show { packages: Vec<String> },
    /// Remove cached downloads
    Rm {
        #[arg(required_unless_present = "all", num_args = 1..)]
        packages: Vec<String>,
        #[arg(
            short = 'a',
            long,
            conflicts_with = "packages",
            help = "Remove all cached downloads"
        )]
        all: bool,
    },
}
#[derive(Subcommand)]
enum BucketCommand {
    List,
    Add {
        name: String,
        repository: Option<String>,
    },
    Rm {
        name: String,
    },
    /// Show known bucket endpoints
    Known,
}
impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Manage(command) => command.name(),
            Self::Search { .. } => "search",
            Self::List { .. } => "list",
            Self::Info { .. } => "info",
            Self::Cat { .. } => "cat",
            Self::Prefix { .. } => "prefix",
            Self::Which { .. } => "which",
            Self::Config { .. } => "config",
            Self::Bucket { .. } => "bucket",
            Self::Depends { .. } => "depends",
            Self::Download { .. } => "download",
            Self::Cache { .. } => "cache",
            Self::Custom(..) => "alias",
        }
    }
}
const SCOPE_COMMANDS: &[&str] = &[
    "install",
    "uninstall",
    "update",
    "cleanup",
    "hold",
    "unhold",
    "list",
    "prefix",
    "which",
    "shim",
];
const ARCH_COMMANDS: &[&str] = &["install", "download", "depends"];
impl Cli {
    fn configured_command() -> clap::Command {
        use clap::builder::styling::{Ansi256Color, AnsiColor, Styles};
        let styles = Styles::styled()
            .header(Ansi256Color(117).on_default().bold())
            .usage(Ansi256Color(117).on_default().bold())
            .literal(AnsiColor::BrightMagenta.on_default().bold())
            .placeholder(AnsiColor::Magenta.on_default());
        Self::command()
            .styles(styles)
            .mut_subcommands(|mut command| {
                let name = command.get_name().to_owned();
                if SCOPE_COMMANDS.contains(&name.as_str()) {
                    let mut scope = Arg::new("global")
                        .short('g')
                        .long("global")
                        .action(ArgAction::SetTrue)
                        .global(true)
                        .help(match name.as_str() {
                            "shim" => "Use global shims; list only global shims",
                            "list" => "List global installations only",
                            "which" => "Search the global shim directory only",
                            "prefix" => "Select the global installation",
                            "install" => "Install globally (requires administrator rights)",
                            "cleanup" => {
                                "Clean global installations (requires administrator rights)"
                            }
                            _ => "Use globally installed packages",
                        });
                    if name == "update" {
                        scope = scope.requires("package_selection");
                    }
                    command = command.arg(scope);
                }
                if ARCH_COMMANDS.contains(&name.as_str()) {
                    command = command.arg(
                        Arg::new("arch")
                            .short('a')
                            .long("arch")
                            .value_name("ARCH")
                            .help("Choose manifest architecture: 64bit, 32bit or arm64"),
                    );
                }
                command
            })
    }

    fn from_configured_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let mut cli = Self::from_arg_matches(matches)?;
        if let Some((_, args)) = matches.subcommand() {
            cli.global = args
                .try_get_one::<bool>("global")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(false);
            cli.arch = args.try_get_one::<String>("arch").ok().flatten().cloned();
        }
        Ok(cli)
    }
    fn parse_configured() -> Self {
        let matches = Self::configured_command().get_matches();
        Self::from_configured_matches(&matches).unwrap_or_else(|error| error.exit())
    }
}
fn main() -> ExitCode {
    match rsc_core::shim::dispatch() {
        Ok(Some(code)) => std::process::exit(code),
        Ok(None) => {}
        Err(error) => {
            eprintln!("error: {error:#}");
            return ExitCode::FAILURE;
        }
    }
    let cli = Cli::parse_configured();
    let out = Output::new(false, cli.command.name());
    let execute = || -> Result<bool> {
        // Queries and shim dispatch do not need an eagerly created worker pool.
        let mut runtime = match &cli.command {
            Command::Download { .. }
            | Command::Manage(
                commands::Command::Install { .. }
                | commands::Command::Update { .. }
                | commands::Command::Fetch { .. },
            ) => {
                let mut b = tokio::runtime::Builder::new_multi_thread();
                b.worker_threads(4);
                b
            }
            _ => tokio::runtime::Builder::new_current_thread(),
        };
        runtime.enable_all().build()?.block_on(run(cli, &out))
    };
    match execute() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            out.error(&error);
            if let Some(status) = error.downcast_ref::<rsc_core::native::CommandFailure>() {
                std::process::exit(status.0);
            }
            ExitCode::FAILURE
        }
    }
}
async fn resolve(config: &Config, input: &str) -> Result<Resolved> {
    rsc_core::manager::resolve(config, input).await
}
async fn run(cli: Cli, out: &Output) -> Result<bool> {
    let config = Config::load()?;
    let uses_architecture = matches!(
        cli.command,
        Command::Download { .. }
            | Command::Depends { .. }
            | Command::Manage(commands::Command::Install { .. } | commands::Command::Update { .. })
    );
    let arch = if !uses_architecture {
        Architecture::native()
    } else if let Some(arch) = cli.arch.as_deref() {
        Architecture::parse(arch)?
    } else if let Some(value) = config.get("default_architecture").filter(|v| !v.is_null()) {
        let value = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        match Architecture::parse(&value) {
            Ok(arch) => arch,
            Err(_) => {
                rsc_core::presentation::warning(
                    "invalid default_architecture; using the machine architecture",
                );
                Architecture::native()
            }
        }
    } else {
        Architecture::native()
    };
    match cli.command {
        Command::Custom(args) => {
            return commands::run(
                commands::Command::Custom(args),
                &config,
                arch,
                cli.global,
                out,
            )
            .await;
        }
        Command::Manage(command) => {
            return commands::run(command, &config, arch, cli.global, out).await;
        }
        Command::List { filter } => {
            let installed = package::list(&config.layout, cli.global)?;
            let matching = rsc_core::native::invoke(
                &config,
                "match",
                json!({"query":filter.unwrap_or_default(),"values":installed.iter().map(|p|p.name.clone()).collect::<Vec<_>>()}),
            )?;
            let rows: Vec<_> = installed
                .into_iter()
                .enumerate()
                .filter(|(i, _)| matching.get(*i).and_then(Value::as_bool) == Some(true))
                .map(|(_, p)| p)
                .collect();
            let warnings: Vec<String> = rows
                .iter()
                .filter_map(|p| {
                    p.error
                        .as_ref()
                        .map(|e| format!("{} ({}): {e}", p.name, p.scope))
                })
                .collect();
            if out.json {
                out.data(&rows, &warnings, &[])?;
            } else {
                out.table(
                    &["Package", "Version", "Bucket", "Arch", "State"],
                    rows.iter()
                        .map(|p| {
                            vec![
                                p.name.clone(),
                                rsc_core::presentation::version_scope(
                                    p.version.as_deref().unwrap_or("?"),
                                    &p.scope,
                                ),
                                p.bucket.clone().unwrap_or_else(|| "-".into()),
                                p.architecture.clone().unwrap_or_else(|| "-".into()),
                                p.state.clone(),
                            ]
                        })
                        .collect(),
                );
                out.warnings(&warnings);
            }
        }
        Command::Search {
            query,
            explicit,
            name_only,
            with_description,
        } => {
            let query = query.unwrap_or_default();
            let options = rsc_core::search::Options {
                explicit,
                name_only,
                with_description,
            };
            let report = rsc_core::search::local_with_options(&config, &query, options)?;
            let rows = report.rows;
            out.warnings(&report.warnings);
            if rows.is_empty() {
                if !options.is_default() {
                    bail!("No local matches found");
                }
                let remote =
                    rsc_core::native::invoke(&config, "search_remote", json!({"query":query}))?;
                let matches = remote["rows"]
                    .as_array()
                    .context("Invalid remote search result")?;
                if !matches.is_empty() {
                    eprintln!(
                        "Matches in other known buckets; add one with rsc bucket add <bucket>"
                    );
                    out.table(
                        &["Package", "Bucket"],
                        matches
                            .iter()
                            .map(|r| vec![string(r, "Name"), string(r, "Source")])
                            .collect(),
                    );
                    return Ok(true);
                }
                if remote["limited"].as_bool() == Some(true) {
                    return Ok(true);
                }
                bail!("No matches found");
            }
            out.search_table(&rows, with_description);
        }
        Command::Info {
            package: input,
            verbose,
        } => {
            let value = rsc_core::native::invoke(
                &config,
                "info",
                json!({"input":input,"verbose":verbose}),
            )?;
            let rows = value.as_array().context("Invalid package information")?;
            for row in rows {
                if let Some(fields) = row.as_object() {
                    out.details(
                        fields
                            .iter()
                            .map(|(key, value)| {
                                (
                                    key.clone(),
                                    if key == "Installed" {
                                        value
                                            .as_array()
                                            .into_iter()
                                            .flatten()
                                            .filter_map(Value::as_str)
                                            .collect::<Vec<_>>()
                                            .join(" | ")
                                    } else {
                                        value
                                            .as_str()
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| value.to_string())
                                    },
                                )
                            })
                            .collect(),
                    );
                }
            }
        }
        Command::Cat { package } => {
            let resolved = resolve(&config, &package).await?;
            rsc_core::native::invoke(&config, "cat", json!({"manifest":resolved.manifest.raw}))?;
        }
        Command::Prefix { package } => {
            let installed = package::find(&config.layout, &package, cli.global)?;
            if out.json {
                out.data(&json!({"package":installed.name,"path":installed.path,"scope":installed.scope}),&[],&[])?;
            } else {
                println!("{}", installed.path.display());
            }
        }
        Command::Which { command } => {
            let (path, target) = which(&config, &command, cli.global)?;
            if out.json {
                out.data(
                    &json!({"command":command,"launcher":path,"target":target}),
                    &[],
                    &[],
                )?;
            } else {
                println!("{}", target.display());
            }
        }
        Command::Config { key, value } => {
            if key.as_deref() == Some("rm") {
                let key = value.context("Configuration name is required after rm")?;
                config.set(&key, None)?;
                println!(
                    "{}",
                    rsc_core::presentation::paint(
                        &format!("Removed {key}"),
                        rsc_core::presentation::Tone::Success,
                        rsc_core::presentation::stdout_color()
                    )
                );
                return Ok(true);
            }
            if let Some(value) = value {
                let key = key.context("A key is required before a configuration value")?;
                let file = config.set(&key, Some(&value))?;
                if out.json {
                    out.data(&json!({"key":key,"file":file,"changed":true}), &[], &[])?;
                } else {
                    println!(
                        "{} {}",
                        rsc_core::presentation::paint(
                            &format!("Saved {key} in"),
                            rsc_core::presentation::Tone::Success,
                            rsc_core::presentation::stdout_color()
                        ),
                        rsc_core::presentation::paint(
                            &file.display().to_string(),
                            rsc_core::presentation::Tone::Secondary,
                            rsc_core::presentation::stdout_color()
                        )
                    );
                }
            } else {
                let settings = config.settings();
                let settings: Vec<_> = if let Some(key) = key {
                    let setting = settings
                        .into_iter()
                        .find(|s| s.key.eq_ignore_ascii_case(&key))
                        .with_context(|| format!("Configuration key {key} is not set"))?;
                    vec![setting]
                } else {
                    settings
                };
                if out.json {
                    out.data(&settings, &[], &[])?;
                } else {
                    out.table(
                        &["Setting", "Value", "Source"],
                        settings
                            .iter()
                            .map(|s| {
                                vec![
                                    s.key.clone(),
                                    s.value
                                        .as_str()
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| s.value.to_string()),
                                    s.source.clone(),
                                ]
                            })
                            .collect(),
                    );
                }
            }
        }
        Command::Bucket { action } => match action {
            Some(BucketCommand::Add { name, repository }) => {
                let row = rsc_core::manager::add_bucket(&config, &name, repository.as_deref())?;
                return commands::show(
                    out,
                    rsc_core::manager::Outcome {
                        rows: vec![row],
                        ..Default::default()
                    },
                );
            }
            Some(BucketCommand::Rm { name }) => {
                let row = rsc_core::manager::remove_bucket(&config, &name)?;
                return commands::show(
                    out,
                    rsc_core::manager::Outcome {
                        rows: vec![row],
                        ..Default::default()
                    },
                );
            }
            Some(BucketCommand::Known) => {
                let known: Value = rsc_core::native::known::json();
                if out.json {
                    out.data(&known, &[], &[])?;
                } else {
                    out.table(
                        &["Bucket", "URL"],
                        known
                            .as_object()
                            .context("Invalid embedded bucket list")?
                            .iter()
                            .map(|(k, v)| vec![k.clone(), v.as_str().unwrap_or("").into()])
                            .collect(),
                    );
                }
            }
            None | Some(BucketCommand::List) => {
                let buckets = bucket::buckets(&config.layout.buckets())?;
                if out.json {
                    out.data(&buckets, &[], &[])?;
                } else {
                    out.table(
                        &["Bucket", "Manifests", "Remote", "Directory"],
                        buckets
                            .iter()
                            .map(|b| {
                                vec![
                                    b.name.clone(),
                                    b.manifests.to_string(),
                                    b.remote.clone().unwrap_or_else(|| "local".into()),
                                    b.path.display().to_string(),
                                ]
                            })
                            .collect(),
                    );
                }
            }
        },
        Command::Depends { package } => {
            let plans = rsc_core::manager::plan(&config, &[package], arch, false).await?;
            let installed = package::list(&config.layout, false)?;
            let rows=plans.into_iter().map(|(p,_,version)|{
                let existing=installed.iter().find(|i|i.name.eq_ignore_ascii_case(&p.name) && i.error.is_none())
                    .map(|i|rsc_core::presentation::version_scope(i.version.as_deref().unwrap_or("?"), &i.scope)).unwrap_or_else(||"no".into());
                json!({"package":format!("{}/{}",p.bucket.as_deref().unwrap_or("local"),p.name),"version":version,"installed":existing,"depth":0})
            }).collect::<Vec<_>>();
            if out.json {
                out.data(&rows, &[], &[])?;
            } else {
                out.table(
                    &["Package", "Version", "Installed"],
                    rows.iter()
                        .map(|r| {
                            vec![
                                format!(
                                    "{}{}",
                                    "  ".repeat(r["depth"].as_u64().unwrap_or(0) as usize),
                                    string(r, "package")
                                ),
                                string(r, "version"),
                                string(r, "installed"),
                            ]
                        })
                        .collect(),
                );
            }
        }
        Command::Download {
            packages,
            force,
            skip_hash_check,
        } => {
            let downloader = Downloader::new(&config)?.force(force);
            let report = Progress::reporter(out.json);
            let mut tasks = Vec::new();
            let mut failures = Vec::new();
            let mut destinations = BTreeSet::new();
            for input in packages {
                let prepared = async {
                    let p = resolve(&config, &input).await?;
                    let (arch, _, nightly) = rsc_core::manager::prepare(&config, &p, arch)?;
                    let version = if p.manifest.version()? == "nightly" {
                        nightly
                    } else {
                        p.manifest.version()?.into()
                    };
                    let mut files = p.manifest.downloads(arch)?;
                    if skip_hash_check || p.manifest.version()? == "nightly" {
                        for file in &mut files {
                            file.hash = None;
                        }
                    }
                    let headers = download::headers(&p.manifest, arch)?;
                    for (index, file) in files.into_iter().enumerate() {
                        let path = download::cache_path(
                            &config.layout.cache,
                            &p.name,
                            &version,
                            &file.url,
                        )?;
                        if destinations.insert(path.to_string_lossy().to_ascii_lowercase()) {
                            tasks.push(Task {
                                id: tasks.len(),
                                label: format!("{} [{}]", p.name, index + 1),
                                app: p.name.clone(),
                                version: version.clone(),
                                file,
                                headers: headers.clone(),
                            });
                        }
                    }
                    Ok::<_, anyhow::Error>(())
                }
                .await;
                if let Err(error) = prepared {
                    failures.push(format!("{}: {error:#}", util::redact_url(&input)));
                }
            }
            let semaphore = Arc::new(Semaphore::new(downloader.concurrency()));
            let mut jobs = JoinSet::new();
            for task in tasks {
                let semaphore = semaphore.clone();
                let downloader = downloader.clone();
                let report = report.clone();
                jobs.spawn(async move {
                    let _permit = semaphore.acquire_owned().await?;
                    let label = task.label.clone();
                    downloader
                        .download(task, report)
                        .await
                        .with_context(|| label)
                });
            }
            let collect = async {
                let mut downloaded = Vec::new();
                while let Some(result) = jobs.join_next().await {
                    match result {
                        Ok(Ok(file)) => downloaded.push(file),
                        Ok(Err(error)) => failures.push(format!("{error:#}")),
                        Err(error) => failures.push(error.to_string()),
                    }
                }
                downloaded
                    .sort_by(|a, b| a.package.cmp(&b.package).then_with(|| a.path.cmp(&b.path)));
                (downloaded, failures)
            };
            let (downloaded, failures) = tokio::select! {
                result=collect=>result,
                _=tokio::signal::ctrl_c()=>bail!("Download cancelled. Validated segmented downloads retain resumable parts; run the same command again."),
            };
            if out.json {
                out.data(&downloaded, &[], &failures)?;
            } else {
                out.table(
                    &["Package", "Size", "Cache", "Integrity"],
                    downloaded
                        .iter()
                        .map(|d| {
                            vec![
                                d.package.clone(),
                                output::size(d.bytes),
                                if d.cached { "reused" } else { "saved" }.into(),
                                if d.verified { "verified" } else { "no hash" }.into(),
                            ]
                        })
                        .collect(),
                );
                for failure in &failures {
                    out.error(&anyhow::anyhow!("{failure}"));
                }
                eprintln!(
                    "{} file(s) available; {} failure(s)",
                    downloaded.len(),
                    failures.len()
                );
            }
            return Ok(failures.is_empty());
        }
        Command::Cache { action } => {
            let (remove, mut packages, all) = match action {
                Some(CacheCommand::Rm { packages, all }) => (true, packages, all),
                Some(CacheCommand::Show { packages }) => (false, packages, false),
                None => (false, Vec::new(), false),
            };
            if all {
                packages = vec!["*".into()];
            }
            if remove {
                if packages.is_empty() {
                    bail!("A package name or '*' is required");
                }
                let pattern = if packages.iter().any(|s| s == "*") {
                    "*".into()
                } else {
                    packages.join("|")
                };
                let rows = rsc_core::manager::clean_cache(&config, &pattern, None)?;
                println!(
                    "{}",
                    rsc_core::presentation::paint(
                        &format!("Removed {} cache file(s)", rows.len()),
                        rsc_core::presentation::Tone::Success,
                        rsc_core::presentation::stdout_color()
                    )
                );
            } else {
                let query = format!(
                    "^(?:{})#",
                    if packages.is_empty() || packages.iter().any(|s| s == "*") {
                        ".*".into()
                    } else {
                        packages.join("|")
                    }
                );
                let entries = cache_entries(&config.layout.cache, None)?;
                let matching = rsc_core::native::invoke(
                    &config,
                    "match",
                    json!({"query":query,"values":entries.iter().map(|r|&r.name).collect::<Vec<_>>()}),
                )?;
                let matches = matching
                    .as_array()
                    .context("Invalid cache matching result")?;
                let rows = entries
                    .into_iter()
                    .zip(matches)
                    .filter_map(|(row, matched)| (matched.as_bool() == Some(true)).then_some(row))
                    .collect::<Vec<_>>();
                out.table(
                    &["Cache file", "Size"],
                    rows.iter()
                        .map(|r| vec![r.name.clone(), output::size(r.bytes)])
                        .collect(),
                );
                println!(
                    "{} file(s), {}",
                    rows.len(),
                    output::size(rows.iter().map(|r| r.bytes).sum())
                );
            }
        }
    }
    Ok(true)
}
fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_owned()
}
fn which(config: &Config, name: &str, global: bool) -> Result<(PathBuf, PathBuf)> {
    util::valid_name(name)?;
    let mut directories = Vec::new();
    if global {
        directories.push(config.layout.shims(true));
    } else {
        if let Some(value) = std::env::var_os(&config.layout.path_variable) {
            directories.extend(std::env::split_paths(&value));
        }
        if config.layout.path_variable != "PATH" {
            if let Some(value) = std::env::var_os("PATH") {
                directories.extend(std::env::split_paths(&value));
            }
        }
        directories.push(config.layout.shims(false));
        directories.push(config.layout.shims(true));
    }
    let extensions = if Path::new(name).extension().is_some() {
        vec!["".to_owned()]
    } else {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        std::iter::once(String::new())
            .chain(
                pathext
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            )
            .chain(std::iter::once(".ps1".into()))
            .collect()
    };
    for directory in directories {
        for extension in &extensions {
            let path = directory.join(format!("{name}{extension}"));
            if !path.is_file() {
                continue;
            }
            let shim = path.with_extension("shim");
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                && shim.is_file()
            {
                let text = util::read_text(&shim)?;
                let target = text
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once('=')?;
                        key.trim()
                            .eq_ignore_ascii_case("path")
                            .then(|| value.trim().trim_matches('"'))
                    })
                    .filter(|s| !s.is_empty())
                    .context("Shim has no target path")?;
                let target = PathBuf::from(target);
                let target = if target.is_absolute() {
                    target
                } else {
                    directory.join(target)
                };
                if !target.is_file() {
                    bail!("Shim target is missing: {}", target.display());
                }
                return Ok((path, target));
            }
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("ps1"))
            {
                let text = util::read_text(&path)?;
                if let Some(target) = text
                    .lines()
                    .next()
                    .and_then(|line| {
                        line.strip_prefix("@rem ")
                            .or_else(|| line.strip_prefix("# "))
                    })
                    .map(|s| PathBuf::from(s.trim()))
                {
                    if target.is_absolute() && target.is_file() {
                        return Ok((path, target));
                    }
                }
            }
            return Ok((path.clone(), path));
        }
    }
    bail!("Command {name} was not found in PATH or Scoop shims")
}
fn cache_entries(root: &Path, package: Option<&str>) -> Result<Vec<CacheEntry>> {
    if let Some(package) = package {
        util::valid_name(package)?;
    }
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((app, _)) = name.split_once('#') else {
            continue;
        };
        if package.is_some_and(|p| !p.eq_ignore_ascii_case(app)) {
            continue;
        }
        rows.push(CacheEntry {
            name,
            path: entry.path(),
            bytes: entry.metadata()?.len(),
        });
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    #[test]
    fn direct_commands_parse_without_external_fallback() {
        let cases = [
            vec!["list"],
            vec!["search", "^rg$"],
            vec!["info", "git", "-v"],
            vec!["cat", "git"],
            vec!["prefix", "git"],
            vec!["which", "git"],
            vec!["config"],
            vec!["bucket", "known"],
            vec!["depends", "git"],
            vec!["download", "git"],
            vec!["cache", "rm", "--all"],
            vec!["install", "git"],
            vec!["uninstall", "git"],
            vec!["update", "*"],
            vec!["status", "--local"],
            vec!["reset", "git"],
            vec!["cleanup", "*"],
            vec!["hold", "git"],
            vec!["unhold", "git"],
            vec!["export"],
            vec!["import", "scoopfile.json"],
            vec!["home", "git"],
            vec!["alias", "list"],
            vec!["shim", "list"],
            vec!["checkup"],
            vec!["create", "https://example.com/a.zip"],
            vec!["virustotal", "git"],
        ];
        for args in cases {
            let cli = Cli::try_parse_from(std::iter::once("rsc").chain(args.iter().copied()))
                .unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert!(
                !matches!(cli.command, Command::Custom(_)),
                "{args:?} routed to alias"
            );
        }
    }
    #[test]
    fn search_help_and_options_describe_actual_search_controls() {
        let mut command = Cli::configured_command();
        command.build();
        let help = command
            .find_subcommand_mut("search")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(
            help.contains("--explicit")
                && help.contains("--name-only")
                && help.contains("--with-description")
        );
        assert!(!help.contains("--global") && !help.contains("--arch"));
        assert!(
            command
                .find_subcommand_mut("install")
                .unwrap()
                .render_long_help()
                .to_string()
                .contains("--global")
        );
        let matches = Cli::configured_command()
            .try_get_matches_from(["rsc", "search", "-e", "-D", "c++"])
            .unwrap();
        assert!(matches!(
            Cli::from_arg_matches(&matches).unwrap().command,
            Command::Search {
                explicit: true,
                with_description: true,
                name_only: false,
                ..
            }
        ));
        assert!(
            Cli::configured_command()
                .try_get_matches_from(["rsc", "search", "-N", "-D", "editor"])
                .is_err()
        );
        let matches = Cli::configured_command()
            .try_get_matches_from(["rsc", "install", "-g", "--arch", "arm64", "git"])
            .unwrap();
        let cli = Cli::from_configured_matches(&matches).unwrap();
        assert!(cli.global && cli.arch.as_deref() == Some("arm64"));
    }
    #[test]
    fn every_command_exposes_only_applicable_scope_and_architecture_options() {
        let mut command = Cli::configured_command();
        command.build();
        for sub in command.get_subcommands() {
            if matches!(sub.get_name(), "help" | "_fetch" | "_hook") {
                continue;
            }
            let ids = sub
                .get_arguments()
                .map(|arg| arg.get_id().as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                ids.contains(&"global"),
                SCOPE_COMMANDS.contains(&sub.get_name()),
                "{}",
                sub.get_name()
            );
            assert_eq!(
                ids.contains(&"arch"),
                ARCH_COMMANDS.contains(&sub.get_name()),
                "{}",
                sub.get_name()
            );
        }
        let rejected: &[&[&str]] = &[
            &["search", "git", "-g"],
            &["list", "--arch", "64bit"],
            &["status", "-g"],
            &["reset", "git", "-g"],
            &["export", "-g"],
            &["install", "git", "-u"],
            &["download", "git", "--no-update-scoop"],
            &["update", "--quiet"],
            &["update", "-f"],
            &["update", "-g"],
            &["checkup", "--anything"],
            &["create", "--arch", "64bit"],
            &["alias", "list", "--arch", "64bit"],
            &["alias", "rm", "sample", "-v"],
            &["virustotal", "git", "--passthru"],
            &["virustotal", "git", "-u"],
            &["cache", "show", "-a"],
            &["shim", "list", "--arch", "64bit"],
        ];
        for args in rejected {
            assert!(
                Cli::configured_command()
                    .try_get_matches_from(std::iter::once("rsc").chain(args.iter().copied()))
                    .is_err(),
                "{args:?}"
            );
        }
        for args in [
            vec!["install", "-a", "arm64", "git"],
            vec!["update", "-f", "-a"],
            vec!["update", "git", "-g"],
            vec!["depends", "-a", "32bit", "git"],
            vec!["alias", "list", "-v"],
            vec!["shim", "list", "-g"],
            vec!["cache", "rm", "-a"],
        ] {
            Cli::configured_command()
                .try_get_matches_from(std::iter::once("rsc").chain(args.iter().copied()))
                .unwrap_or_else(|error| panic!("{args:?}: {error}"));
        }
    }
    #[test]
    fn unknown_commands_are_custom_aliases() {
        let cli = Cli::try_parse_from(["rsc", "myalias", "one", "--flag"]).unwrap();
        assert!(matches!(cli.command,Command::Custom(args) if args==["myalias","one","--flag"]));
    }
}
