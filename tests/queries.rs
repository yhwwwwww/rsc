use rsc_core::{bucket, config::Config, manager, native::query, package::Installed};
use serde_json::json;
use std::{fs, path::Path};

fn put(root: &Path, relative: &str, value: serde_json::Value) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value.to_string()).unwrap();
}
fn config(root: &Path) -> Config {
    let mut c = Config::load().unwrap();
    c.layout.root = root.into();
    c.layout.global_root = root.join("global");
    c.layout.cache = root.join("cache");
    c.layout.no_junction = false;
    c
}
#[test]
fn parallel_search_matches_scoop_names_top_level_bins_and_order() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    for i in 0..100 {
        put(
            root,
            &format!("main/bucket/tool{i:03}.json"),
            json!({"version":"1","description":"中文"}),
        );
    }
    put(
        root,
        "main/bucket/nested/alias.json",
        json!({"version":"2","bin":[["folder/app.exe","aliased"]],
               "architecture":{"arm64":{"bin":"arm-only.exe"}}}),
    );
    put(root, "extras/bucket/TOOL.json", json!({"Version":"3"}));
    let all = bucket::index(root).unwrap();
    let empty = bucket::search(root, "").unwrap();
    assert_eq!(all.packages.len(), 102);
    assert_eq!(
        all.packages.iter().map(|p| &p.source).collect::<Vec<_>>(),
        empty.packages.iter().map(|p| &p.source).collect::<Vec<_>>()
    );
    assert_eq!(bucket::search(root, "^tool").unwrap().packages.len(), 101);
    let alias = bucket::search(root, "aliased").unwrap();
    assert_eq!(alias.packages[0].binaries, ["aliased"]);
    let executable = bucket::search(root, "app").unwrap();
    assert_eq!(executable.packages[0].binaries, ["app.exe"]);
    assert!(
        bucket::search(root, "arm-only")
            .unwrap()
            .packages
            .is_empty()
    );
    // Scoop's raw-content prefilter applies before binary matching.
    assert!(
        bucket::search(root, "^aliased$")
            .unwrap()
            .packages
            .is_empty()
    );
    let named = bucket::search(root, "(?i)^ALIAS$").unwrap();
    assert_eq!(named.packages.len(), 1);
    assert!(named.packages[0].binaries.is_empty());
    assert!(bucket::search(root, "(").is_err());
}
#[test]
fn search_keeps_diagnostics_and_observes_edits_and_deletions() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "main/bucket/a.json", json!({"version":"1"}));
    put(t.path(), "main/bucket/broken.json", json!(false));
    let first = bucket::search(t.path(), "a|broken").unwrap();
    assert_eq!(first.warnings.len(), 1);
    assert_eq!(first.packages[0].version, "1");
    put(t.path(), "main/bucket/a.json", json!({"version":"2"}));
    assert_eq!(
        bucket::search(t.path(), "a").unwrap().packages[0].version,
        "2"
    );
    fs::remove_file(t.path().join("main/bucket/a.json")).unwrap();
    assert!(bucket::search(t.path(), "a").unwrap().packages.is_empty());
}
#[test]
fn resolution_preserves_bucket_priority_nested_files_and_case() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "extras/bucket/foo.json", json!({"version":"2"}));
    put(t.path(), "main/bucket/sub/Foo.json", json!({"version":"1"}));
    assert_eq!(
        bucket::resolve(t.path(), "FOO").unwrap().bucket.as_deref(),
        Some("main")
    );
    assert_eq!(
        bucket::resolve(t.path(), "EXTRAS/foo")
            .unwrap()
            .manifest
            .version()
            .unwrap(),
        "2"
    );
    assert!(bucket::resolve(t.path(), "foo@3").is_err());
}
#[test]
fn status_reuses_scope_metadata_and_checks_missing_dependencies() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "buckets/main/bucket/foo.json",
        json!({"version":"1.10","depends":["dep","main/missing"]}),
    );
    put(
        t.path(),
        "buckets/main/bucket/dep.json",
        json!({"version":"1"}),
    );
    for (name, version, global, held) in [
        ("foo", "1.9", false, true),
        ("foo", "1.10", true, false),
        ("dep", "1", false, false),
    ] {
        let base = if global { "global/apps" } else { "apps" };
        put(
            t.path(),
            &format!("{base}/{name}/current/scoop-manifest.json"),
            json!({"version":version}),
        );
        put(
            t.path(),
            &format!("{base}/{name}/current/scoop-install.json"),
            json!({"bucket":"main","architecture":"64bit","hold":held}),
        );
    }
    let c = config(t.path());
    let result = manager::statuses(&c, true).unwrap();
    let users = result
        .rows
        .iter()
        .find(|r| r["package"] == "foo" && r["scope"] == "user")
        .unwrap();
    let global = result
        .rows
        .iter()
        .find(|r| r["package"] == "foo" && r["scope"] == "global")
        .unwrap();
    assert_eq!(users["outdated"], true);
    assert_eq!(users["hold"], true);
    assert_eq!(users["latest_version"], "1.10");
    assert_eq!(users["missing_deps"], json!(["main/missing"]));
    assert_eq!(global["outdated"], false);
    assert_eq!(global["hold"], false);
    let req = rsc_core::native::invoke(
        &c,
        "status",
        json!({"apps":[{"name":"FOO","global":true},{"name":"absent","global":false}]}),
    )
    .unwrap();
    assert_eq!(req[0]["version"], "1.10");
    assert_eq!(req[1]["installed"], false);
}
#[test]
fn broken_removed_nightly_and_numeric_status_remain_distinct() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "buckets/main/bucket/night.json",
        json!({"version":"nightly"}),
    );
    put(
        t.path(),
        "buckets/main/bucket/old.json",
        json!({"version":"1.1"}),
    );
    let c = config(t.path());
    let make = |name: &str, version: &str, error: Option<String>| Installed {
        name: name.into(),
        version: Some(version.into()),
        bucket: Some("main".into()),
        architecture: Some("64bit".into()),
        scope: "user".into(),
        held: false,
        path: t.path().join(name),
        state: "installed".into(),
        error,
    };
    let states = query::statuses(
        &c,
        &[
            make("night", "nightly-20261008", None),
            make("old", "2.0", None),
            make("gone", "1", None),
            make("broken", "1", Some("failed".into())),
        ],
    )
    .unwrap();
    assert_eq!(states[0]["outdated"], true);
    assert_eq!(states[1]["outdated"], false);
    assert_eq!(states[2]["removed"], true);
    assert_eq!(states[3]["failed"], true);
}

