//! Independent Rust tests; no upstream implementation is included.
use rsc_core::{
    manifest::{Architecture, Manifest},
    native::{lifecycle, query, scripts, windows},
};
use serde_json::json;
#[test]
fn architecture_fallback() {
    let m=Manifest::parse(json!({"version":"1","architecture":{"64bit":{"url":"https://example.invalid/x.zip"},"32bit":{"url":"https://example.invalid/y.zip"}}}).to_string()).unwrap();
    assert_eq!(
        query::architecture(&m, Architecture::Arm64).unwrap(),
        Architecture::X64
    );
}
#[test]
fn architecture_rejection() {
    let m = Manifest::parse(
        json!({"version":"1","architecture":{"64bit":{"url":"https://example.invalid/x.zip"}}})
            .to_string(),
    )
    .unwrap();
    assert!(query::architecture(&m, Architecture::X86).is_err());
}
#[test]
fn fragment_filename() {
    assert_eq!(
        query::filename("https://example.invalid/download?id=1#/tool.zip").unwrap(),
        "tool.zip"
    );
}
#[test]
fn unicode_filename() {
    assert_eq!(
        query::filename("https://example.invalid/%E4%B8%AD%E6%96%87.zip").unwrap(),
        "中文.zip"
    );
}
#[test]
fn reject_path_in_filename() {
    assert!(query::filename("https://example.invalid/x#/../escape.zip").is_err());
}
#[test]
fn numeric_versions() {
    assert!(query::compare("1.10.0", "1.9.9").is_gt());
    assert!(query::compare("1.0", "1.0.0").is_eq());
}
#[test]
fn prerelease_versions() {
    assert!(query::compare("1.0", "1.0-rc1").is_gt());
    assert!(query::compare("1.0-rc2", "1.0-rc1").is_gt());
}
#[test]
fn date_versions() {
    assert!(query::compare("nightly-20261009", "nightly-20261008").is_gt());
}
#[test]
fn case_insensitive_regex() {
    let re = query::matcher("^FOO(?=bar)").unwrap();
    assert!(query::matches(&re, "foobar").unwrap());
    assert!(!query::matches(&re, "barfoo").unwrap());
}
#[test]
fn invalid_regex() {
    assert!(query::matcher("(").is_err());
}
#[test]
fn relative_paths() {
    let dir = std::env::temp_dir();
    assert!(lifecycle::relative(&dir, "../outside").is_err());
    assert!(lifecycle::relative(&dir, r"C:\outside").is_err());
    assert_eq!(
        lifecycle::relative(&dir, "data/file").unwrap(),
        dir.join("data/file")
    );
}
#[test]
fn script_context_expansion() {
    let d = json!({"dir":r"C:\apps\a\current","version":"1","persist_dir":r"C:\persist\a"});
    assert_eq!(
        scripts::expand("$dir; $version; $persist_dir", &d),
        r"C:\apps\a\current; 1; C:\persist\a"
    );
}
#[test]
fn unicode_junction_removal_preserves_target() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("中文 package");
    let target = temp.path().join("persist");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("settings"), "keep").unwrap();
    windows::junction(&root.join("data"), &target).unwrap();
    assert!(windows::is_link(&root.join("data")));
    let mut perms = std::fs::symlink_metadata(root.join("data"))
        .unwrap()
        .permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(root.join("data"), perms).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("data/settings")).unwrap(),
        "keep"
    );
    windows::remove(&root).unwrap();
    assert_eq!(
        std::fs::read_to_string(target.join("settings")).unwrap(),
        "keep"
    );
}
#[test]
fn utf16_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path().join("manifest.json");
    let text = r#"{"version":"1","description":"中文"}"#;
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&p, bytes).unwrap();
    assert_eq!(Manifest::read(&p).unwrap().description(), "中文");
}
