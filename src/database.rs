use crate::{bucket, config::Config, manifest::Architecture, util};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::{fs, time::Duration};
pub fn enabled(config: &Config) -> bool {
    config
        .get("use_sqlite_cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
fn open(config: &Config) -> Result<Connection> {
    fs::create_dir_all(&config.layout.root)?;
    let db = Connection::open(config.layout.root.join("scoop.db"))?;
    db.busy_timeout(Duration::from_secs(30))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS app (
        name TEXT NOT NULL COLLATE NOCASE, description TEXT NOT NULL, version TEXT NOT NULL,
        bucket VARCHAR NOT NULL, manifest JSON NOT NULL, binary TEXT, shortcut TEXT,
        dependency TEXT, suggest TEXT, PRIMARY KEY(name, version, bucket));",
    )?;
    Ok(db)
}
pub fn refresh(config: &Config) -> Result<()> {
    let mut db = open(config)?;
    let tx = db.transaction()?;
    let arch = config
        .get("default_architecture")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| Some(v.to_string()))
        })
        .and_then(|s| Architecture::parse(&s).ok())
        .unwrap_or_else(Architecture::native);
    let strip_binary = regex::Regex::new(r"(?i).*?([^\\/]+)?(\.(exe|bat|cmd|ps1|jar|py))$")?;
    for p in bucket::index(&config.layout.buckets())?.packages {
        let binary = p
            .manifest
            .field("bin", arch)
            .map(|v| {
                let items = if let Some(a) = v.as_array() {
                    a.clone()
                } else {
                    vec![v.clone()]
                };
                items
                    .iter()
                    .filter_map(|item| {
                        let target = if let Some(s) = item.as_str() {
                            s.to_owned()
                        } else if let Some(a) = item.as_array() {
                            format!(
                                "{}.{}",
                                a.get(1).and_then(|v| v.as_str()).unwrap_or(""),
                                a.first()
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .split('.')
                                    .next_back()
                                    .unwrap_or("")
                            )
                        } else {
                            return None;
                        };
                        Some(strip_binary.replace(&target, "$1").into_owned())
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();
        let suggest = util::ci_get(&p.manifest.raw, "suggest")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.values()
                    .flat_map(|v| {
                        if let Some(a) = v.as_array() {
                            a.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>()
                        } else {
                            v.as_str().into_iter().collect()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();
        let shortcuts = p
            .manifest
            .field("shortcuts", arch)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| {
                        v.get(1)
                            .and_then(|v| v.as_str())
                            .map(|s| s.rsplit(['\\', '/']).next().unwrap_or(s))
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();
        tx.execute("INSERT OR REPLACE INTO app(name,description,version,bucket,manifest,binary,shortcut,dependency,suggest) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![p.name, p.manifest.description(), p.manifest.version()?, p.bucket,
            p.manifest.text, binary, shortcuts,
            p.manifest.strings("depends", arch)?.join(" | "),
            suggest])?;
    }
    tx.commit()?;
    Ok(())
}
pub fn historical(
    config: &Config,
    name: &str,
    bucket: &str,
    version: &str,
) -> Result<Option<String>> {
    if !enabled(config) || !config.layout.root.join("scoop.db").is_file() {
        return Ok(None);
    }
    Ok(open(config)?
        .query_row(
            "SELECT manifest FROM app WHERE name=?1 AND bucket=?2 COLLATE NOCASE AND version=?3",
            params![name, bucket, version],
            |r| r.get(0),
        )
        .optional()?)
}
pub fn remove_bucket(config: &Config, name: &str) -> Result<()> {
    if enabled(config) {
        open(config)?.execute("DELETE FROM app WHERE bucket=?1 COLLATE NOCASE", [name])?;
    }
    Ok(())
}

pub fn search(config: &Config, query: &str) -> Result<serde_json::Value> {
    let db = open(config)?;
    let mut statement=db.prepare("SELECT name,bucket,version,description,binary,shortcut FROM app WHERE name LIKE ?1 OR binary LIKE ?1 OR shortcut LIKE ?1")?;
    let rows=statement.query_map([format!("%{query}%")],|r|Ok(serde_json::json!({
        "package":r.get::<_,String>(0)?,"bucket":r.get::<_,String>(1)?,"version":r.get::<_,String>(2)?,
        "description":r.get::<_,String>(3)?,"bins":r.get::<_,Option<String>>(4)?,"shortcuts":r.get::<_,Option<String>>(5)?
    })))?.collect::<std::result::Result<Vec<_>,_>>()?;
    crate::native::invoke(config, "latest", serde_json::json!({"rows":rows}))
}

pub fn remove_names(config: &Config, bucket: &str, names: &[String]) -> Result<()> {
    if enabled(config) && !names.is_empty() {
        let mut db = open(config)?;
        let tx = db.transaction()?;
        for name in names {
            tx.execute(
                "DELETE FROM app WHERE name=?1 AND bucket=?2 COLLATE NOCASE",
                params![name, bucket],
            )?;
        }
        tx.commit()?;
    }
    Ok(())
}
