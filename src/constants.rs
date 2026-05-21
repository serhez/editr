pub(crate) const DEFAULT_MODE: &str = "two-way-safe";
pub(crate) const DEFAULT_EDITOR: &str = "nvim";
pub(crate) const CONTEXT_SCHEMA_VERSION: u32 = 1;
pub(crate) const DEFAULT_MAX_AUTO_HYDRATE_SIZE: &str = "25 MB";
pub(crate) const DEFAULT_WATCH_INTERVAL: &str = "60s";
pub(crate) const DEFAULT_AUTO_STOP_HYDRATION_AFTER: &str = "10m";
pub(crate) const SSH_PROBE_OPTIONS: &[&str] = &[
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ServerAliveInterval=5",
    "-o",
    "ServerAliveCountMax=1",
];
pub(crate) const DEFAULT_CONFIG: &str = r#"# editr config
#
# `editr host:/absolute/remote/path` keeps a local Mutagen mirror and opens
# your editor from that mirror so local editor plugins, LSPs, formatters, and
# git tools see a normal local workspace.

local_root = "~/remote"
editor = "nvim"
mode = "two-way-safe"
sync_vcs = true
keep_session = false

[hydrate]
max_auto_size = "25 MB"
default_mode = "live"

[watcher]
interval = "60s"
notify = true
auto_stop_hydration_after = "10m"

[ignore]
"*" = [
  ".DS_Store",
  ".venv/",
  "venv/",
  "__pycache__/",
  ".mypy_cache/",
  ".ruff_cache/",
  ".pytest_cache/",
  "node_modules/",
  ".git/*.lock",
  ".git/**/*.lock",
]

# Target-specific ignores use the canonical target string.
# "*" and "?" are supported; "*" may match across path separators.
#
# "cluster:/home/user/project*" = [
#   "wandb/",
#   "checkpoints/",
#   "*.pt",
# ]
"#;
