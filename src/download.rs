use crate::{
    config::Config,
    manifest::{Architecture, DownloadFile, Manifest},
    util::{self, FileLock},
};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client, Response, StatusCode,
    header::{
        ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_RANGE, ETAG, HeaderMap,
        HeaderName, HeaderValue, IF_RANGE, LAST_MODIFIED, LOCATION, RANGE, RETRY_AFTER,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

#[derive(Clone, Debug)]
pub enum Event {
    Failed {
        id: usize,
    },
    Started {
        id: usize,
        label: String,
        total: Option<u64>,
    },
    Progress {
        id: usize,
        bytes: u64,
    },
    Message {
        id: usize,
        text: String,
    },
    Finished {
        id: usize,
        path: PathBuf,
        bytes: u64,
        cached: bool,
    },
}
pub type Reporter = Arc<dyn Fn(Event) + Send + Sync>;
#[derive(Clone)]
pub struct Task {
    pub id: usize,
    pub label: String,
    pub app: String,
    pub version: String,
    pub file: DownloadFile,
    pub headers: HeaderMap,
}
#[derive(Debug, Serialize)]
pub struct Downloaded {
    pub package: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub cached: bool,
    pub verified: bool,
}
#[derive(Clone)]
pub struct Downloader {
    client: Client,
    cache: PathBuf,
    options: Options,
    hosts: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    token: Option<String>,
    force: bool,
    config: Config,
}
#[derive(Clone)]
pub struct Options {
    pub threads: usize,
    pub concurrent: usize,
    pub per_host: usize,
    pub split_size: u64,
    pub retries: u64,
}
struct Leased {
    response: Response,
    _permit: OwnedSemaphorePermit,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct State {
    schema: u32,
    url_fingerprint: String,
    final_fingerprint: String,
    hash: Option<String>,
    length: u64,
    etag: Option<String>,
    modified: Option<String>,
    segments: usize,
}
struct Probe {
    length: Option<u64>,
    ranges: bool,
    etag: Option<String>,
    modified: Option<String>,
    final_url: String,
}
#[derive(Debug)]
struct RangeRejected;
impl std::fmt::Display for RangeRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Server did not honor the requested byte range or resource validator")
    }
}
impl std::error::Error for RangeRejected {}
#[derive(Debug)]
struct HttpFailure {
    status: StatusCode,
    retry_after: Option<u64>,
}
impl std::fmt::Display for HttpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.status)
    }
}
impl std::error::Error for HttpFailure {}

