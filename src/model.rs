use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Config {
    pub(crate) local_root: Option<String>,
    pub(crate) editor: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) sync_vcs: Option<bool>,
    pub(crate) keep_session: Option<bool>,
    pub(crate) hydrate: Option<HydrateConfig>,
    pub(crate) watcher: Option<WatcherConfig>,
    pub(crate) ignore: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HydrateConfig {
    pub(crate) max_auto_size: Option<String>,
    pub(crate) default_mode: Option<HydrationMode>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct WatcherConfig {
    pub(crate) interval: Option<String>,
    pub(crate) notify: Option<bool>,
    pub(crate) auto_stop_hydration_after: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SessionMetadata {
    #[serde(default = "crate::util::default_schema_version")]
    pub(crate) schema_version: u32,
    #[serde(default)]
    pub(crate) kind: SessionKind,
    pub(crate) session_name: String,
    pub(crate) target: String,
    pub(crate) host: String,
    pub(crate) remote_path: String,
    pub(crate) local_path: String,
    pub(crate) mode: String,
    pub(crate) sync_vcs: bool,
    #[serde(default)]
    pub(crate) keep_session: bool,
    #[serde(default)]
    pub(crate) project_session: Option<String>,
    #[serde(default)]
    pub(crate) relative_path: Option<String>,
    #[serde(default)]
    pub(crate) owner_pid: Option<u32>,
    #[serde(default)]
    pub(crate) created_at_unix: u64,
    #[serde(default)]
    pub(crate) updated_at_unix: u64,
    #[serde(default)]
    pub(crate) context_file: Option<String>,
    #[serde(default)]
    pub(crate) ignore_patterns: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionKind {
    #[default]
    Project,
    Bootstrap,
    Hydration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HydrationMode {
    Oneshot,
    Live,
}

#[derive(Debug)]
pub(crate) enum MetadataState {
    Current(Box<SessionMetadata>),
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) path: String,
}

#[derive(Debug)]
pub(crate) struct ResolvedOpen {
    pub(crate) target: Target,
    pub(crate) local_root: PathBuf,
    pub(crate) local_path: PathBuf,
    pub(crate) session_name: String,
    pub(crate) metadata_file: PathBuf,
    pub(crate) context_file: PathBuf,
    pub(crate) editor: String,
    pub(crate) mode: String,
    pub(crate) sync_vcs: bool,
    pub(crate) keep_session: bool,
    pub(crate) ignore_patterns: Vec<String>,
    pub(crate) bootstrap: bool,
    pub(crate) create_remote: bool,
    pub(crate) allow_nonempty: bool,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EditrContext {
    pub(crate) schema_version: u32,
    pub(crate) session_name: String,
    pub(crate) target: String,
    pub(crate) host: String,
    pub(crate) remote_path: String,
    pub(crate) local_root: String,
    pub(crate) local_path: String,
    pub(crate) metadata_dir: String,
    pub(crate) context_file: String,
    pub(crate) editr_bin: String,
    pub(crate) ignore_patterns: Vec<String>,
    #[serde(default)]
    pub(crate) remote_path_aliases: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Capabilities {
    pub(crate) name: &'static str,
    pub(crate) version: &'static str,
    pub(crate) context_schema_version: u32,
    pub(crate) features: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SessionRecord {
    pub(crate) session_name: String,
    pub(crate) kind: SessionKind,
    pub(crate) target: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) remote_path: Option<String>,
    pub(crate) local_path: Option<String>,
    pub(crate) project_session: Option<String>,
    pub(crate) owner_pid: Option<u32>,
    pub(crate) owner_alive: bool,
    pub(crate) keep_session: bool,
    pub(crate) mutagen_session: bool,
    pub(crate) classification: SessionClassification,
    pub(crate) age_seconds: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionClassification {
    Active,
    Persistent,
    Suspicious,
    Orphaned,
    StuckHydration,
}

#[derive(Debug, Serialize)]
pub(crate) struct HydrateResponse {
    pub(crate) remote_path: String,
    pub(crate) local_path: String,
    pub(crate) relative_path: String,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) local_exists: bool,
    pub(crate) mode: HydrationMode,
    pub(crate) hydrated: bool,
    pub(crate) session_name: Option<String>,
    pub(crate) over_limit: bool,
}

pub(crate) struct HydrationRuntime {
    pub(crate) owner_pid: u32,
    pub(crate) quiet: bool,
}
