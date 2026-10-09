//! Semantic terminal colors shared by the CLI and native command output.
use console::style;
use std::io::{self, IsTerminal};

#[derive(Clone, Copy, Debug)]
pub enum Tone {
    Heading,
    Primary,
    Bucket,
    Version,
    Installed,
    Success,
    Warning,
    Error,
    Secondary,
    Normal,
}
pub fn stdout_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
pub fn stderr_color() -> bool {
    io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
pub fn paint(text: &str, tone: Tone, color: bool) -> String {
    let s = style(text).force_styling(color);
    match tone {
        Tone::Heading => s.color256(117).bold().to_string(),
        Tone::Primary => s.magenta().bright().bold().to_string(),
        Tone::Bucket => s.green().to_string(),
        Tone::Version => s.magenta().to_string(),
        Tone::Installed => s.blue().to_string(),
        Tone::Success => s.green().to_string(),
        Tone::Warning => s.yellow().to_string(),
        Tone::Error => s.red().bold().to_string(),
        Tone::Secondary => s.dim().to_string(),
        Tone::Normal => text.to_owned(),
    }
}
/// Human output combines installed version and scope; structured records keep separate fields.
pub fn version_scope(version: &str, scope: &str) -> String {
    if scope.is_empty() || scope == "-" {
        version.to_owned()
    } else {
        format!("{version} ({scope})")
    }
}
pub fn state_tone(text: &str) -> Tone {
    let s = text.to_ascii_lowercase();
    if ["failed", "broken", "error", "missing", "removed", "invalid"]
        .iter()
        .any(|w| s.contains(w))
    {
        Tone::Error
    } else if [
        "outdated",
        "held",
        "hold",
        "deprecated",
        "skipped",
        "warning",
        "pending",
        "unknown",
    ]
    .iter()
    .any(|w| s.contains(w))
    {
        Tone::Warning
    } else {
        Tone::Success
    }
}
pub fn warning(text: &str) {
    eprintln!(
        "{} {}",
        paint("warning:", Tone::Warning, stderr_color()),
        paint(text, Tone::Warning, stderr_color())
    );
}
/// Tokenize already validated JSON. Escapes and UTF-8 are preserved byte for byte.
/// Keys, strings, numbers, literals and structural punctuation have distinct tones.
pub fn json(text: &str, color: bool) -> String {
    if !color {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let tone = match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                i = i.min(bytes.len());
                let mut next = i;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if bytes.get(next) == Some(&b':') {
                    Tone::Heading
                } else {
                    Tone::Success
                }
            }
            b'-' | b'0'..=b'9' => {
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    i += 1;
                }
                Tone::Version
            }
            b't' | b'f' | b'n' => {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                Tone::Warning
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                i += 1;
                Tone::Secondary
            }
            _ => {
                // JSON whitespace is ASCII; advance an entire character for defensive safety.
                i += text[i..].chars().next().unwrap().len_utf8();
                Tone::Normal
            }
        };
        out.push_str(&paint(&text[start..i], tone, true));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_highlight_preserves_escaped_unicode_and_literals() {
        let text = serde_json::to_string_pretty(&serde_json::json!({
            "key:\"中文": "string\\\"\n中文", "number": -12.5e20,
            "bool": true, "null": null, "items": [false, 42, "{}"]
        }))
        .unwrap();
        let colored = json(&text, true);
        assert_eq!(console::strip_ansi_codes(&colored), text);
        for sequence in [
            "\x1b[38;5;117m",
            "\x1b[32m",
            "\x1b[35m",
            "\x1b[33m",
            "\x1b[2m",
        ] {
            assert!(colored.contains(sequence), "missing {sequence:?}");
        }
        assert_eq!(json(&text, false), text);
        serde_json::from_str::<serde_json::Value>(&console::strip_ansi_codes(&colored)).unwrap();
    }
    #[test]
    fn priority_colors_are_distinct() {
        assert!(paint("broken", state_tone("broken"), true).contains("[31m"));
        assert!(paint("outdated", state_tone("outdated"), true).contains("[33m"));
        assert!(paint("installed", state_tone("installed"), true).contains("[32m"));
        assert_eq!(paint("installed", Tone::Success, false), "installed");
    }
}
