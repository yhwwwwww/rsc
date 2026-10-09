use crate::util;
use anyhow::{Context, Result, bail};
use std::{fs, path::PathBuf, process::Command};
pub fn dispatch() -> Result<Option<i32>> {
    let executable = std::env::current_exe()?;
    let shim = executable.with_extension("shim");
    if !shim.is_file() {
        return Ok(None);
    }
    let text = util::read_text(&shim)?;
    let mut target = None;
    let mut fixed = None;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key.trim().to_ascii_lowercase().as_str() {
                "path" => target = Some(PathBuf::from(value.trim().trim_matches('"'))),
                "args" => fixed = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    let target = target.context("Shim is missing its target")?;
    let target = if target.is_absolute() {
        target
    } else {
        executable
            .parent()
            .context("Shim has no directory")?
            .join(target)
    };
    if fs::canonicalize(&target)? == fs::canonicalize(&executable)? {
        bail!("Shim points to itself");
    }
    // The manager's own entry point can share its executable file. File
    // identity prevents an old launcher from bypassing a changed shim target.
    if executable
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("rsc.exe"))
        && fixed.as_deref().is_none_or(|s| s.trim().is_empty())
        && util::same_file(&target, &executable).unwrap_or(false)
    {
        return Ok(None);
    }
    let extension = target
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut command = if extension == "ps1" {
        let mut c = Command::new(crate::native::scripts::powershell());
        c.args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&target);
        c
    } else if extension == "cmd" || extension == "bat" {
        let mut c = Command::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
        c.args(["/d", "/s", "/c"]).arg(&target);
        c
    } else {
        Command::new(&target)
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Some(args) = fixed {
            command.raw_arg(args);
        }
        command.raw_arg(raw_arguments());
    }
    #[cfg(not(windows))]
    {
        if fixed.is_some() {
            bail!("Scoop shims are Windows-only");
        }
        command.args(std::env::args_os().skip(1));
    }
    Ok(Some(
        command
            .status()
            .with_context(|| format!("Cannot launch {}", target.display()))?
            .code()
            .unwrap_or(1),
    ))
}
#[cfg(windows)]
fn raw_arguments() -> std::ffi::OsString {
    use std::os::windows::ffi::OsStringExt;
    unsafe extern "system" {
        fn GetCommandLineW() -> *const u16;
    }
    let ptr = unsafe { GetCommandLineW() };
    let mut length = 0;
    unsafe {
        while *ptr.add(length) != 0 {
            length += 1;
        }
    }
    let line = unsafe { std::slice::from_raw_parts(ptr, length) };
    let mut index = 0;
    let mut quoted = false;
    while index < length {
        match line[index] {
            34 => quoted = !quoted,
            9 | 32 if !quoted => break,
            _ => {}
        }
        index += 1;
    }
    while index < length && matches!(line[index], 9 | 32) {
        index += 1;
    }
    std::ffi::OsString::from_wide(&line[index..])
}
