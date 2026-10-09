use super::{CommandFailure, lifecycle, query, scripts, windows};
use crate::{config::Config, manifest::Architecture, package, util};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::Path,
};
fn strings(d: &Value) -> Vec<String> {
    d["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
pub fn invoke(c: &Config, d: &Value) -> Result<Value> {
    let command = d["command"].as_str().unwrap_or("");
    let a = strings(d);
    match command {
        "alias" => alias(c, &a),
        "shim" => shim(c, &a),
        "checkup" => checkup(c, &a),
        "create" => create(c, &a, std::sync::Arc::new(|_| {})),
        "virustotal" => virustotal(c, &a),
        name => {
            util::valid_name(name)?;
            let aliases = c
                .get("alias")
                .and_then(Value::as_object)
                .context("Unknown command")?;
            let file = aliases
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .and_then(|(_, v)| v.as_str())
                .context("Unknown command")?;
            util::valid_name(file)?;
            let body = util::read_text(&c.layout.shims(false).join(format!("{file}.ps1")))?;
            scripts::run(c, &body, &Value::Null, &a)?;
            Ok(Value::Null)
        }
    }
}
fn alias(c: &Config, a: &[String]) -> Result<Value> {
    let sub = a
        .first()
        .map(String::as_str)
        .context("Use alias add, rm or list")?;
    let mut map = c
        .get("alias")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match sub {
        "add" => {
            let name = a.get(1).context("Alias name missing")?;
            util::valid_name(name)?;
            let body = a.get(2).context("Alias command missing")?;
            if map.keys().any(|n| n.eq_ignore_ascii_case(name)) {
                bail!("Alias already exists: {name}")
            }
            let file = format!("scoop-{name}");
            let path = c.layout.shims(false).join(format!("{file}.ps1"));
            if path.exists() {
                bail!("Alias file already exists")
            }
            let summary = a.get(3).map(String::as_str).unwrap_or("");
            if summary.contains(['\n', '\r']) {
                bail!("Alias description must be one line")
            }
            util::atomic_write(&path, format!("# Summary: {summary}\n{body}\n").as_bytes())?;
            map.insert(name.clone(), json!(file));
            c.set("alias", Some(&serde_json::to_string(&map)?))?;
            Ok(Value::Null)
        }
        "rm" => {
            let name = a.get(1).context("Alias name missing")?;
            let key = map
                .keys()
                .find(|n| n.eq_ignore_ascii_case(name))
                .cloned()
                .context("Alias does not exist")?;
            let file = map.remove(&key).unwrap();
            let file = file.as_str().context("Invalid alias config")?;
            util::valid_name(file)?;
            windows::remove(&c.layout.shims(false).join(format!("{file}.ps1")))?;
            c.set("alias", Some(&serde_json::to_string(&map)?))?;
            Ok(Value::Null)
        }
        "list" => {
            let mut rows = Vec::new();
            for (name, file) in map {
                let p = c
                    .layout
                    .shims(false)
                    .join(format!("{}.ps1", file.as_str().unwrap_or("")));
                let text = util::read_text(&p).unwrap_or("<BROKEN>".into());
                let summary = text
                    .lines()
                    .find_map(|s| s.strip_prefix("# Summary: "))
                    .unwrap_or("");
                let command = text
                    .lines()
                    .filter(|s| !s.starts_with("# Summary: "))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut row = json!({"Name":name,"Command":command});
                if a.iter()
                    .skip(1)
                    .any(|arg| arg == "-v" || arg == "--verbose")
                {
                    row["Summary"] = json!(summary);
                }
                rows.push(row);
            }
            rows.sort_by(|a, b| a["Name"].as_str().cmp(&b["Name"].as_str()));
            Ok(json!(rows))
        }
        _ => bail!("Unknown alias action: {sub}"),
    }
}
fn shim(c: &Config, args: &[String]) -> Result<Value> {
    let terminal = args
        .iter()
        .position(|s| s == "--" || s == "--%")
        .unwrap_or(args.len());
    let global = args[..terminal]
        .iter()
        .any(|s| s == "--global" || s == "-g");
    if global && !windows::admin() {
        bail!("Global shims require administrator rights")
    }
    let a = args
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            !(*i < terminal && (s.as_str() == "--global" || s.as_str() == "-g")) && *i != terminal
        })
        .map(|(_, s)| s.clone())
        .collect::<Vec<_>>();
    let sub = a
        .first()
        .map(String::as_str)
        .context("Use shim add, rm, list, info or alter")?;
    let root = c.layout.shims(global);
    match sub {
        "add" => {
            let name = a.get(1).context("Shim name missing")?;
            let path = a.get(2).context("Shim target missing")?;
            let target = if Path::new(path).is_file() {
                util::absolute(path)?
            } else {
                lifecycle::helper(c, path)?
            };
            lifecycle::custom_shim(
                c,
                name,
                &target,
                &a.iter().skip(3).cloned().collect::<Vec<_>>().join(" "),
                global,
            )?;
            Ok(Value::Null)
        }
        "rm" => {
            if a.len() < 2 {
                bail!("Shim name missing")
            }
            for name in &a[1..] {
                util::valid_component(name)?;
                if !root.join(format!("{name}.shim")).exists()
                    && !root.join(format!("{name}.ps1")).exists()
                {
                    return Err(CommandFailure(3).into());
                }
                lifecycle::remove_shim(c, name, global)?;
            }
            Ok(Value::Null)
        }
        "alter" => {
            let name = a.get(1).context("Shim name missing")?;
            util::valid_component(name)?;
            let active = root.join(format!("{name}.shim"));
            let mut alts = fs::read_dir(&root)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{name}.shim."))
                })
                .collect::<Vec<_>>();
            alts.sort_by_key(|e| e.file_name());
            let selected = if let Some(wanted) = a.get(2) {
                alts.into_iter().find(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case(wanted))
                })
            } else {
                alts.into_iter().next()
            }
            .context("No alternative shim")?;
            let tmp = root.join(format!("{name}.rsc-swap"));
            fs::rename(&active, &tmp)?;
            fs::rename(selected.path(), &active)?;
            fs::rename(tmp, selected.path())?;
            Ok(Value::Null)
        }
        "list" | "info" => {
            let filters = a
                .iter()
                .skip(1)
                .filter(|s| s.as_str() != "*")
                .map(|s| query::matcher(s))
                .collect::<Result<Vec<_>>>()?;
            let mut rows = Vec::new();
            for scope in [false, true] {
                if global && !scope {
                    continue;
                }
                let dir = c.layout.shims(scope);
                if !dir.is_dir() {
                    continue;
                }
                for entry in fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !path.extension().is_some_and(|e| e == "shim" || e == "ps1") {
                        continue;
                    }
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    if sub == "info" && a.get(1) != Some(&name) {
                        continue;
                    }
                    if sub == "list"
                        && !filters.is_empty()
                        && !filters
                            .iter()
                            .map(|r| query::matches(r, &name))
                            .collect::<Result<Vec<_>>>()?
                            .iter()
                            .any(|x| *x)
                    {
                        continue;
                    }
                    let text = util::read_text(&path)?;
                    let target = text
                        .lines()
                        .find_map(|l| {
                            l.split_once('=')
                                .filter(|(k, _)| k.trim() == "path")
                                .map(|(_, v)| v.trim().trim_matches('"'))
                        })
                        .unwrap_or("");
                    rows.push(json!({"Name":name,"Target":target,"IsGlobal":scope,"Path":if path.extension().is_some_and(|s|s=="shim"){path.with_extension("exe")}else{path}}));
                }
            }
            if sub == "info" && rows.is_empty() {
                return Err(CommandFailure(3).into());
            }
            Ok(json!(rows))
        }
        _ => bail!("Unknown shim action: {sub}"),
    }
}
fn checkup(c: &Config, a: &[String]) -> Result<Value> {
    if !a.is_empty() {
        bail!("checkup does not accept arguments or options");
    }
    let mut rows = Vec::new();
    for (name, path) in [
        ("User root", &c.layout.root),
        ("Global root", &c.layout.global_root),
        ("Download cache", &c.layout.cache),
    ] {
        rows.push(json!({"Check":name,"Result":if path.is_dir(){"exists"}else{"created on first use"},"Details":path}));
    }
    rows.push(json!({"Check":"PowerShell for manifest scripts","Result":if scripts::powershell().is_file(){"available"}else{"missing"},"Details":scripts::powershell()}));
    for name in ["git", "7zip"] {
        rows.push(match lifecycle::helper(c,name){Ok(p)=>json!({"Check":name,"Result":"available","Details":p}),Err(_)=>json!({"Check":name,"Result":"missing","Details":"Install before packages requiring this helper"})});
    }
    let mut broken = 0;
    for p in package::list(&c.layout, false)? {
        if let Some(error) = p.error {
            broken += 1;
            rows.push(json!({"Check":p.name,"Result":"incomplete installation","Details":error}))
        }
    }
    rows.push(json!({"Check":"Installation metadata","Result":if broken==0{"healthy"}else{"needs repair"},"Details":format!("{broken} incomplete packages")}));
    let path = windows::env_get(&c.layout.path_variable, false)?
        .map(|v| v.0)
        .unwrap_or_default();
    let expected = c.layout.shims(false).to_string_lossy().into_owned();
    rows.push(json!({"Check":"User shim PATH","Result":if path.split(';').any(|p|p.eq_ignore_ascii_case(&expected)){"present"}else{"missing"},"Details":expected}));
    Ok(json!(rows))
}
fn prompt(label: &str, default: &str) -> Result<String> {
    eprint!(
        "{} [{default}]: ",
        crate::presentation::paint(
            label,
            crate::presentation::Tone::Heading,
            crate::presentation::stderr_color()
        )
    );
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(if answer.trim().is_empty() {
        default.into()
    } else {
        answer.trim().into()
    })
}
pub fn create(c: &Config, a: &[String], progress: crate::download::Reporter) -> Result<Value> {
    let url = if let Some(u) = a.first() {
        u.clone()
    } else {
        prompt("Download URL", "")?
    };
    url::Url::parse(&url)?;
    let file = query::filename(&url)?;
    let name = Path::new(&file)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let version = prompt("Version", "1.0")?;
    let homepage = prompt("Homepage", "")?;
    let description = prompt("Description", "")?;
    let license = prompt("License", "")?;
    let config = c.clone();
    let task_url = url.clone();
    let downloaded = std::thread::spawn(move || -> Result<String> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(async {
            let temp = tempfile::tempdir()?;
            let downloader =
                crate::download::Downloader::new(&config)?.cache_directory(temp.path().into());
            let file = downloader
                .download(
                    crate::download::Task {
                        id: 0,
                        app: "manifest".into(),
                        version: "draft".into(),
                        label: "manifest download".into(),
                        file: crate::manifest::DownloadFile {
                            url: task_url,
                            hash: None,
                        },
                        headers: Default::default(),
                    },
                    progress,
                )
                .await?;
            use sha2::Digest;
            Ok(format!("{:x}", sha2::Sha256::digest(fs::read(file.path)?)))
        })
    })
    .join()
    .map_err(|_| anyhow::anyhow!("Download worker failed"))??;
    let value = json!({"version":version,"description":description,"homepage":homepage,"license":license,"url":url,"hash":downloaded});
    let output = a.get(1).cloned().unwrap_or_else(|| format!("{name}.json"));
    util::write_json(Path::new(&output), &value)?;
    println!(
        "{} {}",
        crate::presentation::paint(
            "Created",
            crate::presentation::Tone::Success,
            crate::presentation::stdout_color()
        ),
        crate::presentation::paint(
            &output,
            crate::presentation::Tone::Primary,
            crate::presentation::stdout_color()
        )
    );
    Ok(Value::Null)
}
fn virustotal(c: &Config, a: &[String]) -> Result<Value> {
    for arg in a.iter().filter(|arg| arg.starts_with('-')) {
        if !matches!(
            arg.as_str(),
            "-a" | "--all" | "-s" | "--scan" | "-n" | "--no-depends"
        ) {
            bail!("Unknown virustotal option: {arg}");
        }
    }
    let key = c
        .text("virustotal_api_key")?
        .filter(|s| !s.is_empty())
        .ok_or(CommandFailure(16))?;
    let all = a.iter().any(|s| matches!(s.as_str(), "*" | "-a" | "--all"));
    let scan = a.iter().any(|s| matches!(s.as_str(), "-s" | "--scan"));
    let independent = a
        .iter()
        .any(|s| matches!(s.as_str(), "-n" | "--no-depends"));
    let mut apps = if all {
        package::list(&c.layout, false)?
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
    } else {
        a.iter()
            .filter(|s| !s.starts_with('-'))
            .cloned()
            .collect::<Vec<_>>()
    };
    if apps.is_empty() {
        bail!("A package name or '*' is required")
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    let mut failures = 0;
    while let Some(app) = apps.pop() {
        if !seen.insert(app.clone()) {
            continue;
        }
        let resolved = match query::resolve(c, &json!({"input":app})) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{app}: {e}");
                failures |= 8;
                continue;
            }
        };
        let m = query::manifest(&resolved["manifest"])?;
        let arch = query::architecture(&m, Architecture::native())?;
        if !independent {
            apps.extend(m.strings("depends", arch)?)
        }
        for file in m.downloads(arch)? {
            let key = key.clone();
            let scan = scan;
            let app = app.clone();
            let label = app.clone();
            let report=std::thread::spawn(move||->Result<(Value,bool)>{
                let client=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(60)).build()?;
                let endpoint=file.hash.as_ref().map(|h|format!("files/{}",h.rsplit(':').next().unwrap_or(h))).unwrap_or_else(||{
                    use base64::Engine;format!("urls/{}",base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(file.url.as_bytes()))});
                let mut response=client.get(format!("https://www.virustotal.com/api/v3/{endpoint}")).header("x-apikey",&key).send()?;
                if response.status().as_u16()==404{
                    use base64::Engine;let id=base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(file.url.as_bytes());
                    response=client.get(format!("https://www.virustotal.com/api/v3/urls/{id}")).header("x-apikey",&key).send()?;
                    if response.status().as_u16()==404&&scan{let response=client.post("https://www.virustotal.com/api/v3/urls").header("x-apikey",&key).form(&[("url",&file.url)]).send()?.error_for_status()?;
                        return Ok((json!({"Package":app,"URL":util::redact_url(&file.url),"State":"submitted","Analysis":response.json::<Value>()?}),false))}
                }
                if response.status().as_u16()==404{return Ok((json!({"Package":app,"State":"no report","URL":util::redact_url(&file.url)}),false))}
                let value:Value=response.error_for_status()?.json()?;let stats=&value["data"]["attributes"]["last_analysis_stats"];
                let unsafe_count=stats["malicious"].as_u64().unwrap_or(0)+stats["suspicious"].as_u64().unwrap_or(0);
                let id=value["data"]["id"].as_str().unwrap_or("");
                Ok((json!({"Package":app,"Detections":unsafe_count,"Stats":stats,"Report":format!("https://www.virustotal.com/gui/{endpoint}"),"ID":id}),unsafe_count>0))
            }).join().map_err(|_|anyhow::anyhow!("VirusTotal worker failed"))?;
            match report {
                Ok((row, unsafe_report)) => {
                    if unsafe_report {
                        failures |= 2;
                    }
                    rows.push(row)
                }
                Err(e) => {
                    eprintln!("{label}: {e}");
                    failures |= 4;
                }
            }
        }
    }
    if failures != 0 {
        for row in &rows {
            println!("{row}");
        }
        return Err(CommandFailure(failures).into());
    }
    Ok(json!(rows))
}
