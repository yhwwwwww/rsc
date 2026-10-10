use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rsc_core::download::{Event, Reporter};
use rsc_core::presentation::{self, Tone, paint};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::HashMap,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

pub struct Output {
    pub json: bool,
    pub command: &'static str,
    color: bool,
}
impl Output {
    pub fn new(json: bool, command: &'static str) -> Self {
        Self {
            json,
            command,
            color: presentation::stdout_color(),
        }
    }
    pub fn data(
        &self,
        data: &impl Serialize,
        warnings: &[String],
        errors: &[String],
    ) -> anyhow::Result<()> {
        let mut out = io::stdout().lock();
        serde_json::to_writer_pretty(
            &mut out,
            &json!({"schema":1,"command":self.command,"data":data,"warnings":warnings,"errors":errors}),
        )?;
        writeln!(out)?;
        Ok(())
    }
    pub fn search_table(&self, rows: &[rsc_core::search::Row], descriptions: bool) {
        let mut headers = vec!["Package", "Version", "Bucket", "Installed", "Binaries"];
        if descriptions {
            headers.push("Description");
        }
        let values = rows
            .iter()
            .map(|row| {
                let mut values = vec![
                    row.package.clone(),
                    row.version.clone(),
                    row.bucket.clone(),
                    installed_cell(&row.installations, self.color),
                    row.binaries.clone(),
                ];
                if descriptions {
                    values.push(row.description.clone());
                }
                values
            })
            .collect();
        self.table_inner(&headers, values, Some(rows));
    }
    pub fn table(&self, headers: &[&str], rows: Vec<Vec<String>>) {
        self.table_inner(headers, rows, None);
    }
    fn table_inner(
        &self,
        headers: &[&str],
        rows: Vec<Vec<String>>,
        search: Option<&[rsc_core::search::Row]>,
    ) {
        if rows.is_empty() {
            println!("{}", paint("No results.", Tone::Secondary, self.color));
            return;
        }
        let columns = headers.len();
        let mut widths: Vec<usize> = headers
            .iter()
            .map(|s| console::measure_text_width(s))
            .collect();
        for row in &rows {
            for (i, value) in row.iter().enumerate().take(columns) {
                widths[i] = widths[i].max(console::measure_text_width(value)).min(80);
            }
        }
        if io::stdout().is_terminal() {
            let available = console::Term::stdout().size().1.max(30) as usize;
            while widths.iter().sum::<usize>() + 2 * (columns - 1) > available {
                let Some((index, width)) = widths
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| **w > 5)
                    .max_by_key(|(_, w)| **w)
                else {
                    break;
                };
                let _ = width;
                widths[index] -= 1;
            }
        }
        let render = |row: &[String], body: bool, row_index: usize| -> String {
            let attention = row.iter().any(|v| v.contains("outdated"));
            row.iter()
                .enumerate()
                .map(|(i, value)| {
                    let clean = console::strip_ansi_codes(value).replace(['\r', '\n', '\t'], " ");
                    let text = if body && headers[i] == "Installed" && search.is_some() {
                        console::truncate_str(
                            &installed_cell(&search.unwrap()[row_index].installations, self.color),
                            widths[i],
                            "…",
                        )
                        .into_owned()
                    } else if body && i == 0 {
                        console::truncate_str(
                            &paint(&clean, Tone::Primary, self.color),
                            widths[i],
                            "…",
                        )
                        .into_owned()
                    } else if body {
                        clipped_cell(
                            headers[i],
                            &clean,
                            widths[i],
                            self.color,
                            attention,
                            self.command,
                        )
                    } else {
                        clip(&clean, widths[i])
                    };
                    let pad = widths[i].saturating_sub(console::measure_text_width(&text));
                    if i + 1 == columns {
                        text
                    } else {
                        format!("{text}{}", " ".repeat(pad))
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
        };
        println!(
            "{}",
            paint(
                &render(
                    &headers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    false,
                    0,
                ),
                Tone::Heading,
                self.color
            )
        );
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        for (index, row) in rows.iter().enumerate() {
            let _ = writeln!(stdout, "{}", render(row, true, index));
        }
    }
    pub fn details(&self, fields: Vec<(String, String)>) {
        let label_width = fields
            .iter()
            .map(|(k, _)| console::measure_text_width(k))
            .max()
            .unwrap_or(0)
            .min(24);
        let available = if io::stdout().is_terminal() {
            console::Term::stdout().size().1.max(30) as usize
        } else {
            usize::MAX
        };
        let value_width = available.saturating_sub(label_width + 2).max(8);
        for (key, value) in fields {
            let clean = console::strip_ansi_codes(&value)
                .replace('\r', "")
                .replace('\t', " ");
            let mut lines = Vec::new();
            for line in clean.lines() {
                let mut buffer = String::new();
                let mut width = 0;
                for ch in line.chars() {
                    let next = console::measure_text_width(&ch.to_string());
                    if width + next > value_width && !buffer.is_empty() {
                        lines.push(std::mem::take(&mut buffer));
                        width = 0;
                    }
                    buffer.push(ch);
                    width += next;
                }
                lines.push(buffer);
            }
            if lines.is_empty() {
                lines.push(String::new());
            }
            for (index, line) in lines.into_iter().enumerate() {
                let label = if index == 0 {
                    clip(&key, label_width)
                } else {
                    String::new()
                };
                let padded = format!(
                    "{}{}",
                    label,
                    " ".repeat(label_width.saturating_sub(console::measure_text_width(&label)))
                );
                println!(
                    "{}  {}",
                    paint(&padded, Tone::Heading, self.color),
                    cell(&key, &line, self.color, false, self.command)
                );
            }
        }
    }
    /// All status report sections use stdout so terminal stream merging cannot
    /// insert diagnostic lines into the installed-package table.
    pub fn status_updates(&self, buckets: &[String]) {
        if buckets.is_empty() {
            return;
        }
        println!(
            "{}  {}",
            paint("Bucket updates available", Tone::Heading, self.color),
            paint(&buckets.join(", "), Tone::Bucket, self.color)
        );
        println!(
            "{}\n",
            paint(
                "Run rsc update to refresh bucket manifests.",
                Tone::Secondary,
                self.color
            )
        );
    }
    pub fn status_diagnostics(&self, warnings: &[String], errors: &[String]) {
        if warnings.is_empty() && errors.is_empty() {
            return;
        }
        println!(
            "\n{}",
            paint("Checks needing attention", Tone::Heading, self.color)
        );
        for (messages, tone) in [(warnings, Tone::Warning), (errors, Tone::Error)] {
            for message in messages {
                let clean = console::strip_ansi_codes(message).replace('\r', "");
                for line in clean.lines() {
                    println!("  {}", paint(line, tone, self.color));
                }
            }
        }
    }
    pub fn warnings(&self, warnings: &[String]) {
        for warning in warnings {
            presentation::warning(warning);
        }
    }
    pub fn error(&self, error: &anyhow::Error) {
        if self.json {
            let _ = self.data(&serde_json::Value::Null, &[], &[format!("{error:#}")]);
        } else {
            eprintln!(
                "{} {}",
                paint("error:", Tone::Error, presentation::stderr_color()),
                paint(
                    &format!("{error:#}"),
                    Tone::Error,
                    presentation::stderr_color()
                )
            );
        }
    }
}
/// Version state colors belong to the version itself, independently for each scope.
fn installed_cell(installations: &[rsc_core::search::Installation], color: bool) -> String {
    installations
        .iter()
        .map(|installed| {
            let tone = match installed.state.as_str() {
                "current" => Tone::Installed,
                "newer" => Tone::Version,
                "broken" => Tone::Error,
                _ => Tone::Warning,
            };
            let clean =
                |text: &str| console::strip_ansi_codes(text).replace(['\r', '\n', '\t'], " ");
            let mut text = format!(
                "{} {}",
                paint(&clean(&installed.version), tone, color),
                paint(
                    &format!("({})", clean(&installed.scope)),
                    Tone::Secondary,
                    color
                )
            );
            let mut notes = Vec::new();
            if !color && installed.state != "current" {
                notes.push(installed.state.as_str());
            }
            if installed.held {
                notes.push("held");
            }
            if installed.source_unknown {
                notes.push("source unknown");
            }
            if !notes.is_empty() {
                text.push_str(&paint(
                    &format!(" [{}]", notes.join(", ")),
                    Tone::Secondary,
                    color,
                ));
            }
            text
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Determine priority from the complete value before truncating styled text.
fn clipped_cell(
    header: &str,
    value: &str,
    width: usize,
    color: bool,
    attention: bool,
    command: &str,
) -> String {
    console::truncate_str(&cell(header, value, color, attention, command), width, "…").into_owned()
}
/// Semantic styling is independent of display width; truncation understands ANSI.
fn cell(header: &str, value: &str, color: bool, attention: bool, command: &str) -> String {
    if value.is_empty() || value == "-" || value == "?" {
        return paint(value, Tone::Secondary, color);
    }
    let key = header.to_ascii_lowercase();
    let tone = match key.as_str() {
        "result" if !value.contains("failed") && !value.contains("error") => Tone::Success,
        "state" | "result" | "status" | "info" if command != "info" => {
            return value
                .split(", ")
                .map(|s| paint(s, presentation::state_tone(s), color))
                .collect::<Vec<_>>()
                .join(", ");
        }
        "package" | "package / bucket" | "name" | "app" | "command" | "setting" | "cache file" => {
            Tone::Primary
        }
        "available" | "latest version" if attention => Tone::Warning,
        "available" | "latest version" => Tone::Success,
        "installed" | "installed version" => Tone::Installed,
        "version" | "size" | "files" | "manifests" => Tone::Version,
        "dependencies" if command == "status" => Tone::Error,
        "notes" | "warning" => Tone::Warning,
        "hash" => presentation::state_tone(value),
        "value" if serde_json::from_str::<serde_json::Value>(value).is_ok() => {
            return presentation::json(value, color);
        }
        "bucket" => Tone::Bucket,
        "source" | "scope" | "arch" | "architecture" | "path" | "repository" => Tone::Secondary,
        "homepage" | "url" => Tone::Primary,
        _ => Tone::Normal,
    };
    if matches!(key.as_str(), "version" | "installed" | "installed version") {
        return value
            .split(" | ")
            .map(|value| {
                if let Some((version, suffix)) = value.split_once(" (") {
                    format!(
                        "{} {}",
                        paint(version, Tone::Installed, color),
                        paint(&format!("({suffix}"), Tone::Secondary, color)
                    )
                } else {
                    paint(value, tone, color)
                }
            })
            .collect::<Vec<_>>()
            .join(" | ");
    }
    paint(value, tone, color)
}

fn clip(text: &str, width: usize) -> String {
    if console::measure_text_width(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    for c in text.chars() {
        if console::measure_text_width(&out) + console::measure_text_width(&c.to_string()) + 1
            > width
        {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}
fn progress_style(known_size: bool, width: usize, color: bool) -> ProgressStyle {
    let template = match (known_size, width) {
        (true, 0..=59) => {
            " {spinner:.green} {msg:12!} {wide_bar:.green} {percent:>3}% {bytes_per_sec}"
        }
        (true, 60..=99) => {
            " {spinner:.green} {msg:18!} {wide_bar:.green} {percent:>3}% {bytes_per_sec} ETA {eta_precise}"
        }
        (true, _) => {
            " {spinner:.green} {msg:24!} {wide_bar:.green} {percent:>3}% {bytes}/{total_bytes} {bytes_per_sec} ETA {eta_precise}"
        }
        (false, 0..=59) => " {spinner:.green} {msg:12!} {bytes} {bytes_per_sec}",
        (false, _) => {
            " {spinner:.green} {msg:24!} {bytes} {bytes_per_sec} elapsed {elapsed_precise}"
        }
    };
    let template = if color {
        template.to_owned()
    } else {
        template.replace(":.green", "")
    };
    ProgressStyle::with_template(&template)
        .expect("valid download progress template")
        .tick_chars("+-x| ")
        .progress_chars("█░")
}

pub struct Progress {
    bars: Mutex<HashMap<usize, ProgressBar>>,
    labels: Mutex<HashMap<usize, String>>,
    multi: MultiProgress,
    terminal: bool,
    quiet: bool,
}
impl Progress {
    pub fn reporter(quiet: bool) -> Reporter {
        let display = Arc::new(Self {
            bars: Mutex::new(HashMap::new()),
            labels: Mutex::new(HashMap::new()),
            multi: MultiProgress::new(),
            terminal: io::stderr().is_terminal(),
            quiet,
        });
        Arc::new(move |event| display.event(event))
    }
    fn event(&self, event: Event) {
        if self.quiet {
            return;
        }
        let id = match &event {
            Event::Started { id, .. }
            | Event::Progress { id, .. }
            | Event::Message { id, .. }
            | Event::Finished { id, .. }
            | Event::Failed { id } => *id,
        };
        let (label, first) = if let Ok(mut labels) = self.labels.lock() {
            match &event {
                Event::Started { label, .. } => {
                    let first = labels.insert(id, label.clone()).is_none();
                    (label.clone(), first)
                }
                _ => (
                    labels
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| "download".into()),
                    false,
                ),
            }
        } else {
            ("download".into(), false)
        };
        if !self.terminal {
            match event {
                Event::Started { .. } if first => eprintln!("Download {label}"),
                Event::Message { text, .. } => eprintln!("  {label}: {text}"),
                Event::Finished {
                    path,
                    bytes,
                    cached,
                    ..
                } => eprintln!(
                    "  {} {} ({})",
                    paint(
                        if cached { "Cached" } else { "Saved" },
                        Tone::Success,
                        presentation::stderr_color()
                    ),
                    paint(
                        &path.display().to_string(),
                        Tone::Secondary,
                        presentation::stderr_color()
                    ),
                    size(bytes)
                ),
                _ => {}
            }
            return;
        }
        let Ok(mut bars) = self.bars.lock() else {
            return;
        };
        match event {
            Event::Started { id, label, total } => {
                if let Some(old) = bars.remove(&id) {
                    old.finish_and_clear();
                }
                let bar = self.multi.add(match total {
                    Some(n) => ProgressBar::new(n),
                    None => ProgressBar::new_spinner(),
                });
                let width = console::Term::stderr().size().1 as usize;
                bar.set_style(progress_style(
                    total.is_some(),
                    width,
                    presentation::stderr_color(),
                ));
                let label = console::strip_ansi_codes(&label).replace(['\r', '\n', '\t'], " ");
                bar.set_message(paint(&label, Tone::Primary, presentation::stderr_color()));
                bar.enable_steady_tick(Duration::from_millis(200));
                bars.insert(id, bar);
            }
            Event::Progress { id, bytes } => {
                if let Some(bar) = bars.get(&id) {
                    bar.inc(bytes);
                }
            }
            Event::Message { text, .. } => {
                let _ = self.multi.println(format!(
                    "{}: {text}",
                    paint(&label, Tone::Primary, presentation::stderr_color())
                ));
            }
            Event::Finished {
                id,
                path,
                bytes,
                cached,
            } => {
                if let Some(bar) = bars.remove(&id) {
                    bar.finish_and_clear();
                }
                let _ = self.multi.println(format!(
                    "{}  {}  {}",
                    paint(&label, Tone::Primary, presentation::stderr_color()),
                    paint(
                        if cached { "Cached" } else { "Saved" },
                        Tone::Success,
                        presentation::stderr_color()
                    ),
                    paint(
                        &format!("{} ({})", path.display(), size(bytes)),
                        Tone::Secondary,
                        presentation::stderr_color()
                    )
                ));
            }
            Event::Failed { id } => {
                if let Some(bar) = bars.remove(&id) {
                    bar.finish_and_clear();
                }
            }
        }
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        if let Ok(bars) = self.bars.lock() {
            for bar in bars.values() {
                bar.finish_and_clear();
            }
        }
    }
}

pub fn size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB"] {
        value /= 1024.0;
        unit = next;
        if value < 1024.0 {
            break;
        }
    }
    format!("{value:.1} {unit}")
}
#[derive(Serialize)]
pub struct CacheEntry {
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_colors_only_installed_version_and_keeps_each_scope_state() {
        use rsc_core::search::Installation;
        let make = |scope: &str, state: &str, version: &str, held: bool| Installation {
            scope: scope.into(),
            state: state.into(),
            version: version.into(),
            held,
            source_unknown: false,
        };
        let installs = [
            make("global", "current", "2.0", false),
            make("user", "outdated", "1.9", true),
        ];
        let colored = installed_cell(&installs, true);
        assert_eq!(
            console::strip_ansi_codes(&colored),
            "2.0 (global) | 1.9 (user) [held]"
        );
        assert!(colored.contains("\x1b[38;5;75m2.0") && colored.contains("\x1b[33m1.9"));
        assert!(!colored.contains("\x1b[38;5;75mglobal") && !colored.contains("\x1b[33muser"));
        assert_eq!(
            installed_cell(&installs, false),
            "2.0 (global) | 1.9 (user) [outdated, held]"
        );
        for (state, code) in [
            ("broken", "\x1b[31m"),
            ("newer", "\x1b[35m"),
            ("unknown (nightly)", "\x1b[33m"),
        ] {
            assert!(installed_cell(&[make("user", state, "1", false)], true).contains(code));
        }
    }
    #[test]
    fn heading_subject_bucket_and_scoped_versions_have_distinct_styles() {
        let heading = paint("Package", Tone::Heading, true);
        let primary = paint("git", Tone::Primary, true);
        let bucket = cell("Bucket", "main", true, false, "search");
        assert!(heading.contains("\x1b[38;5;117m"));
        assert!(primary.contains("\x1b[38;5;13m") && !primary.contains("38;5;117"));
        assert!(bucket.contains("\x1b[32m") && !bucket.contains("\x1b[2m"));
        let version = cell("Installed", "2.48.1 (user)", true, false, "info");
        assert!(version.contains("\x1b[38;5;75m2.48.1") && !version.contains("\x1b[38;5;75m(user)"));
        assert_eq!(console::strip_ansi_codes(&version), "2.48.1 (user)");
        for width in [40, 80, 120] {
            for known in [false, true] {
                for color in [false, true] {
                    let _ = progress_style(known, width, color);
                }
            }
        }
    }
    #[test]
    fn table_cells_have_semantic_colors_and_plain_mode() {
        let cases = [
            ("Package", "jq", "\x1b[38;5;13m"),
            ("Version", "1.8.1", "\x1b[35m"),
            ("State", "outdated", "\x1b[33m"),
            ("State", "broken", "\x1b[31m"),
            ("State", "installed", "\x1b[32m"),
            ("Bucket", "main", "\x1b[32m"),
            ("Available", "2.0", "\x1b[33m"),
            ("Dependencies", "missing", "\x1b[31m"),
        ];
        for (key, text, code) in cases {
            let colored = cell(key, text, true, true, "status");
            assert!(colored.contains(code), "{key}: {colored:?}");
            assert_eq!(console::strip_ansi_codes(&colored), text);
            assert_eq!(cell(key, text, false, true, "status"), text);
        }
    }
    #[test]
    fn unicode_clipping_and_mixed_state_colors() {
        assert_eq!(clip("中文内容", 5), "中文…");
        let s = cell("State", "failed, hold, outdated", true, false, "status");
        assert_eq!(console::strip_ansi_codes(&s), "failed, hold, outdated");
        assert!(s.contains("\x1b[31m") && s.contains("\x1b[33m"));
        assert_eq!(console::measure_text_width(&s), 22);
    }
    #[test]
    fn clipped_states_keep_the_original_priority() {
        for (value, code) in [
            ("outdated", "\x1b[33m"),
            ("failed", "\x1b[31m"),
            ("installed", "\x1b[32m"),
        ] {
            let colored = clipped_cell("State", value, 5, true, false, "status");
            assert!(colored.contains(code), "{value}: {colored:?}");
            assert_eq!(console::measure_text_width(&colored), 5);
            assert_eq!(console::strip_ansi_codes(&colored), clip(value, 5));
            assert_eq!(
                clipped_cell("State", value, 5, false, false, "status"),
                clip(value, 5)
            );
        }
    }
}