#[test]
fn search_binary_matches_keep_filename_before_alias_and_decode_json() {
    let re = query::matcher("git").unwrap();
    let matches = rsc_core::search::matching_binaries(
        &json!([
            ["nested\\\\git.exe", "git-alias"],
            ["other.exe", "mygit"],
            "folder/git.cmd"
        ]),
        &re,
    )
    .unwrap();
    assert_eq!(matches, ["git.exe", "mygit", "git.cmd"]);
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("main/bucket/encoded.json");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, r#"{"version":"1","\u0062in":"git.exe"}"#).unwrap();
    assert_eq!(
        bucket::search(t.path(), "git").unwrap().packages[0].binaries,
        ["git.exe"]
    );
}

#[test]
fn search_installed_markers_distinguish_bucket_scope_and_version() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "buckets/main/bucket/foo.json",
        json!({"version":"1.10"}),
    );
    put(
        t.path(),
        "buckets/extras/bucket/foo.json",
        json!({"version":"2"}),
    );
    put(
        t.path(),
        "buckets/main/bucket/bar.json",
        json!({"version":"1"}),
    );
    for (scope, version, held) in [("apps", "1.9", true), ("global/apps", "1.10", false)] {
        put(
            t.path(),
            &format!("{scope}/foo/current/scoop-manifest.json"),
            json!({"version":version}),
        );
        put(
            t.path(),
            &format!("{scope}/foo/current/scoop-install.json"),
            json!({"bucket":"main","architecture":"64bit","hold":held}),
        );
    }
    let report = rsc_core::search::local(&config(t.path()), "foo").unwrap();
    let main = report.rows.iter().find(|r| r.bucket == "main").unwrap();
    assert_eq!(main.installed, "global 1.10 | user 1.9");
    assert_eq!(main.state, "global: current, user: outdated, held");
    let other = report.rows.iter().find(|r| r.bucket == "extras").unwrap();
    assert!(other.installed.is_empty() && other.state.is_empty());
}

