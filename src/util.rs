use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde_json::{Map, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

/// Bounded, ordered parallel work. No cache, stale data, or unbounded thread creation.
pub(crate) fn parallel_map<T: Sync, U: Send>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U> {
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .min(16)
        .min(items.len());
    if workers < 2 {
        return items.iter().map(f).collect();
    }
    std::thread::scope(|scope| {
        let f = &f;
        let handles = items
            .chunks(items.len().div_ceil(workers))
            .map(|chunk| scope.spawn(move || chunk.iter().map(f).collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("query worker panicked"))
            .collect()
    })
}
pub fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?;
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        let bytes = &bytes[2..];
        if bytes.len() % 2 != 0 {
            bail!("Invalid UTF-16 file: {}", path.display());
        }
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        return String::from_utf16(&words)
            .with_context(|| format!("Invalid UTF-16: {}", path.display()));
    }
    let bytes = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes[3..].to_vec()
    } else {
        bytes
    };
    String::from_utf8(bytes).with_context(|| format!("Invalid UTF-8: {}", path.display()))
}

pub fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_str(&read_text(path)?)
        .with_context(|| format!("Invalid JSON: {}", path.display()))
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().context("File has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Cannot create {}", parent.display()))?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("Cannot replace {}", path.display()))?;
    Ok(())
}

pub fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

pub struct FileLock {
    _file: File,
}
impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("Cannot open lock {}", path.display()))?;
        FileExt::try_lock_exclusive(&file).with_context(|| {
            format!(
                "Another operation is using {}. Retry when it finishes.",
                path.display()
            )
        })?;
        Ok(Self { _file: file })
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._file);
    }
}

pub fn ci_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}
pub fn object_get<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}
pub fn valid_component(text: &str) -> Result<()> {
    if text.is_empty()
        || text == "."
        || text == ".."
        || text.ends_with(['.', ' '])
        || text
            .chars()
            .any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
    {
        bail!("Invalid package name or version: {text}");
    }
    Ok(())
}
pub fn valid_name(text: &str) -> Result<()> {
    valid_component(text)?;
    if !text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        || !text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        bail!("Invalid package or bucket name: {text}");
    }
    Ok(())
}
pub fn absolute(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
pub fn redact_url(text: &str) -> String {
    if let Ok(mut url) = url::Url::parse(text) {
        if !url.username().is_empty() || url.password().is_some() {
            let _ = url.set_username("***");
            let _ = url.set_password(Some("***"));
        }
        if url.query().is_some() {
            url.set_query(Some("redacted"));
        }
        return url.to_string();
    }
    text.to_owned()
}

pub fn canonical_path(path: &Path) -> Result<PathBuf> {
    let path = fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let text = path
            .to_str()
            .context("Windows integration requires a Unicode path")?;
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(local));
        }
    }
    Ok(path)
}

pub fn remove_tree(root: &Path) -> Result<()> {
    crate::native::windows::remove(root)
}

/// File identity, including hard links; canonical paths alone cannot establish it.
#[cfg(windows)]
pub fn same_file(a: &Path, b: &Path) -> Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let a = File::open(a)?;
    let b = File::open(b)?;
    let mut left: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let mut right: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(a.as_raw_handle() as _, &mut left) } == 0
        || unsafe { GetFileInformationByHandle(b.as_raw_handle() as _, &mut right) } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((
        left.dwVolumeSerialNumber,
        left.nFileIndexHigh,
        left.nFileIndexLow,
    ) == (
        right.dwVolumeSerialNumber,
        right.nFileIndexHigh,
        right.nFileIndexLow,
    ))
}
#[cfg(not(windows))]
pub fn same_file(a: &Path, b: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let left = fs::metadata(a)?;
    let right = fs::metadata(b)?;
    Ok((left.dev(), left.ino()) == (right.dev(), right.ino()))
}
