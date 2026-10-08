//! Independent Rust implementation of Scoop-compatible package behavior.
//! PowerShell is used only for scripts supplied by manifests or user aliases.
pub mod commands;
pub mod lifecycle;
pub mod query;
pub mod scripts;
pub mod windows;
use crate::{config::Config, util};
use anyhow::{Result, bail};
use serde_json::{Value, json};
#[derive(Debug)]
pub struct CommandFailure(pub i32);
impl std::fmt::Display for CommandFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Command exited with {}", self.0)
    }
}
impl std::error::Error for CommandFailure {}
pub fn invoke(c: &Config, action: &str, d: Value) -> Result<Value> {
    match action {
        "admin" => {
            if !windows::admin() {
                bail!("Global changes require administrator rights")
            };
            Ok(json!(true))
        }
        "resolve" => query::resolve(c, &d),
        "prepare" => query::prepare(c, &d),
        "filename" => Ok(json!(
            d["urls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| query::filename(v.as_str().unwrap_or("")))
                .collect::<Result<Vec<_>>>()?
        )),
        "match" | "search_match" | "search_remote" | "latest" | "info" | "status" => {
            query::invoke(c, action, &d)
        }
        "install" | "uninstall" | "unlink" | "reset" | "cleanup" => {
            lifecycle::invoke(c, action, &d)
        }
        "headers" => query::headers(c, d["url"].as_str().unwrap_or("")),
        "special" => query::special(c, d["url"].as_str().unwrap_or("")),
        "authenticated" => {
            windows::download(c, &d)?;
            Ok(Value::Null)
        }
        "config" => {
            windows::isolated_path(c, d["value"].as_str().unwrap_or(""))?;
            Ok(Value::Null)
        }
        "command" => commands::invoke(c, &d),
        "home" if d["url"].is_string() => {
            windows::open_url(d["url"].as_str().unwrap())?;
            Ok(Value::Null)
        }
        "home" => {
            let r = query::resolve(
                c,
                &json!({"input":d["input"].as_str().or(d["package"].as_str()).unwrap_or("")}),
            )?;
            let home = util::ci_get(&r["manifest"], "homepage")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("No homepage in manifest"))?;
            windows::open_url(home)?;
            Ok(Value::Null)
        }
        "cat" => {
            let text = serde_json::to_string_pretty(&d["manifest"])?;
            if let Some(style) = c.text("cat_style")?.filter(|s| !s.is_empty()) {
                use std::io::Write;
                let mut p = std::process::Command::new("bat")
                    .args(["--language=json", "--style", &style, "-"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()?;
                p.stdin.take().unwrap().write_all(text.as_bytes())?;
                if !p.wait()?.success() {
                    bail!("Manifest viewer failed")
                }
            } else {
                println!("{text}");
            }
            Ok(Value::Null)
        }
        "confirm_install" => {
            println!("{}", serde_json::to_string_pretty(&d["manifest"])?);
            eprint!("Install this manifest? [y/N] ");
            use std::io::Write;
            std::io::stderr().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            Ok(json!(matches!(
                answer.trim().to_ascii_lowercase().as_str(),
                "y" | "yes"
            )))
        }
        _ => bail!("Unknown native action: {action}"),
    }
}
pub mod attributes;
pub mod known;
pub mod metalink;
