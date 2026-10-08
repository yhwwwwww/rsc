//! Public bucket identities, maintained independently as configuration data.
pub const BUCKETS: &[(&str, &str)] = &[
    ("main", "ScoopInstaller/Main"),
    ("extras", "ScoopInstaller/Extras"),
    ("versions", "ScoopInstaller/Versions"),
    ("nirsoft", "ScoopInstaller/Nirsoft"),
    ("sysinternals", "ScoopInstaller/Sysinternals"),
    ("php", "ScoopInstaller/PHP"),
    ("nerd-fonts", "matthewjberger/scoop-nerd-fonts"),
    ("nonportable", "ScoopInstaller/Nonportable"),
    ("java", "ScoopInstaller/Java"),
    ("games", "Calinou/scoop-games"),
];
pub fn json() -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for &(name, repo) in BUCKETS {
        m.insert(
            name.into(),
            serde_json::json!(format!("https://github.com/{repo}")),
        );
    }
    m.into()
}