impl Downloader {
    pub fn new(config: &Config) -> Result<Self> {
        let number = |key: &str, default: u64, zero: bool, max: u64| -> Result<u64> {
            let n = config.number(key, default)?;
            if (!zero && n == 0) || n > max {
                bail!(
                    "Configuration {key} must be {}..={max}",
                    if zero { 0 } else { 1 }
                );
            }
            Ok(n)
        };
        let options = Options {
            threads: number("download.threads", 4, false, 64)? as usize,
            concurrent: number("download.concurrent", 4, false, 64)? as usize,
            per_host: number("download.per_host", 8, false, 64)? as usize,
            split_size: number("download.split_size", 4194304, false, u64::MAX)?,
            retries: number("download.retries", 3, true, 10)?,
        };
        let timeout = number("download.timeout", 30, false, 3600)?;
        let mut builder = Client::builder()
            .user_agent(
                config
                    .text("user_agent")?
                    .unwrap_or_else(|| format!("rsc/{}", env!("CARGO_PKG_VERSION"))),
            )
            .connect_timeout(Duration::from_secs(timeout.min(30)))
            .read_timeout(Duration::from_secs(timeout))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(proxy) = config.text("proxy")?.filter(|s| !s.is_empty()) {
            if proxy.eq_ignore_ascii_case("none") {
                builder = builder.no_proxy();
            } else if !proxy.eq_ignore_ascii_case("default") {
                if !windows_proxy(&proxy) {
                    builder = builder.proxy(scoop_proxy(&proxy)?);
                } else {
                    builder = builder.no_proxy();
                }
            }
        }
        let token = std::env::var("SCOOP_GH_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .or(config.text("gh_token")?.filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("GH_TOKEN").ok().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty()));
        Ok(Self {
            client: builder.build()?,
            cache: config.layout.cache.clone(),
            options,
            hosts: Arc::new(Mutex::new(HashMap::new())),
            token,
            force: false,
            config: config.clone(),
        })
    }
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
    pub fn concurrency(&self) -> usize {
        self.options.concurrent
    }
    async fn send(
        &self,
        input: &str,
        headers: &HeaderMap,
        range: Option<&str>,
        validator: Option<&str>,
    ) -> Result<Leased> {
        let mut url = url::Url::parse(input)?;
        url.set_fragment(None);
        let mut headers = headers.clone();
        if url.scheme() == "https"
            && url.host_str() == Some("api.github.com")
            && url.port_or_known_default() == Some(443)
            && (range.is_some() || url.path().contains("/releases/assets/"))
        {
            headers
                .entry(reqwest::header::ACCEPT)
                .or_insert(HeaderValue::from_static("application/octet-stream"));
        }
        if url.scheme() == "https"
            && url.host_str() == Some("api.github.com")
            && url.port_or_known_default() == Some(443)
            && !headers.contains_key(AUTHORIZATION)
        {
            if let Some(token) = &self.token {
                let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| anyhow!("Invalid GitHub token"))?;
                value.set_sensitive(true);
                headers.insert(AUTHORIZATION, value);
            }
        }
        for redirect in 0..=10 {
            let host = format!(
                "{}:{}",
                url.host_str().context("URL has no host")?,
                url.port_or_known_default().unwrap_or(443)
            );
            let semaphore = {
                let mut hosts = self
                    .hosts
                    .lock()
                    .map_err(|_| anyhow!("Download scheduler lock failed"))?;
                hosts
                    .entry(host)
                    .or_insert_with(|| Arc::new(Semaphore::new(self.options.per_host)))
                    .clone()
            };
            let permit = semaphore.acquire_owned().await?;
            let mut request = self
                .client
                .get(url.clone())
                .headers(headers.clone())
                .header(ACCEPT_ENCODING, "identity");
            if let Some(range) = range {
                request = request.header(RANGE, range);
            }
            if let Some(validator) = validator {
                request = request.header(IF_RANGE, validator);
            }
            let response = request.send().await.map_err(|e| e.without_url())?;
            if [301, 302, 303, 307, 308].contains(&response.status().as_u16()) {
                if redirect == 10 {
                    bail!("Too many HTTP redirects");
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .context("HTTP redirect has no Location")?
                    .to_str()?;
                let next = url.join(location).context("Invalid redirect URL")?;
                if !["http", "https"].contains(&next.scheme()) {
                    bail!("Redirect uses an unsupported protocol");
                }
                if url.scheme() == "https" && next.scheme() == "http" {
                    bail!("Refusing an HTTPS-to-HTTP redirect");
                }
                if url.origin() != next.origin() {
                    headers.clear();
                }
                drop(response);
                drop(permit);
                url = next;
                continue;
            }
            return Ok(Leased {
                response,
                _permit: permit,
            });
        }
        unreachable!()
    }
    async fn pause(&self, error: &anyhow::Error, attempt: u64) -> Result<()> {
        if error.is::<RangeRejected>() {
            return Err(anyhow!(RangeRejected));
        }
        if let Some(http) = error.downcast_ref::<HttpFailure>() {
            if !http.status.is_server_error() && ![408, 429].contains(&http.status.as_u16()) {
                return Err(anyhow!("HTTP {}", http.status));
            }
        }
        let delay = error
            .downcast_ref::<HttpFailure>()
            .and_then(|h| h.retry_after)
            .unwrap_or(1u64 << attempt.min(5))
            .min(60);
        tokio::time::sleep(Duration::from_secs(delay)).await;
        Ok(())
    }
    async fn probe(&self, task: &Task) -> Result<Probe> {
        let mut last = None;
        for attempt in 0..=self.options.retries {
            let result = async {
                let mut lease = self
                    .send(&task.file.url, &task.headers, Some("bytes=0-0"), None)
                    .await?;
                let response = &mut lease.response;
                let status = response.status();
                if ![200, 206, 416].contains(&status.as_u16()) {
                    return Err(http_error(response));
                }
                check_encoding(response)?;
                let length = if status == StatusCode::PARTIAL_CONTENT {
                    let (start, end, total) =
                        content_range(response).ok_or_else(|| anyhow!(RangeRejected))?;
                    if start != 0 || end != 0 || total == 0 {
                        return Err(anyhow!(RangeRejected));
                    }
                    let mut received = 0;
                    while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
                        received += chunk.len();
                        if received > 1 {
                            return Err(anyhow!(RangeRejected));
                        }
                    }
                    if received != 1 {
                        bail!("Range probe body has the wrong length");
                    }
                    Some(total)
                } else if status == StatusCode::RANGE_NOT_SATISFIABLE {
                    if response
                        .headers()
                        .get(CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        == Some("bytes */0")
                    {
                        Some(0)
                    } else {
                        None
                    }
                } else {
                    response.content_length()
                };
                Ok(Probe {
                    length,
                    ranges: status == StatusCode::PARTIAL_CONTENT,
                    etag: header_text(response, ETAG),
                    modified: header_text(response, LAST_MODIFIED),
                    final_url: response.url().to_string(),
                })
            }
            .await;
            match result {
                Ok(probe) => return Ok(probe),
                Err(error) => {
                    if error.is::<RangeRejected>() {
                        return Ok(Probe {
                            length: None,
                            ranges: false,
                            etag: None,
                            modified: None,
                            final_url: task.file.url.clone(),
                        });
                    }
                    if attempt < self.options.retries {
                        self.pause(&error, attempt).await?;
                    }
                    last = Some(error);
                }
            }
        }
        Err(last.context("Download probe failed")?)
    }
    pub fn cache_directory(mut self, directory: PathBuf) -> Self {
        self.cache = directory;
        self
    }
    pub async fn manifest(&self, url: &str) -> Result<Manifest> {
        Manifest::parse(self.text(url).await?)
    }
    pub async fn text(&self, url: &str) -> Result<String> {
        if self
            .config
            .text("proxy")?
            .is_some_and(|s| windows_proxy(&s))
        {
            let temp = tempfile::tempdir()?;
            let target = temp.path().join("body");
            crate::native::invoke(
                &self.config,
                "authenticated",
                serde_json::json!({"url":url,"to":target}),
            )?;
            if fs::metadata(&target)?.len() > 16 * 1024 * 1024 {
                bail!("Remote JSON exceeds 16 MiB");
            }
            return util::read_text(&target);
        }
        let mut response = self.send(url, &HeaderMap::new(), None, None).await?;
        if !response.response.status().is_success() {
            return Err(http_error(&response.response));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .response
            .chunk()
            .await
            .map_err(|e| e.without_url())?
        {
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                bail!("Remote manifest exceeds 16 MiB");
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(bytes).context("Remote manifest is not UTF-8")?;
        Ok(text.trim_start_matches('\u{feff}').to_owned())
    }
    pub async fn download(&self, task: Task, report: Reporter) -> Result<Downloaded> {
        let id = task.id;
        let result = self.perform(task, report.clone()).await;
        if result.is_err() {
            report(Event::Failed { id });
        }
        result
    }
    async fn perform(&self, task: Task, report: Reporter) -> Result<Downloaded> {
        self.perform_depth(task, report, 0).await
    }
    async fn perform_depth(
        &self,
        mut task: Task,
        report: Reporter,
        depth: usize,
    ) -> Result<Downloaded> {
        if depth > 3 {
            bail!("Too many Metalink redirects");
        }
        util::valid_name(&task.app)?;
        util::valid_component(&task.version)?;
        let hash = Hash::parse(task.file.hash.as_deref())?;
        let path = cache_path(&self.cache, &task.app, &task.version, &task.file.url)?;
        let key = cache_key(&path)?;
        let _lock = FileLock::acquire(&self.cache.join(".rsc-locks").join(format!("{key}.lock")))?;
        let state_dir = self.cache.join(".rsc-downloads").join(&key);
        if path.try_exists()? && !self.force {
            let check_path = path.clone();
            let check_hash = hash.clone();
            let valid =
                tokio::task::spawn_blocking(move || verify(&check_path, check_hash.as_ref()))
                    .await??;
            if valid {
                let bytes = fs::metadata(&path)?.len();
                report(Event::Started {
                    id: task.id,
                    label: task.label.clone(),
                    total: Some(bytes),
                });
                if hash.is_none() {
                    report(Event::Message {
                        id: task.id,
                        text: "No manifest hash: existing cache has not been integrity-verified"
                            .into(),
                    });
                }
                report(Event::Finished {
                    id: task.id,
                    path: path.clone(),
                    bytes,
                    cached: true,
                });
                return Ok(Downloaded {
                    package: task.app,
                    path,
                    bytes,
                    cached: true,
                    verified: hash.is_some(),
                });
            }
            report(Event::Message {
                id: task.id,
                text: "Cached file failed its hash; downloading a replacement".into(),
            });
        }
        fs::create_dir_all(&state_dir)?;
        util::write_json(
            &state_dir.join("package.json"),
            &serde_json::json!({"package":task.app,"version":task.version}),
        )?;
        let parsed = url::Url::parse(&task.file.url)?;
        let host = parsed.host_str().unwrap_or("");
        if host.ends_with(".fosshub.com")
            || host == "fosshub.com"
            || host.ends_with(".sourceforge.net")
            || host == "sourceforge.net"
            || (host == "github.com" && self.token.is_some())
        {
            let value = crate::native::invoke(
                &self.config,
                "special",
                serde_json::json!({"url":task.file.url}),
            )?;
            task.file.url = value
                .as_str()
                .context("Special URL resolver returned no URL")?
                .into();
        }
        let ftp = task.file.url.starts_with("ftp://");
        let file_source = task.file.url.starts_with("file://");
        let authenticated = self
            .config
            .text("proxy")?
            .is_some_and(|s| windows_proxy(&s));
        if self.config.get("private_hosts").is_some() {
            let headers = crate::native::invoke(
                &self.config,
                "headers",
                serde_json::json!({"url":task.file.url}),
            )?;
            if let Some(headers) = headers.as_object() {
                for (name, value) in headers {
                    task.headers.insert(
                        HeaderName::from_bytes(name.as_bytes())?,
                        HeaderValue::from_str(
                            value.as_str().context("Invalid private_hosts header")?,
                        )?,
                    );
                }
            }
        }
        let probe = if ftp || file_source || authenticated {
            Probe {
                length: None,
                ranges: false,
                etag: None,
                modified: None,
                final_url: task.file.url.clone(),
            }
        } else {
            self.probe(&task).await?
        };
        report(Event::Started {
            id: task.id,
            label: task.label.clone(),
            total: probe.length,
        });
        if hash.is_none() {
            report(Event::Message {
                id: task.id,
                text: "Manifest has no hash; integrity cannot be verified".into(),
            });
        }
        let segmented = probe.ranges
            && probe.length.is_some_and(|n| n >= self.options.split_size)
            && self.options.threads > 1;
        let bytes = if file_source {
            let source = url::Url::parse(&task.file.url)?
                .to_file_path()
                .map_err(|_| anyhow!("Invalid local file URL"))?;
            let destination = state_dir.join("complete");
            let count = tokio::task::spawn_blocking(move || -> Result<u64> {
                Ok(fs::copy(source, destination)?)
            })
            .await??;
            report(Event::Progress {
                id: task.id,
                bytes: count,
            });
            count
        } else if authenticated && !ftp {
            crate::native::invoke(
                &self.config,
                "authenticated",
                serde_json::json!({"url":task.file.url,"to":state_dir.join("complete"),"cookie":task.headers.get(reqwest::header::COOKIE).and_then(|h|h.to_str().ok())}),
            )?;
            fs::metadata(state_dir.join("complete"))?.len()
        } else if ftp {
            let source = task.file.url.clone();
            let staged = state_dir.join("complete");
            let proxy = self.config.text("proxy")?;
            let reporter = report.clone();
            let id = task.id;
            tokio::task::spawn_blocking(move || {
                crate::ftp::transfer(&source, &staged, proxy.as_deref(), reporter, id)
            })
            .await??
        } else if segmented {
            match self
                .segmented(&task, &probe, &state_dir, report.clone())
                .await
            {
                Ok(bytes) => bytes,
                Err(error) if error.is::<RangeRejected>() => {
                    report(Event::Message {
                        id: task.id,
                        text: "Range response changed; restarting with one stream".into(),
                    });
                    self.single(&task, &state_dir, report.clone()).await?
                }
                Err(error) => return Err(error),
            }
        } else {
            report(Event::Message {
                id: task.id,
                text: if probe.ranges {
                    "Using one stream for this file"
                } else {
                    "Server does not support reliable ranges; using one stream"
                }
                .into(),
            });
            self.single(&task, &state_dir, report.clone()).await?
        };
        report(Event::Message {
            id: task.id,
            text: "Checking completed file".into(),
        });
        let staged = state_dir.join("complete");
        let mut bytes = bytes;
        if fs::metadata(&staged)?.len() <= 4 * 1024 * 1024 {
            if let Some(resource) =
                crate::native::metalink::parse(&fs::read(&staged)?, &task.file.url)?
            {
                let mut result = None;
                let mut error = None;
                for url in resource.urls {
                    if url == task.file.url {
                        continue;
                    }
                    let mut redirected = task.clone();
                    redirected.file.url = url;
                    redirected.file.hash = task.file.hash.clone().or(resource.hash.clone());
                    redirected.headers = HeaderMap::new();
                    let nested = self.clone().cache_directory(state_dir.join("metalink"));
                    match Box::pin(nested.perform_depth(redirected, report.clone(), depth + 1))
                        .await
                    {
                        Ok(file) => {
                            result = Some(file);
                            break;
                        }
                        Err(e) => error = Some(e),
                    }
                }
                let file = result.ok_or_else(|| {
                    error.unwrap_or_else(|| anyhow!("Metalink has no usable mirror"))
                })?;
                fs::copy(&file.path, &staged)?;
                bytes = file.bytes;
            }
        }
        let check_path = staged.clone();
        let check_hash = hash.clone();
        if !tokio::task::spawn_blocking(move || verify(&check_path, check_hash.as_ref())).await?? {
            // Remove only files in this manager-owned per-download directory.
            fs::remove_dir_all(&state_dir)?;
            bail!(
                "Hash mismatch for {}. The file was not published to cache; run download again.",
                task.label
            );
        }
        let destination = path.clone();
        let staged_copy = staged.clone();
        tokio::task::spawn_blocking(move || publish(&staged_copy, &destination)).await??;
        // Cache publication is complete; cleanup failure does not undo the valid artifact.
        if let Err(error) = fs::remove_dir_all(&state_dir) {
            report(Event::Message {
                id: task.id,
                text: format!("Download complete; temporary cleanup failed: {error}"),
            });
        }
        report(Event::Finished {
            id: task.id,
            path: path.clone(),
            bytes,
            cached: false,
        });
        Ok(Downloaded {
            package: task.app,
            path,
            bytes,
            cached: false,
            verified: hash.is_some(),
        })
    }
    async fn segmented(
        &self,
        task: &Task,
        probe: &Probe,
        directory: &Path,
        report: Reporter,
    ) -> Result<u64> {
        let length = probe.length.context("Segmented download has no length")?;
        let count = self.options.threads.min(length as usize).max(1);
        let state = State {
            schema: 1,
            url_fingerprint: fingerprint(&task.file.url),
            final_fingerprint: fingerprint(&probe.final_url),
            hash: task.file.hash.clone(),
            length,
            etag: probe.etag.clone(),
            modified: probe.modified.clone(),
            segments: count,
        };
        let state_path = directory.join("state.json");
        let previous = if state_path.is_file() {
            util::read_json(&state_path)
                .ok()
                .and_then(|v| serde_json::from_value::<State>(v).ok())
        } else {
            None
        };
        let validator = probe
            .etag
            .as_ref()
            .filter(|e| !e.starts_with("W/"))
            .cloned()
            .or_else(|| probe.modified.clone());
        let strong_etag = probe.etag.as_ref().is_some_and(|s| !s.starts_with("W/"));
        let resumable =
            previous.as_ref() == Some(&state) && (strong_etag || task.file.hash.is_some());
        if !resumable {
            for index in 0..count {
                let part = directory.join(format!("part-{index}"));
                if part.try_exists()? {
                    fs::remove_file(part)?;
                }
            }
        }
        util::write_json(&state_path, &state)?;
        let base = length / count as u64;
        let mut jobs = JoinSet::new();
        for index in 0..count {
            let start = base * index as u64;
            let end = if index + 1 == count {
                length - 1
            } else {
                start + base - 1
            };
            let path = directory.join(format!("part-{index}"));
            let mut existing = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if existing > end - start + 1 {
                fs::remove_file(&path)?;
                existing = 0;
            }
            let existing = existing.min(end - start + 1);
            if existing > 0 {
                report(Event::Progress {
                    id: task.id,
                    bytes: existing,
                });
            }
            if existing == end - start + 1 {
                continue;
            }
            let engine = self.clone();
            let task = task.clone();
            let report = report.clone();
            let validator = validator.clone();
            jobs.spawn(async move {
                engine
                    .part(
                        &task,
                        &path,
                        start,
                        end,
                        length,
                        validator.as_deref(),
                        report,
                    )
                    .await
            });
        }
        while let Some(result) = jobs.join_next().await {
            match result {
                Ok(Ok(())) => {}
                other => {
                    jobs.abort_all();
                    while jobs.join_next().await.is_some() {}
                    return match other {
                        Ok(Err(error)) => Err(error),
                        Err(error) => Err(error.into()),
                        _ => unreachable!(),
                    };
                }
            }
        }
        let directory = directory.to_owned();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let staged = directory.join("complete");
            let mut out = fs::File::create(&staged)?;
            for index in 0..count {
                let mut input = fs::File::open(directory.join(format!("part-{index}")))?;
                std::io::copy(&mut input, &mut out)?;
            }
            out.sync_all()?;
            if out.metadata()?.len() != length {
                bail!("Assembled download has an incorrect length");
            }
            Ok(())
        })
        .await??;
        Ok(length)
    }
    async fn part(
        &self,
        task: &Task,
        path: &Path,
        start: u64,
        end: u64,
        total: u64,
        validator: Option<&str>,
        report: Reporter,
    ) -> Result<()> {
        let mut last = None;
        for attempt in 0..=self.options.retries {
            let result = async {
                let existing = tokio::fs::metadata(path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                if existing == end - start + 1 {
                    return Ok(());
                }
                let offset = start + existing;
                if offset > end {
                    bail!("Partial download exceeds its segment");
                }
                let range = format!("bytes={offset}-{end}");
                let mut lease = self
                    .send(&task.file.url, &task.headers, Some(&range), validator)
                    .await?;
                let response = &mut lease.response;
                if response.status() == StatusCode::OK
                    || response.status() == StatusCode::RANGE_NOT_SATISFIABLE
                {
                    return Err(anyhow!(RangeRejected));
                }
                if response.status() != StatusCode::PARTIAL_CONTENT {
                    return Err(http_error(response));
                }
                if content_range(response) != Some((offset, end, total)) {
                    return Err(anyhow!(RangeRejected));
                }
                if let Some(validator) = validator {
                    let actual = if validator.starts_with('"') {
                        header_text(response, ETAG)
                    } else {
                        header_text(response, LAST_MODIFIED)
                    };
                    if actual.as_deref() != Some(validator) {
                        return Err(anyhow!(RangeRejected));
                    }
                }
                check_encoding(response)?;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .await?;
                let mut bytes = existing;
                while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
                    if bytes + chunk.len() as u64 > end - start + 1 {
                        return Err(anyhow!(RangeRejected));
                    }
                    file.write_all(&chunk).await?;
                    bytes += chunk.len() as u64;
                    report(Event::Progress {
                        id: task.id,
                        bytes: chunk.len() as u64,
                    });
                }
                file.flush().await?;
                file.sync_all().await?;
                if bytes != end - start + 1 {
                    bail!("Segment ended before all bytes arrived");
                }
                Ok(())
            }
            .await;
            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    if attempt < self.options.retries {
                        self.pause(&error, attempt).await?;
                        report(Event::Message {
                            id: task.id,
                            text: format!(
                                "Retrying interrupted segment ({}/{})",
                                attempt + 1,
                                self.options.retries
                            ),
                        });
                    }
                    last = Some(error);
                }
            }
        }
        Err(last.context("Segment download failed")?)
    }
    async fn single(&self, task: &Task, directory: &Path, report: Reporter) -> Result<u64> {
        let mut last = None;
        for attempt in 0..=self.options.retries {
            let result = async {
                let mut lease = self.send(&task.file.url, &task.headers, None, None).await?;
                let response = &mut lease.response;
                if response.status() != StatusCode::OK {
                    return Err(http_error(response));
                }
                check_encoding(response)?;
                let length = response.content_length();
                report(Event::Started {
                    id: task.id,
                    label: task.label.clone(),
                    total: length,
                });
                let mut file = tokio::fs::File::create(directory.join("complete")).await?;
                let mut bytes = 0;
                while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
                    file.write_all(&chunk).await?;
                    bytes += chunk.len() as u64;
                    report(Event::Progress {
                        id: task.id,
                        bytes: chunk.len() as u64,
                    });
                }
                file.flush().await?;
                file.sync_all().await?;
                if length.is_some_and(|n| n != bytes) {
                    bail!("Download has an incorrect length");
                }
                Ok(bytes)
            }
            .await;
            match result {
                Ok(bytes) => return Ok(bytes),
                Err(error) => {
                    if attempt < self.options.retries {
                        self.pause(&error, attempt).await?;
                    }
                    last = Some(error);
                }
            }
        }
        Err(last.context("Download failed")?)
    }
}
fn fingerprint(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn cache_key(path: &Path) -> Result<String> {
    Ok(fingerprint(
        path.file_name()
            .context("Cache path has no filename")?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str(),
    ))
}
pub fn cache_path(cache: &Path, app: &str, version: &str, url: &str) -> Result<PathBuf> {
    util::valid_name(app)?;
    util::valid_component(version)?;
    let legacy = regex::Regex::new(r"[^\w.\-]+")?
        .replace_all(url, "_")
        .into_owned();
    let legacy = cache.join(format!("{app}#{version}#{legacy}"));
    // Scoop 0.6 keeps an existing legacy URL-derived filename before using its URL hash.
    if legacy
        .file_name()
        .is_some_and(|s| s.to_string_lossy().encode_utf16().count() <= 255)
        && legacy.is_file()
    {
        return Ok(legacy);
    }
    let raw_extension = Path::new(url)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let parsed = url::Url::parse(url)?;
    let fallback = parsed.fragment().unwrap_or(parsed.path());
    let extension = if util::valid_component(raw_extension).is_ok() {
        raw_extension
    } else {
        Path::new(fallback)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
    };
    let extension = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    util::valid_component(&format!("x{extension}"))?;
    Ok(cache.join(format!(
        "{app}#{version}#{}{extension}",
        &fingerprint(url)[..7]
    )))
}
pub fn headers(manifest: &Manifest, arch: Architecture) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(value) = manifest.field("headers", arch) {
        let object = value
            .as_object()
            .context("Manifest headers must be an object")?;
        for (name, value) in object {
            let name =
                HeaderName::from_bytes(name.as_bytes()).context("Invalid manifest header name")?;
            if [
                RANGE,
                IF_RANGE,
                ACCEPT_ENCODING,
                reqwest::header::HOST,
                reqwest::header::CONTENT_LENGTH,
            ]
            .contains(&name)
            {
                bail!("Manifest cannot override range or transport headers");
            }
            let mut value = HeaderValue::from_str(
                value
                    .as_str()
                    .context("Manifest header values must be strings")?,
            )
            .context("Invalid manifest header value")?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
    }
    if let Some(value) = manifest.field("cookie", arch) {
        let object = value
            .as_object()
            .context("Manifest cookie must be an object")?;
        let cookie = object
            .iter()
            .map(|(k, v)| {
                format!(
                    "{k}={}",
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let mut value = HeaderValue::from_str(&cookie).context("Invalid manifest cookie")?;
        value.set_sensitive(true);
        headers.insert(reqwest::header::COOKIE, value);
    }
    Ok(headers)
}
fn header_text(response: &Response, key: HeaderName) -> Option<String> {
    response
        .headers()
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}
fn content_range(response: &Response) -> Option<(u64, u64, u64)> {
    let value = response
        .headers()
        .get(CONTENT_RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes ")?;
    let (interval, total) = value.split_once('/')?;
    let (start, end) = interval.split_once('-')?;
    let result = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    if result.0 > result.1 || result.1 >= result.2 {
        None
    } else {
        Some(result)
    }
}
fn check_encoding(response: &Response) -> Result<()> {
    if response
        .headers()
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
    {
        bail!("Server ignored identity encoding; cannot safely save byte ranges");
    }
    Ok(())
}
fn http_error(response: &Response) -> anyhow::Error {
    HttpFailure {
        status: response.status(),
        retry_after: response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok()),
    }
    .into()
}
#[derive(Clone)]
struct Hash {
    algorithm: Algorithm,
    expected: String,
}
#[derive(Clone)]
enum Algorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}
impl Hash {
    fn parse(text: Option<&str>) -> Result<Option<Self>> {
        let Some(text) = text else { return Ok(None) };
        let (name, digest) = text
            .split_once(':')
            .map(|(n, d)| (Some(n.to_lowercase()), d))
            .unwrap_or((None, text));
        let algorithm = match (name.as_deref(), digest.len()) {
            (Some("md5") | None, 32) => Algorithm::Md5,
            (Some("sha1") | None, 40) => Algorithm::Sha1,
            (Some("sha256") | None, 64) => Algorithm::Sha256,
            (Some("sha512") | None, 128) => Algorithm::Sha512,
            _ => bail!("Unsupported or malformed manifest hash"),
        };
        if !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("Manifest hash is not hexadecimal");
        }
        Ok(Some(Self {
            algorithm,
            expected: digest.to_ascii_lowercase(),
        }))
    }
}
fn verify(path: &Path, hash: Option<&Hash>) -> Result<bool> {
    let Some(hash) = hash else {
        return Ok(path.is_file());
    };
    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; 128 * 1024];
    macro_rules! digest {
        ($kind:ty) => {{
            let mut digest = <$kind>::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                digest.update(&buf[..n]);
            }
            format!("{:x}", digest.finalize())
        }};
    }
    let computed = match hash.algorithm {
        Algorithm::Md5 => digest!(md5::Md5),
        Algorithm::Sha1 => digest!(sha1::Sha1),
        Algorithm::Sha256 => digest!(sha2::Sha256),
        Algorithm::Sha512 => digest!(sha2::Sha512),
    };
    Ok(computed == hash.expected)
}
fn publish(staged: &Path, destination: &Path) -> Result<()> {
    // Both paths live under the cache directory, so publication can move the
    // completed file without allocating another full-size temporary copy.
    fs::rename(staged, destination).context("Cannot publish verified download to cache")?;
    Ok(())
}

