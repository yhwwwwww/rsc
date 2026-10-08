//! Windows integration implemented directly through the operating system APIs.
use crate::{config::Config, util};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Networking::WinHttp::*,
    Security::*,
    System::{Diagnostics::ToolHelp::*, Environment::*, Registry::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};
pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn check(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}
pub fn admin() -> bool {
    unsafe {
        let mut authority = SID_IDENTIFIER_AUTHORITY {
            Value: [0, 0, 0, 0, 0, 5],
        };
        let mut sid = null_mut();
        let mut member = 0;
        if AllocateAndInitializeSid(&mut authority, 2, 32, 544, 0, 0, 0, 0, 0, 0, &mut sid) == 0 {
            return false;
        }
        let ok = CheckTokenMembership(null_mut(), sid, &mut member) != 0;
        FreeSid(sid);
        ok && member != 0
    }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn key(global: bool, write: bool) -> Result<Key> {
    unsafe {
        let scope = if global {
            HKEY_LOCAL_MACHINE
        } else {
            HKEY_CURRENT_USER
        };
        let path = wide(if global {
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"
        } else {
            "Environment"
        });
        let mut h = null_mut();
        let code = if write {
            RegCreateKeyExW(
                scope,
                path.as_ptr(),
                0,
                null(),
                0,
                KEY_READ | KEY_WRITE,
                null(),
                &mut h,
                null_mut(),
            )
        } else {
            RegOpenKeyExW(scope, path.as_ptr(), 0, KEY_READ, &mut h)
        };
        if code != 0 {
            bail!("Cannot open environment registry (Windows error {code})")
        }
        Ok(Key(h))
    }
}
pub fn env_get(name: &str, global: bool) -> Result<Option<(String, u32)>> {
    unsafe {
        let k = key(global, false)?;
        let n = wide(name);
        let mut size = 0;
        let mut kind = 0;
        let code = RegQueryValueExW(k.0, n.as_ptr(), null(), &mut kind, null_mut(), &mut size);
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != 0 {
            bail!("Cannot read {name} (Windows error {code})")
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            bail!("{name} is not a string environment value")
        }
        let mut data = vec![0u16; size as usize / 2 + 1];
        let code = RegQueryValueExW(
            k.0,
            n.as_ptr(),
            null(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut size,
        );
        if code != 0 {
            bail!("Cannot read {name} (Windows error {code})")
        }
        while data.last() == Some(&0) {
            data.pop();
        }
        Ok(Some((String::from_utf16(&data)?, kind)))
    }
}
pub fn env_set(name: &str, value: Option<&str>, global: bool) -> Result<()> {
    unsafe {
        let k = key(global, true)?;
        let n = wide(name);
        let code = if let Some(v) = value {
            let v = wide(v);
            RegSetValueExW(
                k.0,
                n.as_ptr(),
                0,
                REG_EXPAND_SZ,
                v.as_ptr().cast(),
                (v.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(k.0, n.as_ptr())
        };
        if code != 0 && code != ERROR_FILE_NOT_FOUND {
            bail!("Cannot write {name} (Windows error {code})")
        }
        let v = value.map(wide);
        SetEnvironmentVariableW(n.as_ptr(), v.as_ref().map_or(null(), |v| v.as_ptr()));
        broadcast();
        Ok(())
    }
}
pub fn broadcast() {
    unsafe {
        let mut result = 0;
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            wide("Environment").as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            &mut result,
        );
    }
}
fn same(a: &str, b: &str) -> bool {
    a.trim()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.trim().trim_end_matches(['\\', '/']))
}
pub fn path_edit(name: &str, path: &Path, add: bool, global: bool) -> Result<()> {
    let process = std::env::var(name).unwrap_or_default();
    let p = path.to_string_lossy();
    let old = env_get(name, global)?.map(|v| v.0).unwrap_or_default();
    let mut parts = old
        .split(';')
        .filter(|v| !v.is_empty() && !same(v, &p))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if add {
        parts.insert(0, p.to_string())
    }
    let joined = parts.join(";");
    env_set(
        name,
        if joined.is_empty() {
            None
        } else {
            Some(&joined)
        },
        global,
    )?;
    // Children inherit the complete process PATH, including host tools.
    let mut combined = process
        .split(';')
        .filter(|v| !v.is_empty() && !same(v, &p))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if add {
        combined.insert(0, p.to_string());
    }
    unsafe {
        SetEnvironmentVariableW(wide(name).as_ptr(), wide(combined.join(";")).as_ptr());
    }
    Ok(())
}
pub fn isolated_path(c: &Config, value: &str) -> Result<()> {
    let name = if value.eq_ignore_ascii_case("true") {
        "SCOOP_PATH"
    } else if value.is_empty() || value.eq_ignore_ascii_case("false") {
        "PATH"
    } else {
        value
    };
    for global in [false, true] {
        if global && !admin() {
            continue;
        }
        let old = c.layout.path_variable.as_str();
        if old != name {
            if let Some((s, _)) = env_get(old, global)? {
                for p in s
                    .split(';')
                    .filter(|p| Path::new(p).starts_with(c.layout.base(global)))
                {
                    path_edit(old, Path::new(p), false, global)?;
                    path_edit(name, Path::new(p), true, global)?;
                }
            }
        }
        path_edit(name, &c.layout.shims(global), true, global)?;
    }
    if name != "PATH" {
        let old = env_get("PATH", false)?.map(|s| s.0).unwrap_or_default();
        let marker = format!("%{name}%");
        if !old.split(';').any(|p| p.eq_ignore_ascii_case(&marker)) {
            env_set("PATH", Some(&format!("{old};{marker}")), false)?;
        }
    }
    Ok(())
}
pub fn open_url(url: &str) -> Result<()> {
    unsafe {
        let code = ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        ) as isize;
        if code <= 32 {
            bail!("Cannot open homepage (Windows error {code})")
        }
        Ok(())
    }
}
pub fn remove(path: &Path) -> Result<()> {
    (|| -> Result<()> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            if metadata.file_attributes() & 1 != 0 {
                super::attributes::writable_link(path)?;
            }
            if metadata.file_attributes() & 0x10 != 0 {
                fs::remove_dir(path)?
            } else {
                fs::remove_file(path)?
            }
            return Ok(());
        }
        if metadata.is_dir() {
            for e in fs::read_dir(path)? {
                remove(&e?.path())?;
            }
            fs::remove_dir(path)?
        } else {
            if metadata.permissions().readonly() {
                let mut p = metadata.permissions();
                p.set_readonly(false);
                fs::set_permissions(path, p)?;
            }
            fs::remove_file(path)?
        }
        Ok(())
    })()
    .with_context(|| format!("Cannot remove {}", path.display()))
}
pub fn is_link(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & 0x400 != 0)
}
pub fn junction(link: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?
    }
    fs::create_dir(link)?;
    unsafe extern "system" {
        fn CreateFileW(
            p: *const u16,
            access: u32,
            share: u32,
            security: *const std::ffi::c_void,
            creation: u32,
            flags: u32,
            template: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn DeviceIoControl(
            h: *mut std::ffi::c_void,
            code: u32,
            input: *const u8,
            size: u32,
            output: *mut u8,
            outsize: u32,
            returned: *mut u32,
            overlapped: *mut std::ffi::c_void,
        ) -> i32;
    }
    let target = util::canonical_path(target)?;
    let printable = target.to_string_lossy();
    let subst = if let Some(unc) = printable.strip_prefix(r"\\") {
        format!(r"\??\UNC\{unc}")
    } else {
        format!(r"\??\{printable}")
    };
    let a = wide(&subst);
    let b = wide(printable.as_ref());
    let mut data = Vec::new();
    data.extend(0xa0000003u32.to_le_bytes());
    data.extend(((8 + (a.len() + b.len()) * 2) as u16).to_le_bytes());
    data.extend(0u16.to_le_bytes());
    for v in [
        0u16,
        ((a.len() - 1) * 2) as u16,
        (a.len() * 2) as u16,
        ((b.len() - 1) * 2) as u16,
    ] {
        data.extend(v.to_le_bytes())
    }
    for v in a.into_iter().chain(b) {
        data.extend(v.to_le_bytes())
    }
    let result = unsafe {
        let h = CreateFileW(
            wide(link).as_ptr(),
            0x40000000,
            7,
            null(),
            3,
            0x02200000,
            null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            Err(std::io::Error::last_os_error().into())
        } else {
            let mut returned = 0;
            let ok = DeviceIoControl(
                h,
                0x900a4,
                data.as_ptr(),
                data.len() as u32,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            );
            let result = check(ok != 0);
            CloseHandle(h);
            result
        }
    };
    if result.is_err() {
        let _ = fs::remove_dir(link);
    }
    result
}
pub fn running(app: &Path) -> Result<Vec<String>> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Vec::new();
        let prefix = app.to_string_lossy().to_lowercase() + r"\";
        let mut next = Process32FirstW(snapshot, &mut entry);
        while next != 0 {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
            if !process.is_null() {
                let mut text = vec![0u16; 32768];
                let mut count = text.len() as u32;
                if QueryFullProcessImageNameW(process, 0, text.as_mut_ptr(), &mut count) != 0 {
                    let image = String::from_utf16_lossy(&text[..count as usize]);
                    if image.to_lowercase().starts_with(&prefix) {
                        found.push(format!("{image} (PID {})", entry.th32ProcessID))
                    }
                }
                CloseHandle(process);
            }
            next = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        Ok(found)
    }
}
pub fn shortcut(
    path: &Path,
    target: &Path,
    args: &str,
    working: &Path,
    icon: Option<(&Path, i32)>,
) -> Result<()> {
    use windows_sys::Win32::System::Com::*;
    use windows_sys::core::GUID;
    const CLASS: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    const LINK: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
    const FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);
    unsafe fn method(object: *mut std::ffi::c_void, index: usize) -> *mut std::ffi::c_void {
        unsafe { *(*(object as *mut *mut *mut std::ffi::c_void)).add(index) }
    }
    unsafe fn release(object: *mut std::ffi::c_void) {
        let f: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
            unsafe { std::mem::transmute(method(object, 2)) };
        unsafe {
            f(object);
        }
    }
    fn hr(code: i32) -> Result<()> {
        if code < 0 {
            bail!("Shortcut COM error 0x{:08x}", code as u32)
        }
        Ok(())
    }
    fs::create_dir_all(path.parent().context("Shortcut has no parent")?)?;
    unsafe {
        let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let mut object = null_mut();
        let result = (|| -> Result<()> {
            hr(CoCreateInstance(
                &CLASS,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &LINK,
                &mut object,
            ))?;
            for (index, value) in [
                (20, target.to_string_lossy().to_string()),
                (11, args.to_owned()),
                (9, working.to_string_lossy().to_string()),
            ] {
                let f: unsafe extern "system" fn(*mut std::ffi::c_void, *const u16) -> i32 =
                    std::mem::transmute(method(object, index));
                hr(f(object, wide(value).as_ptr()))?;
            }
            if let Some((icon, index)) = icon {
                let f: unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, i32) -> i32 =
                    std::mem::transmute(method(object, 17));
                hr(f(object, wide(icon).as_ptr(), index))?;
            }
            let query: unsafe extern "system" fn(
                *mut std::ffi::c_void,
                *const GUID,
                *mut *mut std::ffi::c_void,
            ) -> i32 = std::mem::transmute(method(object, 0));
            let mut file = null_mut();
            hr(query(object, &FILE, &mut file))?;
            let save: unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, i32) -> i32 =
                std::mem::transmute(method(file, 6));
            let result = hr(save(file, wide(path).as_ptr(), 1));
            release(file);
            result
        })();
        if !object.is_null() {
            release(object)
        }
        if initialized {
            CoUninitialize()
        }
        result
    }
}
struct Http(*mut std::ffi::c_void);
impl Drop for Http {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn handle(p: *mut std::ffi::c_void) -> Result<Http> {
    check(!p.is_null())?;
    Ok(Http(p))
}
pub fn download(c: &Config, d: &Value) -> Result<()> {
    unsafe {
        let u = url::Url::parse(d["url"].as_str().context("Download URL missing")?)?;
        let proxy = c.text("proxy")?.unwrap_or_default();
        let credentials = proxy.rsplit_once('@').map(|(auth, _)| auth.to_owned());
        let proxy = proxy.rsplit_once('@').map_or(proxy.as_str(), |(_, p)| p);
        let mode = if proxy == "none" {
            WINHTTP_ACCESS_TYPE_NO_PROXY
        } else if proxy.is_empty() || proxy == "default" {
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
        } else {
            WINHTTP_ACCESS_TYPE_NAMED_PROXY
        };
        let proxy_w = wide(proxy);
        let session = handle(WinHttpOpen(
            wide("rsc/0.1").as_ptr(),
            mode,
            if mode == WINHTTP_ACCESS_TYPE_NAMED_PROXY {
                proxy_w.as_ptr()
            } else {
                null()
            },
            null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 30000, 30000, 30000, 30000);
        let connect = handle(WinHttpConnect(
            session.0,
            wide(u.host_str().context("Missing host")?).as_ptr(),
            u.port_or_known_default().unwrap_or(443),
            0,
        ))?;
        let resource = format!(
            "{}{}",
            u.path(),
            u.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let request = handle(WinHttpOpenRequest(
            connect.0,
            wide("GET").as_ptr(),
            wide(resource).as_ptr(),
            null(),
            null(),
            null(),
            if u.scheme() == "https" {
                WINHTTP_FLAG_SECURE
            } else {
                0
            },
        ))?;
        let policy = WINHTTP_AUTOLOGON_SECURITY_LEVEL_LOW;
        check(
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_AUTOLOGON_POLICY,
                (&policy as *const u32).cast(),
                4,
            ) != 0,
        )?;
        let mut headers = super::query::headers(c, u.as_str())?;
        if let Some(cookie) = d["cookie"].as_str() {
            headers["Cookie"] = cookie.into();
        }
        let text = headers
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v.as_str().map(|v| format!("{k}: {v}\r\n")))
            .collect::<String>();
        let hw = wide(&text);
        let mut response = 0u32;
        for attempt in 0..3 {
            check(
                WinHttpSendRequest(
                    request.0,
                    hw.as_ptr(),
                    text.encode_utf16().count() as u32,
                    null(),
                    0,
                    0,
                    0,
                ) != 0,
            )?;
            check(WinHttpReceiveResponse(request.0, null_mut()) != 0)?;
            let mut size = 4;
            check(
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    null(),
                    (&mut response as *mut u32).cast(),
                    &mut size,
                    null_mut(),
                ) != 0,
            )?;
            if response != 407 || attempt == 2 {
                break;
            }
            let mut schemes = 0;
            let mut first = 0;
            let mut target = 0;
            check(WinHttpQueryAuthSchemes(request.0, &mut schemes, &mut first, &mut target) != 0)?;
            let scheme = if schemes & WINHTTP_AUTH_SCHEME_NEGOTIATE != 0 {
                WINHTTP_AUTH_SCHEME_NEGOTIATE
            } else {
                WINHTTP_AUTH_SCHEME_NTLM
            };
            let login = credentials
                .as_ref()
                .filter(|s| !s.eq_ignore_ascii_case("currentuser"))
                .map(|s| -> Result<_> {
                    let i = s
                        .char_indices()
                        .find(|(i, c)| *c == ':' && (*i == 0 || s.as_bytes()[i - 1] != b'\\'))
                        .map(|(i, _)| i)
                        .context("Proxy credentials require username:password")?;
                    let unescape = |s: &str| s.replace(r"\@", "@").replace(r"\:", ":");
                    Ok((wide(unescape(&s[..i])), wide(unescape(&s[i + 1..]))))
                })
                .transpose()?;
            let scheme = if login.is_some() && schemes & WINHTTP_AUTH_SCHEME_BASIC != 0 {
                WINHTTP_AUTH_SCHEME_BASIC
            } else {
                scheme
            };
            check(
                WinHttpSetCredentials(
                    request.0,
                    WINHTTP_AUTH_TARGET_PROXY,
                    scheme,
                    login.as_ref().map_or(null(), |v| v.0.as_ptr()),
                    login.as_ref().map_or(null(), |v| v.1.as_ptr()),
                    null_mut(),
                ) != 0,
            )?;
        }
        if !(200..300).contains(&response) {
            bail!("HTTP {response} through Windows proxy")
        }
        let path = Path::new(d["to"].as_str().context("Download destination missing")?);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?
        }
        let mut file = fs::File::create(path)?;
        let mut buffer = vec![0u8; 65536];
        loop {
            let mut count = 0;
            check(
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut count,
                ) != 0,
            )?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count as usize])?;
        }
        file.sync_all()?;
        Ok(())
    }
}
