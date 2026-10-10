use std::{env, path::Path, process::Command};

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is missing");
    let root = Path::new(&root);
    println!("cargo:rerun-if-env-changed=RSC_GIT_VERSION");
    for name in ["HEAD", "refs", "packed-refs", "shallow"] {
        if let Some(location) = git(root, &["rev-parse", "--git-path", name]) {
            let path = root.join(location);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    let version = env::var("RSC_GIT_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git(root, &["describe", "--tags"]))
        .expect("Cannot derive the rsc version. Fetch Git tags and build with scripts/build.ps1.");
    assert!(!version.contains(['\n', '\r']), "Invalid Git version");
    println!("cargo:rustc-env=RSC_GIT_VERSION={version}");
}
