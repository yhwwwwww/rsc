use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub root: PathBuf,
    pub global_root: PathBuf,
    pub cache: PathBuf,
    pub path_variable: String,
    pub no_junction: bool,
}
impl Layout {
    pub fn base(&self, global: bool) -> &Path {
        if global {
            &self.global_root
        } else {
            &self.root
        }
    }
    pub fn apps(&self, global: bool) -> PathBuf {
        self.base(global).join("apps")
    }
    pub fn shims(&self, global: bool) -> PathBuf {
        self.base(global).join("shims")
    }
    pub fn buckets(&self) -> PathBuf {
        self.root.join("buckets")
    }
}