#[test]
fn search_version_states_cover_newer_unknown_nightly_and_broken() {
    let make = |version: Option<&str>, error: Option<&str>| Installed {
        name: "app".into(),
        version: version.map(str::to_owned),
        bucket: Some("main".into()),
        architecture: None,
        scope: "user".into(),
        held: false,
        path: std::path::PathBuf::new(),
        state: "installed".into(),
        error: error.map(str::to_owned),
    };
    let state = rsc_core::search::installation_state;
    assert_eq!(state(&make(Some("1.9"), None), "1.10"), "outdated");
    assert_eq!(state(&make(Some("1.0"), None), "1.0.0"), "current");
    assert_eq!(state(&make(Some("2"), None), "1.10"), "newer");
    assert_eq!(
        state(&make(Some("nightly-20261009"), None), "nightly"),
        "unknown (nightly)"
    );
    assert_eq!(state(&make(None, None), "1"), "broken");
    assert_eq!(state(&make(Some("1"), Some("incomplete")), "1"), "broken");
}

#[test]
fn search_options_control_literal_names_and_decoded_descriptions() {
    use rsc_core::search::{Options, scan_with_options};
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "main/bucket/git.json", json!({"version":"1"}));
    put(
        t.path(),
        "main/bucket/wrapper.json",
        json!({"version":"1","bin":"git.exe"}),
    );
    put(
        t.path(),
        "main/bucket/editor.json",
        json!({"version":"1","description":"Git C++ editor 中文"}),
    );
    let names = scan_with_options(
        t.path(),
        "git",
        Options {
            name_only: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(
        names
            .packages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["git"]
    );
    let literal = scan_with_options(
        t.path(),
        "c++",
        Options {
            explicit: true,
            with_description: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(literal.packages[0].name, "editor");
    assert_eq!(literal.packages[0].description, "Git C++ editor 中文");
    let encoded = t.path().join("main/bucket/encoded.json");
    fs::write(encoded, r#"{"version":"2","description":"\u4e2d\u6587"}"#).unwrap();
    let described = scan_with_options(
        t.path(),
        "中文",
        Options {
            with_description: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(described.packages.len(), 2);
    assert!(
        scan_with_options(t.path(), "中文", Options::default())
            .unwrap()
            .packages
            .is_empty()
    );
}
#[test]
fn sqlite_search_options_restrict_fields_and_escape_literal_wildcards() {
    use rsc_core::{database, search::Options};
    let t = tempfile::tempdir().unwrap();
    let c = config(t.path());
    put(
        t.path(),
        "buckets/main/bucket/git.json",
        json!({"version":"1"}),
    );
    put(
        t.path(),
        "buckets/main/bucket/wrapper.json",
        json!({"version":"1","bin":"git.exe"}),
    );
    put(
        t.path(),
        "buckets/main/bucket/editor.json",
        json!({"version":"1","description":"100% editor"}),
    );
    database::refresh(&c).unwrap();
    let names = database::search_with_options(
        &c,
        "git",
        Options {
            name_only: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(names.as_array().unwrap().len(), 1);
    let descriptions = database::search_with_options(
        &c,
        "%",
        Options {
            explicit: true,
            with_description: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(descriptions.as_array().unwrap().len(), 1);
    assert_eq!(descriptions[0]["package"], "editor");
}
