//! Parses download redirection metadata without an external downloader.
use anyhow::{Result, bail};
use quick_xml::{Reader, events::Event};
#[derive(Debug)]
pub struct Resource {
    pub urls: Vec<String>,
    pub hash: Option<String>,
}
pub fn parse(bytes: &[u8], base: &str) -> Result<Option<Resource>> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Ok(None);
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.trim_start_matches('\u{feff}').trim_start(),
        Err(_) => return Ok(None),
    };
    if !text.starts_with('<') || !text.contains("metalink") {
        return Ok(None);
    }
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut root = false;
    let mut files = 0;
    let mut capture = None;
    let mut algorithm = String::new();
    let mut urls = Vec::new();
    let mut hash = None;
    loop {
        match reader.read_event()? {
            Event::Start(ref event) => {
                let name = event.local_name();
                match name.as_ref() {
                    b"metalink" => root = true,
                    b"file" => {
                        files += 1;
                        if files > 1 {
                            bail!("A manifest URL must resolve to one Metalink file")
                        }
                    }
                    b"url" => capture = Some("url"),
                    b"hash" => {
                        capture = Some("hash");
                        algorithm = event
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| String::from_utf8_lossy(&a.value).to_string())
                            .unwrap_or_default();
                    }
                    _ => {}
                }
            }
            Event::Empty(ref event) if event.local_name().as_ref() == b"file" => {
                files += 1;
                if files > 1 {
                    bail!("A manifest URL must resolve to one Metalink file")
                }
            }
            Event::Text(ref event) => {
                if let Some(kind) = capture {
                    let value = event.decode()?.to_string();
                    if kind == "url" {
                        let url = url::Url::parse(base)?.join(&value)?;
                        if matches!(url.scheme(), "http" | "https" | "ftp") {
                            urls.push(url.to_string())
                        }
                    } else if matches!(algorithm.as_str(), "sha-256" | "sha256") {
                        hash = Some(value)
                    }
                }
            }
            Event::End(ref e) => {
                if matches!(e.local_name().as_ref(), b"url" | b"hash") {
                    capture = None
                }
            }
            Event::DocType(_) => bail!("Metalink document types are unsupported"),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root {
        return Ok(None);
    }
    if urls.is_empty() {
        bail!("Metalink has no usable resource URLs")
    }
    urls.sort_by_key(|u| if u.starts_with("https:") { 0 } else { 1 });
    Ok(Some(Resource { urls, hash }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_and_hash() {
        let m=parse(br#"<?xml version="1.0"?><metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="a"><hash type="sha-256">abc</hash><url>https://example.invalid/a</url></file></metalink>"#,"https://example.invalid/meta").unwrap().unwrap();
        assert_eq!(m.hash.as_deref(), Some("abc"));
        assert_eq!(m.urls.len(), 1);
    }
    #[test]
    fn ordinary_xml_is_not_redirect() {
        assert!(
            parse(b"<document/>", "https://example.invalid/a")
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn multi_file_rejected() {
        assert!(
            parse(
                b"<metalink><file/><file><url>https://example.invalid/a</url></file></metalink>",
                "https://example.invalid/a"
            )
            .is_err()
        );
    }
}
