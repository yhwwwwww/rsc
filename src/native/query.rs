use crate::{
    bucket::{self, PackageSpec},
    config::Config,
    manifest::{Architecture, Manifest},
    package, util,
};
use anyhow::{Context, Result, bail};
use fancy_regex::Regex;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
pub fn matcher(query: &str) -> Result<Regex> {
    Regex::new(&format!("(?i){query}")).context("Invalid search expression")
}
pub fn matches(re: &Regex, s: &str) -> Result<bool> {
    Ok(re.is_match(s)?)
}
pub fn filename(url: &str) -> Result<String> {
    let u = url::Url::parse(url)?;
    let text = if let Some(f) = u.fragment().filter(|f| f.starts_with('/')) {
        f.trim_start_matches('/').to_owned()
    } else {
        u.path_segments()
            .and_then(|mut p| p.next_back())
            .filter(|p| !p.is_empty())
            .unwrap_or("download")
            .to_owned()
    };
    let text = percent_encoding::percent_decode_str(&text)
        .decode_utf8()?
        .to_string();
    util::valid_component(&text)?;
    Ok(text)
}
pub fn manifest(v: &Value) -> Result<Manifest> {
    Manifest::parse(serde_json::to_string(v)?)
}
pub fn architecture(m: &Manifest, desired: Architecture) -> Result<Architecture> {
    if util::ci_get(&m.raw, "url").is_none() && util::ci_get(&m.raw, "architecture").is_none() {
        return Ok(desired);
    }
    if m.field("url", desired).is_some() {
        return Ok(desired);
    }
    let candidates = match desired {
        Architecture::Arm64 => vec![Architecture::X64, Architecture::X86],
        Architecture::X64 => vec![Architecture::X86],
        _ => vec![],
    };
    for a in candidates {
        if m.field("url", a).is_some() {
            return Ok(a);
        }
    }
    bail!("Package does not support {desired}")
}
pub fn prepare(_c: &Config, d: &Value) -> Result<Value> {
    let m = manifest(&d["manifest"])?;
    let a = architecture(
        &m,
        Architecture::parse(d["architecture"].as_str().unwrap_or("64bit"))?,
    )?;
    let version = m.version()?;
    util::valid_component(version)?;
    let mut helpers = Vec::new();
    let urls = m.strings("url", a)?;
    if m.field("innosetup", a).and_then(Value::as_bool) == Some(true) {
        helpers.push("innounp")
    }
    if urls.iter().any(|u| {
        filename(u).is_ok_and(|f| {
            let f = f.to_lowercase();
            [
                ".7z", ".rar", ".tar", ".tgz", ".gz", ".bz2", ".xz", ".zst", ".lzh",
            ]
            .iter()
            .any(|s| f.ends_with(s))
        })
    }) {
        helpers.push("7zip")
    }
    if m.field("extract_with", a).and_then(Value::as_str) == Some("dark") {
        helpers.push("wixtoolset")
    }
    Ok(
        json!({"architecture":a,"helpers":helpers,"nightly":format!("nightly-{}",chrono::Local::now().format("%Y%m%d"))}),
    )
}
pub fn headers(c: &Config, url: &str) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for item in c
        .get("private_hosts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let pattern = util::ci_get(item, "match")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !pattern.is_empty() && matches(&matcher(pattern)?, url)? {
            if let Some(h) = util::ci_get(item, "headers") {
                if let Some(map) = h.as_object() {
                    for (k, v) in map {
                        if let Some(s) = v.as_str() {
                            out.insert(k.clone(), json!(s));
                        }
                    }
                } else {
                    for line in h.as_str().unwrap_or("").lines() {
                        if let Some((k, v)) = line.split_once('=') {
                            out.insert(k.trim().into(), json!(v.trim()));
                        }
                    }
                }
            }
        }
    }
    Ok(out.into())
}
pub fn http(c: &Config, url: &str, method: &str, body: Option<Value>) -> Result<(u16, String)> {
    let c = c.clone();
    let url = url.to_owned();
    let method = method.to_owned();
    std::thread::spawn(move || -> Result<(u16, String)> {
        if c.text("proxy")?
            .is_some_and(|s| s.starts_with("currentuser@") || s.ends_with("@default"))
            && method == "GET"
        {
            let tmp = tempfile::tempdir()?;
            let p = tmp.path().join("response");
            super::windows::download(&c, &json!({"url":url,"to":p}))?;
            return Ok((200, util::read_text(&p)?));
        }
        let mut b = reqwest::blocking::Client::builder()
            .user_agent("rsc/0.1")
            .timeout(std::time::Duration::from_secs(60));
        if let Some(proxy) = c.text("proxy")? {
            if proxy == "none" {
                b = b.no_proxy()
            } else if !proxy.is_empty() && proxy != "default" {
                let p = if proxy.contains("://") {
                    proxy
                } else {
                    format!("http://{proxy}")
                };
                b = b.proxy(reqwest::Proxy::all(p)?)
            }
        }
        let client = b.build()?;
        let mut r = client.request(method.parse()?, &url);
        if let Some(h) = headers(&c, &url)?.as_object() {
            for (k, v) in h {
                r = r.header(k, v.as_str().context("Invalid private header")?);
            }
        }
        if url.starts_with("https://api.github.com/") {
            if let Some(token) = c.text("gh_token")?.or(c.text("github_token")?) {
                r = r.bearer_auth(token)
            }
        }
        if let Some(body) = body {
            r = r.json(&body)
        }
        let response = r.send().map_err(|e| e.without_url())?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|n| n > 16 * 1024 * 1024)
        {
            bail!("Remote metadata exceeds 16 MiB")
        }
        use std::io::Read;
        let mut text = String::new();
        response
            .take(16 * 1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        if text.len() > 16 * 1024 * 1024 {
            bail!("Remote metadata exceeds 16 MiB")
        }
        Ok((status, text))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("HTTP worker failed"))?
}
pub fn get(c: &Config, url: &str) -> Result<String> {
    let (code, text) = http(c, url, "GET", None)?;
    if !(200..300).contains(&code) {
        bail!("HTTP {code}: {}", util::redact_url(url))
    }
    Ok(text.trim_start_matches('\u{feff}').into())
}
pub fn resolve(c: &Config, d: &Value) -> Result<Value> {
    resolve_report(c, d, true)
}
fn resolve_report(c: &Config, d: &Value, emit_warning: bool) -> Result<Value> {
    let input = d["input"].as_str().context("Package missing")?;
    if input.starts_with("https://") || input.starts_with("http://") {
        let name = Path::new(&filename(input)?)
            .file_stem()
            .context("Manifest filename missing")?
            .to_string_lossy()
            .into_owned();
        util::valid_name(&name)?;
        return Ok(
            json!({"name":name,"bucket":null,"source":input,"manifest":manifest(&serde_json::from_str::<Value>(&get(c,input)?)?)?.raw}),
        );
    }
    if Path::new(input).is_file() {
        let p = bucket::resolve(&c.layout.buckets(), input)?;
        return Ok(
            json!({"name":p.name,"bucket":p.bucket,"source":p.source,"manifest":p.manifest.raw}),
        );
    }
    let spec = PackageSpec::parse(input)?;
    let base = spec
        .bucket
        .as_ref()
        .map(|b| format!("{b}/{}", spec.name))
        .unwrap_or(spec.name.clone());
    let (p, warning) = bucket::Resolver::new(&c.layout.buckets())?.resolve_report(&base)?;
    if emit_warning {
        if let Some(warning) = &warning {
            crate::presentation::warning(warning);
        }
    }
    let mut source = p.source;
    let mut m = p.manifest;
    if let Some(version) = spec
        .version
        .filter(|v| m.version().ok() != Some(v.as_str()))
    {
        if let Some(text) = d["historical"]["ManifestText"].as_str() {
            m = Manifest::parse(text.to_owned())?
        } else {
            let bucket_dir = c.layout.buckets().join(
                p.bucket
                    .as_deref()
                    .context("Version lookup requires a bucket")?,
            );
            let relative = Path::new(&source)
                .strip_prefix(&bucket_dir)?
                .to_string_lossy()
                .replace('\\', "/");
            let history = crate::manager::git(
                c,
                Some(&bucket_dir),
                &["log", "--all", "--format=%H", "--", &relative],
            )?;
            let mut found = None;
            for revision in history.lines() {
                if let Ok(text) = crate::manager::git(
                    c,
                    Some(&bucket_dir),
                    &["show", &format!("{revision}:{relative}")],
                ) {
                    if let Ok(old) = Manifest::parse(text) {
                        if old.version()? == version {
                            found = Some(old);
                            break;
                        }
                    }
                }
            }
            m = if let Some(old) = found {
                old
            } else {
                autoupdate(c, &m, &version)?
            };
        }
        if m.version()? != version {
            bail!("Historical version mismatch")
        }
        let path = c
            .layout
            .root
            .join("workspace")
            .join(format!("{}@{version}.json", p.name));
        util::write_json(&path, &m.raw)?;
        source = path.to_string_lossy().into_owned();
        return Ok(
            json!({"name":p.name,"bucket":null,"source":source,"manifest":m.raw,"source_warning":warning}),
        );
    }
    Ok(
        json!({"name":p.name,"bucket":p.bucket,"source":source,"manifest":m.raw,"source_warning":warning}),
    )
}
fn substitute(s: &str, version: &str, url: Option<&str>) -> String {
    let mut vars = vec![
        ("$version", version.to_owned()),
        ("$dotVersion", version.replace('_', ".")),
        ("$underscoreVersion", version.replace('.', "_")),
        ("$dashVersion", version.replace('.', "-")),
        ("$cleanVersion", version.replace('.', "")),
        ("$matchHead", version.split('.').next().unwrap_or("").into()),
    ];
    if let Some(u) = url {
        vars.push(("$url", u.into()));
        vars.push((
            "$baseurl",
            u.rsplit_once('/').map(|(p, _)| p).unwrap_or(u).into(),
        ));
    }
    vars.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    let mut out = s.to_owned();
    for (k, v) in vars {
        out = out.replace(k, &v)
    }
    out
}
fn expand(v: &Value, version: &str) -> Value {
    match v {
        Value::String(s) => json!(substitute(s, version, None)),
        Value::Array(a) => json!(a.iter().map(|v| expand(v, version)).collect::<Vec<_>>()),
        Value::Object(o) => o
            .iter()
            .map(|(k, v)| (k.clone(), expand(v, version)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
        _ => v.clone(),
    }
}
fn autoupdate(c: &Config, m: &Manifest, version: &str) -> Result<Manifest> {
    let auto = util::ci_get(&m.raw, "autoupdate")
        .context("Version unavailable in bucket history and no autoupdate template")?;
    let mut raw = m.raw.clone();
    raw["version"] = json!(version);
    let auto = expand(auto, version);
    for (k, v) in auto.as_object().context("Invalid autoupdate template")? {
        raw[k] = v.clone();
    }
    // Hash definitions may be a URL/regex lookup rather than a literal digest.
    for a in [Architecture::X64, Architecture::X86, Architecture::Arm64] {
        let temp = manifest(&raw)?;
        let hash = temp.field("hash", a).cloned();
        if let Some(h) = hash.filter(Value::is_object) {
            let url_template = util::ci_get(&h, "url")
                .and_then(Value::as_str)
                .context("Autoupdate hash URL missing")?;
            let urls = temp.strings("url", a)?;
            let mut hashes = Vec::new();
            for url in urls {
                let text = get(c, &substitute(url_template, version, Some(&url)))?;
                let pattern = util::ci_get(&h, "regex")
                    .and_then(Value::as_str)
                    .unwrap_or(r"(?i)\b([0-9a-f]{64})\b");
                let re = matcher(pattern)?;
                let captures = re
                    .captures(&text)?
                    .context("Cannot find generated manifest hash")?;
                hashes.push(
                    captures
                        .get(1)
                        .or_else(|| captures.get(0))
                        .context("Missing hash capture")?
                        .as_str()
                        .to_owned(),
                );
            }
            if raw["architecture"][a.to_string()].is_object() {
                raw["architecture"][a.to_string()]["hash"] = json!(hashes)
            } else {
                raw["hash"] = json!(hashes)
            }
        }
    }
    manifest(&raw)
}
pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\d+|[A-Za-z]+").unwrap());
    let re = &*RE;
    let aa = re.find_iter(a).map(|x| x.as_str()).collect::<Vec<_>>();
    let bb = re.find_iter(b).map(|x| x.as_str()).collect::<Vec<_>>();
    for i in 0..aa.len().max(bb.len()) {
        let x = aa.get(i).copied().unwrap_or("0");
        let y = bb.get(i).copied().unwrap_or("0");
        let order = match (x.parse::<u128>(), y.parse::<u128>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            // Scoop compares present mixed text/number fields as strings.
            // This keeps legacy numeric versions below Git tags such as v0.1.0.
            (Ok(x), Err(_)) if i < aa.len() && i < bb.len() => {
                x.to_string().cmp(&y.to_ascii_lowercase())
            }
            (Err(_), Ok(y)) if i < aa.len() && i < bb.len() => {
                x.to_ascii_lowercase().cmp(&y.to_string())
            }
            (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
            (Err(_), Ok(_)) => std::cmp::Ordering::Less,
            _ => x.to_lowercase().cmp(&y.to_lowercase()),
        };
        if !order.is_eq() {
            return order;
        }
    }
    std::cmp::Ordering::Equal
}
/// Reuse installation metadata and bucket filenames for the whole status operation.
pub fn statuses(c: &Config, installed: &[package::Installed]) -> Result<Vec<Value>> {
    statuses_for(c, installed, installed)
}
fn statuses_for(
    c: &Config,
    installed: &[package::Installed],
    selected: &[package::Installed],
) -> Result<Vec<Value>> {
    let resolver = bucket::Resolver::new(&c.layout.buckets())?;
    let names = installed
        .iter()
        .map(|p| p.name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut rows = Vec::with_capacity(selected.len());
    for p in selected {
        let mut row = json!({"installed":true,"failed":p.error.is_some(),"hold":p.held,
            "removed":false,"outdated":false,"missing_deps":[],"version":p.version});
        let source = p
            .bucket
            .as_ref()
            .map(|b| format!("{b}/{}", p.name))
            .unwrap_or_else(|| p.name.clone());
        if let Ok((latest, warning)) = resolver.resolve_report(&source) {
            if let Some(warning) = warning {
                row["source_warning"] = json!(warning);
            }
            let m = latest.manifest;
            let v = m.version()?;
            row["latest_version"] = json!(v);
            row["outdated"] = json!(
                p.version.as_deref() != Some(v)
                    && (v == "nightly"
                        || p.version
                            .as_ref()
                            .is_some_and(|old| compare(v, old).is_gt()))
            );
            let a = Architecture::parse(p.architecture.as_deref().unwrap_or("64bit"))?;
            row["missing_deps"] = json!(
                m.strings("depends", a)?
                    .into_iter()
                    .filter(|s| PackageSpec::parse(s)
                        .is_ok_and(|s| !names.contains(&s.name.to_ascii_lowercase())))
                    .collect::<Vec<_>>()
            );
        } else {
            let info = util::read_json(&package::metadata_file(
                &p.path,
                "scoop-install.json",
                "install.json",
            ))
            .unwrap_or(Value::Null);
            if let Some(source) = info["url"].as_str() {
                if let Ok(latest) = resolve_report(c, &json!({"input":source}), false) {
                    if latest["source_warning"].is_string() {
                        row["source_warning"] = latest["source_warning"].clone();
                    }
                    let v = latest["manifest"]["version"].as_str().unwrap_or("");
                    row["latest_version"] = json!(v);
                    row["outdated"] = json!(p.version.as_deref() != Some(v));
                }
            } else {
                row["removed"] = json!(true);
            }
        }
        rows.push(row);
    }
    Ok(rows)
}
pub fn invoke(c: &Config, action: &str, d: &Value) -> Result<Value> {
    match action {
        "match" => {
            let re = matcher(d["query"].as_str().unwrap_or(""))?;
            Ok(json!(
                d["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| matches(&re, v.as_str().unwrap_or("")))
                    .collect::<Result<Vec<_>>>()?
            ))
        }
        "search_match" => {
            let re = matcher(d["query"].as_str().unwrap_or(""))?;
            Ok(json!(
                d["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| -> Result<bool> {
                        if matches(&re, v["name"].as_str().unwrap_or(""))? {
                            return Ok(true);
                        }
                        let m = manifest(&v["manifest"])?;
                        for a in [Architecture::X64, Architecture::X86, Architecture::Arm64] {
                            for bin in m.bins(a) {
                                if matches(&re, &bin)? {
                                    return Ok(true);
                                }
                            }
                        }
                        Ok(false)
                    })
                    .collect::<Result<Vec<_>>>()?
            ))
        }
        "latest" => {
            let mut map = BTreeMap::<(String, String), Value>::new();
            for row in d["rows"].as_array().into_iter().flatten() {
                let key = (
                    row["bucket"].as_str().unwrap_or("").into(),
                    row["package"].as_str().unwrap_or("").into(),
                );
                if map.get(&key).is_none_or(|old| {
                    compare(
                        row["version"].as_str().unwrap_or(""),
                        old["version"].as_str().unwrap_or(""),
                    )
                    .is_gt()
                }) {
                    map.insert(key, row.clone());
                }
            }
            Ok(json!(map.into_values().collect::<Vec<_>>()))
        }
        "status" => {
            let installed = package::list(&c.layout, false)?;
            let requested = d["apps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| {
                    (
                        item["name"].as_str().unwrap_or("").to_ascii_lowercase(),
                        item["global"].as_bool().unwrap_or(false),
                    )
                })
                .collect::<std::collections::HashSet<_>>();
            let selected = installed
                .iter()
                .filter(|p| requested.contains(&(p.name.to_ascii_lowercase(), p.scope == "global")))
                .cloned()
                .collect::<Vec<_>>();
            let states = statuses_for(c, &installed, &selected)?;
            let lookup = selected
                .iter()
                .enumerate()
                .map(|(i, p)| ((p.name.to_ascii_lowercase(), p.scope == "global"), i))
                .collect::<std::collections::HashMap<_, _>>();
            let rows = d["apps"].as_array().into_iter().flatten().map(|item| {
                let key = (item["name"].as_str().unwrap_or("").to_ascii_lowercase(), item["global"].as_bool().unwrap_or(false));
                lookup.get(&key).map(|i|states[*i].clone()).unwrap_or_else(||
                    json!({"installed":false,"failed":false,"hold":false,"removed":false,"outdated":false,"missing_deps":[]}))
            }).collect::<Vec<_>>();
            Ok(json!(rows))
        }
        "info" => {
            let input = d["input"].as_str().unwrap_or("");
            let resolved = resolve(c, d).or_else(|_| -> Result<Value> {
                let p = package::find(&c.layout, input, false)?;
                let m = Manifest::read(&package::metadata_file(
                    &p.path,
                    "scoop-manifest.json",
                    "manifest.json",
                ))?;
                Ok(json!({"name":p.name,"bucket":p.bucket,"source":p.path,"manifest":m.raw}))
            })?;
            let m = manifest(&resolved["manifest"])?;
            let mut row = json!({"Name":resolved["name"],"Description":m.description(),"Version":m.version()?,"Bucket":resolved["bucket"],"Website":m.raw["homepage"],"License":m.raw["license"],"Manifest":resolved["source"]});
            let installed = package::list(&c.layout, false)?
                .into_iter()
                .filter(|p| p.name.as_str() == resolved["name"].as_str().unwrap_or(""))
                .map(|p| {
                    crate::presentation::version_scope(
                        p.version.as_deref().unwrap_or("?"),
                        &p.scope,
                    )
                })
                .collect::<Vec<_>>();
            row["Installed"] = json!(installed);
            if d["verbose"].as_bool() == Some(true) {
                row["Architecture"] = json!(Architecture::native());
                row["Binaries"] = json!(m.bins(Architecture::native()));
                row["Dependencies"] = m.raw["depends"].clone();
                row["Notes"] = m.raw["notes"].clone();
            }
            Ok(json!([row]))
        }
        "search_remote" => {
            let re = matcher(d["query"].as_str().unwrap_or(""))?;
            let buckets = bucket::inventory(&c.layout.buckets())?;
            let mut rows = Vec::new();
            let mut limited = false;
            for &(name, repo) in super::known::BUCKETS {
                if buckets.iter().any(|b| b.name.eq_ignore_ascii_case(name)) {
                    continue;
                }
                let (status, text) = http(
                    c,
                    &format!("https://api.github.com/repos/{repo}/git/trees/HEAD?recursive=1"),
                    "GET",
                    None,
                )?;
                if status == 403 || status == 429 {
                    limited = true;
                    break;
                }
                if status != 200 {
                    continue;
                }
                let tree: Value = serde_json::from_str(&text)?;
                for item in tree["tree"].as_array().into_iter().flatten() {
                    let p = item["path"].as_str().unwrap_or("");
                    if p.ends_with(".json") && p.starts_with("bucket/") {
                        let app = Path::new(p).file_stem().unwrap().to_string_lossy();
                        if matches(&re, &app)? {
                            rows.push(json!({"Name":app,"Source":name}))
                        }
                    }
                }
            }
            if limited {
                eprintln!("GitHub search rate limit reached; configure gh_token or retry later.");
            }
            Ok(json!({"rows":rows,"limited":limited}))
        }
        _ => bail!("Unknown query action {action}"),
    }
}
pub fn special(c: &Config, text: &str) -> Result<Value> {
    let u = url::Url::parse(text)?;
    let host = u.host_str().unwrap_or("");
    if host == "sourceforge.net" || host.ends_with(".sourceforge.net") {
        if let Some(rest) = u.path().strip_prefix("/projects/") {
            if let Some((project, path)) = rest.split_once("/files/") {
                let path = path.trim_end_matches("/download");
                return Ok(json!(format!(
                    "https://downloads.sourceforge.net/project/{project}/{path}"
                )));
            }
        }
    }
    if host == "github.com" && c.text("gh_token")?.or(c.text("github_token")?).is_some() {
        let parts = u
            .path_segments()
            .map(|p| p.collect::<Vec<_>>())
            .unwrap_or_default();
        if parts.len() >= 6 && parts[2] == "releases" && parts[3] == "download" {
            let release: Value = serde_json::from_str(&get(
                c,
                &format!(
                    "https://api.github.com/repos/{}/{}/releases/tags/{}",
                    parts[0], parts[1], parts[4]
                ),
            )?)?;
            if let Some(asset) = release["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|v| v["name"].as_str() == Some(parts[5]))
            {
                if let Some(url) = asset["url"].as_str() {
                    return Ok(json!(url));
                }
            }
        }
    }
    if host == "fosshub.com" || host.ends_with(".fosshub.com") {
        let page = get(c, text)?;
        let mut fields = serde_json::Map::new();
        let capture = |key: &str| -> Result<Option<String>> {
            let re = regex::Regex::new(&format!(r#""{}"\\s*:\\s*"([^"]+)""#, regex::escape(key)))?;
            Ok(re
                .captures(&page)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().into()))
        };
        if let Some(project) = capture("p")? {
            fields.insert("projectId".into(), json!(project));
        }
        if let Some(release) = capture("r")? {
            fields.insert("releaseId".into(), json!(release));
        }
        let parts = u
            .path_segments()
            .map(|p| p.collect::<Vec<_>>())
            .unwrap_or_default();
        let project = parts
            .first()
            .copied()
            .unwrap_or("")
            .trim_end_matches(".html");
        let file = u
            .query_pairs()
            .find(|(k, _)| k == "dwl")
            .map(|(_, v)| v.into_owned())
            .or_else(|| u.fragment().map(|s| s.trim_start_matches('/').into()))
            .or_else(|| parts.get(1).map(|s| (*s).into()))
            .context("FossHub download filename missing")?;
        fields.insert("projectUri".into(), json!(project));
        fields.insert("fileName".into(), json!(file));
        fields.insert("source".into(), json!("CF"));
        fields.insert("isLatestVersion".into(), json!(true));
        let (status, body) = http(
            c,
            "https://api.fosshub.com/download/",
            "POST",
            Some(fields.into()),
        )?;
        if status != 200 {
            bail!("FossHub HTTP {status}")
        }
        let body: Value = serde_json::from_str(&body)?;
        if !body["error"].is_null() {
            bail!("FossHub download lookup failed: {}", body["error"])
        }
        return Ok(json!(
            body["data"]["url"]
                .as_str()
                .context("FossHub response has no URL")?
        ));
    }
    Ok(json!(text))
}
