//! Runs only code supplied by manifests or user aliases, with an independent context adapter.
use crate::{config::Config, util};
use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
pub fn powershell() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}
pub fn hook(c: &Config, script: Option<&Value>, context: &Value) -> Result<()> {
    let Some(body) = text(script).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    run(c, &body, context, &[]).map(|_| ())
}
pub fn run(c: &Config, body: &str, context: &Value, args: &[String]) -> Result<i32> {
    let temp = tempfile::Builder::new()
        .prefix("rsc-user-script-")
        .tempdir()?;
    let path = temp.path().join("context.json");
    util::write_json(
        &path,
        &json!({"context":context,"body":body,"args":args,"exe":std::env::current_exe()?,"root":c.layout.root,"global_root":c.layout.global_root,"cache":c.layout.cache}),
    )?;
    let adapter = format!(
        r#"$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$request=Get-Content -LiteralPath {} -Raw -Encoding UTF8 | ConvertFrom-Json
$rscExecutable=$request.exe
$dir=$request.context.dir
$original_dir=$request.context.original_dir
$persist_dir=$request.context.persist_dir
$app=$request.context.name
$global=[bool]$request.context.global
$architecture=$request.context.architecture
$version=$request.context.version
$bucket=$request.context.bucket
$manifest=$request.context.manifest
$fname=$request.context.fname
$scoopdir=$request.root
$bucketsdir=Join-Path $request.root 'buckets'
$cachedir=$request.cache
function scoop {{ & $rscExecutable @args }}
function Invoke-RscHelper {{
    param([string]$Operation,[object[]]$Values)
    $p=[IO.Path]::GetTempFileName()
    try {{
        @{{operation=$Operation;values=$Values;context=$request.context}} | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $p -Encoding UTF8
        & $rscExecutable _hook $p
        if($LASTEXITCODE -ne 0){{throw "Manifest helper $Operation failed"}}
    }} finally {{Remove-Item -LiteralPath $p -ErrorAction SilentlyContinue}}
}}
function Get-HelperPath {{param($Name) Invoke-RscHelper 'helper' @($Name)}}
function shim {{param($Path,$Global,$Name,$Arguments) Invoke-RscHelper 'shim' @($Path,$Global,$Name,$Arguments)}}
function rm_shim {{param($Name,$Directory) Invoke-RscHelper 'rm_shim' @($Name,$Directory)}}
function Expand-7zipArchive {{param($Path,$DestinationPath,$ExtractDir,$Removal) Invoke-RscHelper 'extract' @($Path,$DestinationPath,$ExtractDir,$Removal)}}
function Remove-Junction {{param($Path) Invoke-RscHelper 'remove_link' @($Path)}}
function New-Junction {{param($Path,$Target) Invoke-RscHelper 'link' @($Path,$Target)}}
function Get-Env {{param($Name,$Global) Invoke-RscHelper 'get_env' @($Name,$Global)}}
function Set-Env {{param($Name,$Value,$Global) Invoke-RscHelper 'set_env' @($Name,$Value,$Global);[Environment]::SetEnvironmentVariable($Name,$Value,'Process')}}
function Add-Path {{param($Path,$Global) Invoke-RscHelper 'add_path' @($Path,$Global)}}
function Remove-Path {{param($Path,$Global) Invoke-RscHelper 'remove_path' @($Path,$Global)}}
function ensure {{param($Path) Invoke-RscHelper 'ensure' @($Path)}}
function is_admin {{ [bool]::Parse((Invoke-RscHelper 'admin' @())) }}
function warn {{Write-Warning ($args -join ' ')}}
function info {{Write-Host ($args -join ' ')}}
function abort {{throw ($args -join ' ')}}
function debug {{Write-Verbose ($args -join ' ')}}
try {{ & ([ScriptBlock]::Create($request.body)) @($request.args);if(-not $? ){{exit 1}};exit 0 }} catch {{[Console]::Error.WriteLine($_.Exception.Message);exit 1}}
"#,
        quote(&path.to_string_lossy())
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        adapter
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let mut child = Command::new(powershell());
    child
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-OutputFormat",
            "Text",
            "-InputFormat",
            "Text",
            "-EncodedCommand",
            &encoded,
        ])
        .env_remove("PSModulePath")
        .env("SCOOP", &c.layout.root)
        .env("SCOOP_GLOBAL", &c.layout.global_root)
        .env("SCOOP_CACHE", &c.layout.cache)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(dir) = context["dir"].as_str().filter(|p| Path::new(p).is_dir()) {
        child.current_dir(dir);
    }
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    child.env(
        "PATH",
        format!(
            "{};{};{}",
            c.layout.shims(false).display(),
            c.layout.shims(true).display(),
            inherited.to_string_lossy()
        ),
    );
    let status = child
        .status()
        .context("Cannot execute manifest/user script")?;
    if !status.success() {
        bail!("User script exited with {}", status.code().unwrap_or(1))
    }
    Ok(status.code().unwrap_or(0))
}
pub fn expand(text: &str, d: &Value) -> String {
    let mut out = text.to_owned();
    for key in [
        "original_dir",
        "persist_dir",
        "architecture",
        "version",
        "dir",
        "app",
        "name",
    ] {
        let value = if key == "app" { &d["name"] } else { &d[key] };
        if let Some(value) = value.as_str() {
            out = out
                .replace(&["$", "{", key, "}"].concat(), value)
                .replace(&["$", key].concat(), value);
        }
    }
    out
}
