/// Full Git description embedded at build time.
pub const VERSION: &str = env!("RSC_GIT_VERSION");

pub mod bucket;
pub mod config;
pub mod database;
pub mod download;
pub mod ftp;
pub mod layout;
pub mod manager;
pub mod manifest;
pub mod native;
pub mod package;
pub mod shim;
pub mod util;

pub mod presentation;
pub mod search;