fn scoop_proxy(input: &str) -> Result<reqwest::Proxy> {
    if input.contains("://") {
        return reqwest::Proxy::all(input).map_err(|_| anyhow!("Invalid proxy configuration"));
    }
    let separator = input
        .char_indices()
        .find(|(i, c)| *c == '@' && (*i == 0 || input.as_bytes()[i - 1] != b'\\'))
        .map(|(i, _)| i);
    let (credentials, address) = separator
        .map(|i| (Some(&input[..i]), &input[i + 1..]))
        .unwrap_or((None, input));
    if address == "default" {
        bail!(
            "Explicit credentials with the system proxy are not implemented; specify the proxy address"
        );
    }
    let mut proxy = reqwest::Proxy::all(format!("http://{address}"))
        .map_err(|_| anyhow!("Invalid proxy configuration"))?;
    if let Some(credentials) = credentials {
        let split = credentials
            .char_indices()
            .find(|(i, c)| *c == ':' && (*i == 0 || credentials.as_bytes()[i - 1] != b'\\'))
            .map(|(i, _)| i)
            .context("Proxy credentials must be username:password")?;
        let unescape = |s: &str| s.replace(r"\@", "@").replace(r"\:", ":");
        proxy = proxy.basic_auth(
            &unescape(&credentials[..split]),
            &unescape(&credentials[split + 1..]),
        );
    }
    Ok(proxy)
}

fn windows_proxy(value: &str) -> bool {
    let value = value.to_lowercase();
    value.starts_with("currentuser@") || value.ends_with("@default")
}
