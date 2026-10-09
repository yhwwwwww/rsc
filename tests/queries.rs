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
fn parallel_search_preserves_names_aliases_architectures_and_order() {
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
        json!({"version":"2","bin":[["app.exe","aliased"]],"architecture":{"arm64":{"bin":"arm-only.exe"}}}),
    );
    put(
        root,
        "extras/bucket/TOOL.json",
        json!({"Version":"3","Description":"extras"}),
    );
    let all = bucket::index(root).unwrap();
    let empty = bucket::search(root, "").unwrap();
    assert_eq!(all.packages.len(), 102);
    assert_eq!(
        all.packages.iter().map(|p| &p.source).collect::<Vec<_>>(),
        empty.packages.iter().map(|p| &p.source).collect::<Vec<_>>()
    );
    for input in ["^tool", "^aliased$", "^arm-only$", "(?i)^ALIAS$"] {
        let re = query::matcher(input).unwrap();
        let expected = all
            .packages
            .iter()
            .filter(|p| {
                query::matches(&re, &p.name).unwrap()
                    || [
                        rsc_core::manifest::Architecture::X64,
                        rsc_core::manifest::Architecture::X86,
                        rsc_core::manifest::Architecture::Arm64,
                    ]
                    .iter()
                    .any(|a| {
                        p.manifest
                            .bins(*a)
                            .iter()
                            .any(|b| query::matches(&re, b).unwrap())
                    })
            })
            .map(|p| &p.source)
            .collect::<Vec<_>>();
        let matched = bucket::search(root, input).unwrap();
        assert_eq!(
            expected,
            matched
                .packages
                .iter()
                .map(|p| &p.source)
                .collect::<Vec<_>>()
        );
    }
    assert!(bucket::search(root, "(").is_err());
}
#[test]
fn search_keeps_diagnostics_and_observes_edits_and_deletions() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "main/bucket/a.json", json!({"version":"1"}));
    put(t.path(), "main/bucket/broken.json", json!(false));
    let first = bucket::search(t.path(), "a").unwrap();
    assert_eq!(first.warnings.len(), 1);
    assert_eq!(first.packages[0].manifest.version().unwrap(), "1");
    put(t.path(), "main/bucket/a.json", json!({"version":"2"}));
    assert_eq!(
        bucket::search(t.path(), "a").unwrap().packages[0]
            .manifest
            .version()
            .unwrap(),
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
