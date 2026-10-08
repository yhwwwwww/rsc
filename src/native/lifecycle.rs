//! Package extraction and Windows integration, implemented independently in Rust.
use super::{query, scripts, windows};
use crate::{
    config::Config,
    manifest::{Architecture, Manifest},
    util,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};
fn items(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.clone(),
        Some(v) => vec![v.clone()],
        None => vec![],
    }
}
pub fn relative(base: &Path, s: &str) -> Result<PathBuf> {
    let p = Path::new(s);
    if p.is_absolute()
        || p.components().any(|v| {
            matches!(
                v,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
    {
        bail!("Manifest path must stay inside package: {s}")
    }
    Ok(base.join(p))
}
fn context(c: &Config, d: &Value) -> Result<Value> {
    let mut ctx = d.clone();
    let name = d["name"].as_str().context("Package name missing")?;
    util::valid_name(name)?;
    let global = d["global"].as_bool().unwrap_or(false);
    ctx["persist_dir"] = json!(c.layout.base(global).join("persist").join(name));
    ctx["original_dir"] = d["dir"].clone();
    ctx["fname"] = d["files"].get(0).cloned().unwrap_or(Value::Null);
    Ok(ctx)
}
fn args(v: Option<&Value>, d: &Value) -> Vec<String> {
    items(v)
        .iter()
        .filter_map(Value::as_str)
        .map(|s| scripts::expand(s, d))
        .collect()
}
pub fn helper(c: &Config, name: &str) -> Result<PathBuf> {
    let (app, files) = match name.to_lowercase().as_str() {
        "7zip" | "7z" => ("7zip", vec!["7z.exe"]),
        "innounp" => ("innounp", vec!["innounp.exe"]),
        "lessmsi" => ("lessmsi", vec!["lessmsi.exe"]),
        "dark" => ("wixtoolset", vec!["dark.exe", "bin/dark.exe"]),
        _ => (name, vec![name]),
    };
    for global in [false, true] {
        for file in &files {
            let p = c.layout.apps(global).join(app).join("current").join(file);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        for file in &files {
            for extension in ["", ".exe", ".cmd"] {
                let p = dir.join(format!("{file}{extension}"));
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    bail!("Required extraction helper {name} is not installed")
}
fn run_tool(c: &Config, name: &str, args: &[String]) -> Result<()> {
    let tool = helper(c, name)?;
    let status = Command::new(tool).args(args).status()?;
    if !status.success() {
        bail!("{name} failed with exit {}", status.code().unwrap_or(1))
    }
    Ok(())
}
fn copy_merge(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_merge(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else {
        if let Some(p) = target.parent() {
            fs::create_dir_all(p)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}
fn find_extract(root: &Path, pattern: &str) -> Result<PathBuf> {
    let path = relative(root, pattern)?;
    if path.exists() {
        return Ok(path);
    }
    if pattern.contains(['*', '?']) {
        let mut pending = vec![root.to_owned()];
        let regex = regex::Regex::new(&format!(
            "(?i)^{}$",
            regex::escape(pattern)
                .replace(r"\*", ".*")
                .replace(r"\?", ".")
        ))?;
        while let Some(p) = pending.pop() {
            for e in fs::read_dir(p)? {
                let e = e?;
                if e.file_type()?.is_dir() {
                    let rel = e
                        .path()
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/");
                    if regex.is_match(&rel) {
                        return Ok(e.path());
                    }
                    pending.push(e.path());
                }
            }
        }
    }
    bail!("Archive directory does not exist: {pattern}")
}
pub fn extract(
    c: &Config,
    archive: &Path,
    dest: &Path,
    select: Option<&str>,
    kind: Option<&str>,
) -> Result<bool> {
    let name = archive
        .file_name()
        .context("Archive name missing")?
        .to_string_lossy()
        .to_lowercase();
    let zip = name.ends_with(".zip");
    let other = [
        ".7z", ".rar", ".tar", ".tgz", ".gz", ".bz2", ".xz", ".zst", ".lzh",
    ]
    .iter()
    .any(|s| name.ends_with(s));
    let msi = name.ends_with(".msi");
    if !zip && !other && !msi && kind.is_none() {
        return Ok(false);
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("contents");
    fs::create_dir(&root)?;
    if zip && kind.is_none() {
        let mut archive = zip::ZipArchive::new(fs::File::open(archive)?)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let path = entry
                .enclosed_name()
                .context("Archive entry escapes destination")?;
            let target = root.join(&path);
            if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                bail!("Archive symbolic links require an explicit manifest installer")
            }
            if entry.is_dir() {
                fs::create_dir_all(target)?
            } else {
                fs::create_dir_all(target.parent().unwrap())?;
                let mut file = fs::File::create(target)?;
                io::copy(&mut entry, &mut file)?;
            }
        }
    } else if kind == Some("innounp") {
        run_tool(
            c,
            "innounp",
            &[
                "-x".into(),
                "-y".into(),
                format!("-d{}", root.display()),
                archive.display().to_string(),
            ],
        )?;
    } else if kind == Some("dark") {
        run_tool(
            c,
            "dark",
            &[
                "-x".into(),
                root.display().to_string(),
                archive.display().to_string(),
            ],
        )?;
    } else if msi {
        if c.get("use_lessmsi").and_then(Value::as_bool) == Some(true) {
            run_tool(
                c,
                "lessmsi",
                &[
                    "x".into(),
                    archive.display().to_string(),
                    format!("{}\\", root.display()),
                ],
            )?;
        } else {
            let status = Command::new("msiexec.exe")
                .args(["/a"])
                .arg(archive)
                .args(["/qn", "/norestart"])
                .arg(format!("TARGETDIR={}", root.display()))
                .status()?;
            if !matches!(status.code(), Some(0 | 3010)) {
                bail!("MSI extraction failed: {:?}", status.code())
            }
        }
    } else {
        run_tool(
            c,
            "7zip",
            &[
                "x".into(),
                "-y".into(),
                format!("-o{}", root.display()),
                archive.display().to_string(),
            ],
        )?;
        if [".tar.gz", ".tar.bz2", ".tar.xz", ".tgz"]
            .iter()
            .any(|s| name.ends_with(s))
        {
            let tar = fs::read_dir(&root)?
                .filter_map(|e| e.ok())
                .find(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case("tar"))
                })
                .context("Compressed archive has no TAR payload")?;
            run_tool(
                c,
                "7zip",
                &[
                    "x".into(),
                    "-y".into(),
                    format!("-o{}", root.display()),
                    tar.path().display().to_string(),
                ],
            )?;
            fs::remove_file(tar.path())?;
        }
    }
    let selected = if let Some(pattern) = select.filter(|s| !s.is_empty()) {
        find_extract(&root, pattern)?
    } else if msi && root.join("SourceDir").is_dir() {
        root.join("SourceDir")
    } else if kind == Some("innounp") && root.join("{app}").is_dir() {
        root.join("{app}")
    } else {
        root
    };
    copy_merge(&selected, dest)?;
    Ok(true)
}
fn add_shim(c: &Config, name: &str, target: &Path, fixed: &str, global: bool) -> Result<()> {
    util::valid_component(name)?;
    if !target.is_file() {
        bail!("Shim target missing: {}", target.display())
    }
    let dir = c.layout.shims(global);
    fs::create_dir_all(&dir)?;
    let descriptor = dir.join(format!("{name}.shim"));
    if descriptor.is_file() {
        let old = util::read_text(&descriptor)?;
        let new = format!("path = \"{}\"\nargs = {fixed}\n", target.display());
        if old != new {
            let source = old
                .lines()
                .find_map(|l| l.strip_prefix("path = "))
                .unwrap_or("")
                .trim_matches('"');
            let app = Path::new(source)
                .ancestors()
                .find_map(|p| {
                    if p.parent()
                        .is_some_and(|p| p.file_name().is_some_and(|n| n == "apps"))
                    {
                        p.file_name().map(|n| n.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .unwrap_or("external".into());
            fs::copy(&descriptor, dir.join(format!("{name}.shim.{app}")))?;
        }
    }
    let launcher = dir.join(format!("{name}.exe"));
    let source = std::env::current_exe()?;
    if !launcher.is_file() || fs::metadata(&launcher)?.len() != fs::metadata(&source)?.len() {
        windows::remove(&launcher)?;
        fs::copy(&source, &launcher)
            .with_context(|| format!("Cannot create shim {}", launcher.display()))?;
    }
    // GUI targets receive a GUI-subsystem launcher with the same native dispatcher.
    let bytes = fs::read(target).unwrap_or_default();
    if bytes.starts_with(b"MZ") && bytes.len() > 64 {
        let pe = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
        if bytes.get(pe + 92..pe + 94) == Some(&[2, 0][..]) {
            let mut stub = fs::read(&launcher)?;
            if stub.len() > pe + 94 {
                let offset = u32::from_le_bytes(stub[60..64].try_into().unwrap()) as usize + 92;
                stub[offset..offset + 2].copy_from_slice(&2u16.to_le_bytes());
                fs::write(&launcher, stub)?;
            }
        }
    }
    util::atomic_write(
        &descriptor,
        format!("path = \"{}\"\nargs = {fixed}\n", target.display()).as_bytes(),
    )
    .with_context(|| format!("Cannot write shim {}", descriptor.display()))?;
    windows::path_edit(&c.layout.path_variable, &dir, true, global)?;
    Ok(())
}
pub fn remove_shim(c: &Config, name: &str, global: bool) -> Result<()> {
    let dir = c.layout.shims(global);
    let active = dir.join(format!("{name}.shim"));
    let mut alternatives = if dir.is_dir() {
        fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{name}.shim."))
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    alternatives.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
    if let Some(other) = alternatives.pop() {
        windows::remove(&active)?;
        fs::rename(other.path(), &active)?;
    } else {
        for ext in ["shim", "exe", "cmd", "ps1"] {
            windows::remove(&dir.join(format!("{name}.{ext}")))?;
        }
    }
    Ok(())
}
fn bins(c: &Config, m: &Manifest, a: Architecture, ctx: &Value, add: bool) -> Result<()> {
    let dir = Path::new(ctx["dir"].as_str().unwrap());
    let global = ctx["global"].as_bool().unwrap_or(false);
    for item in items(m.field("bin", a)) {
        let (target, name, fixed) = if let Some(s) = item.as_str() {
            (
                s.to_owned(),
                Path::new(s)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                String::new(),
            )
        } else {
            let arr = item.as_array().context("Invalid binary entry")?;
            let target = arr
                .first()
                .and_then(Value::as_str)
                .context("Binary target missing")?;
            let name = arr
                .get(1)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    Path::new(target)
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                });
            (target.to_owned(), name, args(arr.get(2), ctx).join(" "))
        };
        if add {
            add_shim(c, &name, &relative(dir, &target)?, &fixed, global)?
        } else {
            let descriptor = c.layout.shims(global).join(format!("{name}.shim"));
            let app = ctx["name"].as_str().unwrap_or("");
            if descriptor.is_file()
                && util::read_text(&descriptor)?.contains(&format!(r"\apps\{app}\"))
            {
                remove_shim(c, &name, global)?
            } else {
                windows::remove(&c.layout.shims(global).join(format!("{name}.shim.{app}")))?;
            }
            // Scoop-created script shims do not have a descriptor.
            if !descriptor.exists() {
                for ext in ["ps1", "cmd"] {
                    windows::remove(&c.layout.shims(global).join(format!("{name}.{ext}")))?;
                }
            }
        }
    }
    Ok(())
}
fn shortcut_root(global: bool) -> Result<PathBuf> {
    let key = if global { "ProgramData" } else { "APPDATA" };
    let root = PathBuf::from(std::env::var_os(key).context("Start Menu folder missing")?);
    Ok(root.join(r"Microsoft\Windows\Start Menu\Programs\Scoop Apps"))
}
fn integrations(c: &Config, m: &Manifest, a: Architecture, ctx: &Value, add: bool) -> Result<()> {
    let dir = Path::new(ctx["dir"].as_str().context("Install directory missing")?);
    let global = ctx["global"].as_bool().unwrap_or(false);
    bins(c, m, a, ctx, add)?;
    for entry in items(m.field("shortcuts", a)) {
        let values = entry.as_array().context("Shortcut must be an array")?;
        let target = values
            .first()
            .and_then(Value::as_str)
            .context("Shortcut target missing")?;
        let name = values
            .get(1)
            .and_then(Value::as_str)
            .context("Shortcut name missing")?;
        let path = relative(&shortcut_root(global)?, &format!("{name}.lnk"))?;
        if add {
            let target = relative(dir, target)?;
            let arguments = values
                .get(2)
                .and_then(Value::as_str)
                .map(|s| scripts::expand(s, ctx))
                .unwrap_or_default();
            let icon = values
                .get(3)
                .and_then(Value::as_str)
                .map(|s| relative(dir, s))
                .transpose()?;
            windows::shortcut(
                &path,
                &target,
                &arguments,
                dir,
                icon.as_ref().map(|p| (p.as_path(), 0)),
            )?;
        } else {
            windows::remove(&path)?
        }
    }
    if let Some(module) = m.field("psmodule", a) {
        let name = util::ci_get(module, "name")
            .and_then(Value::as_str)
            .context("PowerShell module name missing")?;
        util::valid_component(name)?;
        let root = c.layout.base(global).join("modules");
        let link = root.join(name);
        windows::remove(&link)?;
        if add {
            fs::create_dir_all(&root)?;
            windows::junction(&link, dir)?;
        }
        let remaining = root.is_dir() && fs::read_dir(&root)?.next().is_some();
        windows::path_edit("PSModulePath", &root, add || remaining, global)?;
    }
    for p in m.strings("env_add_path", a)? {
        let path = relative(dir, &p)?;
        windows::path_edit(&c.layout.path_variable, &path, add, global)?;
    }
    if let Some(vars) = m.field("env_set", a).and_then(Value::as_object) {
        for (k, v) in vars {
            let value = scripts::expand(
                v.as_str().context("Environment value must be a string")?,
                ctx,
            );
            if add {
                windows::env_set(k, Some(&value), global)?
            } else if windows::env_get(k, global)?
                .is_some_and(|(old, _)| old.eq_ignore_ascii_case(&value))
            {
                windows::env_set(k, None, global)?;
            }
        }
    }
    Ok(())
}
fn persist(c: &Config, m: &Manifest, a: Architecture, ctx: &Value) -> Result<()> {
    let dir = Path::new(ctx["dir"].as_str().unwrap());
    let root = Path::new(ctx["persist_dir"].as_str().unwrap());
    for entry in items(m.field("persist", a)) {
        let (source, target) = if let Some(s) = entry.as_str() {
            (s, s)
        } else {
            let arr = entry.as_array().context("Invalid persist entry")?;
            let s = arr
                .first()
                .and_then(Value::as_str)
                .context("Persist source missing")?;
            (s, arr.get(1).and_then(Value::as_str).unwrap_or(s))
        };
        let source = relative(dir, source)?;
        let target = relative(root, target)?;
        if windows::is_link(&source) {
            continue;
        }
        let existing = fs::symlink_metadata(&source).is_ok();
        if target.exists() {
            windows::remove(&source)?
        } else if existing {
            fs::create_dir_all(target.parent().unwrap())?;
            fs::rename(&source, &target)?;
        } else {
            fs::create_dir_all(&target)?;
        }
        if let Some(p) = source.parent() {
            fs::create_dir_all(p)?;
        }
        if target.is_dir() {
            windows::junction(&source, &target)?
        } else {
            fs::hard_link(&target, &source)?;
        }
    }
    let _ = c;
    Ok(())
}
fn installer(
    c: &Config,
    m: &Manifest,
    a: Architecture,
    ctx: &Value,
    uninstall: bool,
) -> Result<()> {
    let key = if uninstall {
        "uninstaller"
    } else {
        "installer"
    };
    if let Some(field) = m.field(key, a) {
        if util::ci_get(field, "script").is_some() {
            return scripts::hook(c, util::ci_get(field, "script"), ctx);
        }
        let file = util::ci_get(field, "file")
            .and_then(Value::as_str)
            .or(ctx["fname"].as_str())
            .context("Installer file missing")?;
        let target = relative(Path::new(ctx["dir"].as_str().unwrap()), file)?;
        let mut command = Command::new(target);
        command.args(args(util::ci_get(field, "args"), ctx));
        command.current_dir(ctx["dir"].as_str().unwrap());
        let status = command.status()?;
        let code = status.code().unwrap_or(1);
        let accepted = util::ci_get(field, "exit_codes").is_some_and(|v| {
            v.as_object()
                .is_some_and(|o| o.contains_key(&code.to_string()))
                || v.as_array()
                    .is_some_and(|a| a.iter().any(|v| v.as_i64() == Some(code as i64)))
        });
        if code != 0 && !accepted {
            bail!("Package {key} failed with exit {code}")
        }
        if !uninstall && util::ci_get(field, "keep").and_then(Value::as_bool) != Some(true) {
            windows::remove(&relative(Path::new(ctx["dir"].as_str().unwrap()), file)?)?;
        }
    }
    Ok(())
}
pub fn invoke(c: &Config, action: &str, d: &Value) -> Result<Value> {
    let mut ctx = context(c, d)?;
    let name = ctx["name"].as_str().unwrap().to_owned();
    let global = ctx["global"].as_bool().unwrap_or(false);
    let original = PathBuf::from(ctx["dir"].as_str().context("Package directory missing")?);
    let root = c.layout.apps(global).join(&name);
    if !original.starts_with(&root) {
        bail!("Package directory must stay in its app root")
    }
    let a = Architecture::parse(ctx["architecture"].as_str().unwrap_or("64bit"))?;
    let m = if d["manifest"].is_object() {
        Some(query::manifest(&d["manifest"])?)
    } else {
        None
    };
    if action == "install" {
        let m = m.context("Installation manifest missing")?;
        let mut archive_index = 0;
        let kind = if m.field("innosetup", a).and_then(Value::as_bool) == Some(true) {
            Some("innounp")
        } else {
            m.field("extract_with", a).and_then(Value::as_str)
        };
        let select = m.strings("extract_dir", a)?;
        let destinations = m.strings("extract_to", a)?;
        for file in d["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let file = relative(&original, file)?;
            let to = relative(
                &original,
                destinations
                    .get(archive_index)
                    .map(String::as_str)
                    .unwrap_or("."),
            )?;
            if extract(
                c,
                &file,
                &to,
                select.get(archive_index).map(String::as_str),
                kind,
            )? {
                windows::remove(&file)?;
                archive_index += 1;
            }
        }
        scripts::hook(c, m.field("pre_install", a), &ctx)?;
        installer(c, &m, a, &ctx, false)?;
        if !c.layout.no_junction {
            windows::remove(&root.join("current"))?;
            windows::junction(&root.join("current"), &original)?;
            ctx["dir"] = json!(root.join("current"));
        }
        integrations(c, &m, a, &ctx, true)?;
        persist(c, &m, a, &ctx)?;
        scripts::hook(c, m.field("post_install", a), &ctx)?;
        if let Some(notes) = scripts::text(m.field("notes", a)) {
            eprintln!("\n{}", scripts::expand(&notes, &ctx));
        }
        if let Some(suggest) = m.field("suggest", a).and_then(Value::as_object) {
            for (label, packages) in suggest {
                eprintln!(
                    "Suggested for {label}: {}",
                    scripts::text(Some(packages)).unwrap_or_default()
                );
            }
        }
        return Ok(Value::Null);
    }
    if action == "cleanup" {
        let mut removed = Vec::new();
        if root.is_dir() {
            for e in fs::read_dir(&root)? {
                let e = e?;
                let p = e.path();
                let component = e.file_name().to_string_lossy().into_owned();
                if component != "current" && p.is_dir() && p != original {
                    windows::remove(&p)?;
                    removed.push(component);
                }
            }
        }
        return Ok(json!({"removed":removed}));
    }
    if action == "reset" {
        let m = m.context("Reset manifest missing")?;
        windows::remove(&root.join("current"))?;
        if !c.layout.no_junction {
            windows::junction(&root.join("current"), &original)?;
            ctx["dir"] = json!(root.join("current"));
        }
        integrations(c, &m, a, &ctx, true)?;
        persist(c, &m, a, &ctx)?;
        return Ok(Value::Null);
    }
    let running = windows::running(&root)?;
    if !running.is_empty() {
        bail!("Package is running:\n{}", running.join("\n"))
    }
    if root.join("current").is_dir() {
        ctx["dir"] = json!(root.join("current"));
    }
    if let Some(m) = m {
        // Failed installations may not have created persistent data yet.
        fs::create_dir_all(ctx["persist_dir"].as_str().unwrap())?;
        scripts::hook(c, m.field("pre_uninstall", a), &ctx)?;
        installer(c, &m, a, &ctx, true)?;
        integrations(c, &m, a, &ctx, false)?;
        scripts::hook(c, m.field("post_uninstall", a), &ctx)?;
    }
    windows::remove(&root.join("current"))?;
    if action == "uninstall" {
        windows::remove(&root)?;
        if d["purge"].as_bool() == Some(true) {
            windows::remove(&c.layout.base(global).join("persist").join(&name))?;
        }
    }
    Ok(Value::Null)
}
pub fn hook_helper(c: &Config, d: &Value) -> Result<()> {
    let v = &d["values"];
    let s = |i: usize| v[i].as_str().unwrap_or("");
    let global = v[1].as_bool().unwrap_or(false);
    match d["operation"].as_str().unwrap_or("") {
        "helper" => println!("{}", helper(c, s(0))?.display()),
        "ensure" => {
            fs::create_dir_all(s(0))?;
            println!("{}", s(0));
        }
        "admin" => println!("{}", windows::admin()),
        "get_env" => {
            if let Some((value, _)) = windows::env_get(s(0), global)? {
                println!("{value}")
            }
        }
        "set_env" => windows::env_set(s(0), v[1].as_str(), v[2].as_bool().unwrap_or(false))?,
        "add_path" | "remove_path" => windows::path_edit(
            &c.layout.path_variable,
            Path::new(s(0)),
            d["operation"] == "add_path",
            global,
        )?,
        "link" => windows::junction(Path::new(s(0)), Path::new(s(1)))?,
        "remove_link" => {
            if windows::is_link(Path::new(s(0))) {
                windows::remove(Path::new(s(0)))?
            }
        }
        "shim" => {
            let target = Path::new(s(0));
            let name = if s(2).is_empty() {
                target
                    .file_stem()
                    .context("Shim target missing")?
                    .to_str()
                    .context("Non-Unicode shim name")?
            } else {
                s(2)
            };
            add_shim(
                c,
                name,
                target,
                &args(v.get(3), &d["context"]).join(" "),
                global,
            )?
        }
        "rm_shim" => remove_shim(c, s(0), Path::new(s(1)).starts_with(&c.layout.global_root))?,
        "extract" => {
            extract(c, Path::new(s(0)), Path::new(s(1)), v[2].as_str(), None)?;
            if v[3].as_bool() == Some(true) {
                windows::remove(Path::new(s(0)))?;
            }
        }
        other => bail!("Unknown manifest helper {other}"),
    }
    Ok(())
}
pub fn custom_shim(c: &Config, name: &str, target: &Path, fixed: &str, global: bool) -> Result<()> {
    add_shim(c, name, target, fixed, global)
}
