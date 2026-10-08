//! FTP fallback through the Windows networking API; package bytes stay in Rust.
use crate::download::{Event, Reporter};
use anyhow::{Context, Result};
use std::{fs::File, io::Write, path::Path};
#[cfg(windows)]
pub fn transfer(
    url: &str,
    path: &Path,
    proxy: Option<&str>,
    report: Reporter,
    id: usize,
) -> Result<u64> {
    use std::{ffi::c_void, ptr};
    type Handle = *mut c_void;
    #[link(name = "wininet")]
    unsafe extern "system" {
        fn InternetOpenW(
            agent: *const u16,
            access: u32,
            proxy: *const u16,
            bypass: *const u16,
            flags: u32,
        ) -> Handle;
        fn InternetOpenUrlW(
            session: Handle,
            url: *const u16,
            headers: *const u16,
            length: u32,
            flags: u32,
            context: usize,
        ) -> Handle;
        fn InternetReadFile(
            handle: Handle,
            buffer: *mut c_void,
            length: u32,
            read: *mut u32,
        ) -> i32;
        fn InternetCloseHandle(handle: Handle) -> i32;
        fn InternetSetOptionW(
            handle: Handle,
            option: u32,
            buffer: *const c_void,
            length: u32,
        ) -> i32;
    }
    struct Session(Handle);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                InternetCloseHandle(self.0);
            }
        }
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let user_agent = wide(concat!("rsc/", env!("CARGO_PKG_VERSION")));
    let credentials = proxy.and_then(|p| p.rsplit_once('@')).map(|(auth, _)| auth);
    let proxy = proxy
        .map(|p| p.rsplit_once('@').map_or(p, |(_, server)| server))
        .filter(|p| !p.is_empty() && *p != "default");
    let proxy_text = proxy.filter(|p| *p != "none").map(wide);
    let session = Session(unsafe {
        InternetOpenW(
            user_agent.as_ptr(),
            if proxy == Some("none") {
                1
            } else if proxy_text.is_some() {
                3
            } else {
                0
            },
            proxy_text.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
            ptr::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(std::io::Error::last_os_error()).context("Cannot initialize FTP session");
    }
    if let Some(auth) = credentials.filter(|s| !s.eq_ignore_ascii_case("currentuser")) {
        let split = auth
            .char_indices()
            .find(|(i, c)| *c == ':' && (*i == 0 || auth.as_bytes()[i - 1] != b'\\'))
            .map(|(i, _)| i)
            .context("Proxy credentials require username:password")?;
        for (option, value) in [(43, &auth[..split]), (44, &auth[split + 1..])] {
            let value = wide(&value.replace(r"\@", "@").replace(r"\:", ":"));
            if unsafe {
                InternetSetOptionW(
                    session.0,
                    option,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("Cannot set FTP proxy credentials");
            }
        }
    }
    let timeout = 30000u32;
    for option in [2, 5, 6] {
        unsafe {
            InternetSetOptionW(session.0, option, (&timeout as *const u32).cast(), 4);
        }
    }
    let mut parsed = url::Url::parse(url)?;
    parsed.set_fragment(None);
    let resource = wide(parsed.as_str());
    let stream = Session(unsafe {
        InternetOpenUrlW(
            session.0,
            resource.as_ptr(),
            ptr::null(),
            0,
            0x80000000 | 0x04000000 | 0x08000000 | 0x00000200,
            0,
        )
    });
    if stream.0.is_null() {
        return Err(std::io::Error::last_os_error()).context("Cannot open FTP resource");
    }
    let mut file = File::create(path)?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0;
    loop {
        let mut count = 0u32;
        if unsafe {
            InternetReadFile(
                stream.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut count,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("FTP read failed");
        }
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count as usize])?;
        total += u64::from(count);
        report(Event::Progress {
            id,
            bytes: u64::from(count),
        });
    }
    file.sync_all()?;
    Ok(total)
}
#[cfg(not(windows))]
pub fn transfer(_: &str, _: &Path, _: Option<&str>, _: Reporter, _: usize) -> Result<u64> {
    bail!("FTP downloads require Windows")
}
