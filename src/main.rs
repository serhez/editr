use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_MODE: &str = "two-way-safe";
const DEFAULT_EDITOR: &str = "nvim";
const CONTEXT_SCHEMA_VERSION: u32 = 1;
const DEFAULT_MAX_AUTO_HYDRATE_SIZE: &str = "25 MB";
const DEFAULT_WATCH_INTERVAL: &str = "60s";
const DEFAULT_AUTO_STOP_HYDRATION_AFTER: &str = "10m";
const SSH_PROBE_OPTIONS: &[&str] = &[
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ServerAliveInterval=5",
    "-o",
    "ServerAliveCountMax=1",
];
const DEFAULT_CONFIG: &str = r#"# editr config
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

#[derive(Debug, Parser)]
#[command(
    name = "editr",
    version,
    about = "Open remote projects in local editor through a Mutagen-backed mirror",
    long_about = None,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(
        value_name = "TARGET",
        help = "Remote target, e.g. host:/absolute/path"
    )]
    target: Option<String>,

    #[command(flatten)]
    open_options: OpenOptions,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Open a remote target in the configured editor.
    Open(OpenCommand),
    /// Report CLI features for companion integrations.
    Capabilities(CapabilitiesCommand),
    /// Copy or sync one remote file into an editr mirror.
    Hydrate(HydrateCommand),
    /// List editr-tracked sessions.
    List(ListCommand),
    /// Show Mutagen status for all sessions, a target, or a session name.
    Status(SessionCommand),
    /// Flush a target/session, or every editr-tracked session with --all.
    Flush(SessionCommand),
    /// Stop a target/session, or every editr-tracked session with --all.
    Stop(StopCommand),
    /// Watch for stale editr sessions and optionally notify.
    Watch(WatchCommand),
}

#[derive(Debug, Args)]
struct OpenCommand {
    #[arg(
        value_name = "TARGET",
        help = "Remote target, e.g. host:/absolute/path"
    )]
    target: String,

    #[command(flatten)]
    options: OpenOptions,
}

#[derive(Clone, Debug, Args)]
struct OpenOptions {
    /// Local mirror root.
    #[arg(long, short = 'L', value_name = "PATH")]
    local_root: Option<PathBuf>,

    /// Exact local mirror path. Advanced; normally derived from --local-root and TARGET.
    #[arg(long, value_name = "PATH")]
    local_path: Option<PathBuf>,

    /// Exact Mutagen session name. Advanced; normally derived from TARGET.
    #[arg(long, value_name = "NAME")]
    session_name: Option<String>,

    /// Config file path.
    #[arg(long, short = 'C', value_name = "PATH")]
    config: Option<PathBuf>,

    /// Extra Mutagen ignore pattern. Can be passed multiple times.
    #[arg(long, short = 'i', value_name = "PATTERN")]
    ignore: Vec<String>,

    /// Mutagen mode for the steady-state session.
    #[arg(long, short = 'm', value_name = "MODE")]
    mode: Option<String>,

    /// Editor shell command to launch from the local mirror.
    #[arg(long, short = 'e', value_name = "COMMAND")]
    editor: Option<String>,

    /// Skip the initial remote-to-local refresh.
    #[arg(long)]
    no_bootstrap: bool,

    /// Create the remote directory if it does not exist.
    #[arg(long)]
    create_remote: bool,

    /// Allow starting/resuming when the local mirror is non-empty and unmarked.
    #[arg(long)]
    allow_nonempty: bool,

    /// Sync the remote .git directory and other VCS metadata.
    #[arg(long, conflicts_with = "ignore_vcs")]
    sync_vcs: bool,

    /// Ask Mutagen to ignore VCS metadata.
    #[arg(long)]
    ignore_vcs: bool,

    /// Keep the Mutagen session running after the editor exits.
    #[arg(long)]
    keep_session: bool,

    /// Print the resolved plan without creating sessions or opening the editor.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct CapabilitiesCommand {
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct HydrateCommand {
    /// editr context JSON file, usually from EDITR_CONTEXT.
    #[arg(long, value_name = "PATH")]
    context: PathBuf,

    /// Absolute remote file path to hydrate.
    #[arg(long, value_name = "PATH")]
    remote_path: String,

    /// Hydration mode.
    #[arg(long, value_enum)]
    mode: Option<HydrationMode>,

    /// Maximum file size to hydrate without --allow-large.
    #[arg(long, value_name = "SIZE")]
    max_size: Option<String>,

    /// Report remote size and mapping without creating a Mutagen session.
    #[arg(long)]
    check: bool,

    /// Hydrate even when the remote file exceeds --max-size.
    #[arg(long)]
    allow_large: bool,

    /// Allow hydrating over an existing local file.
    #[arg(long)]
    allow_existing: bool,

    /// Process id that owns a live hydration session.
    #[arg(long, value_name = "PID")]
    owner_pid: Option<u32>,

    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListCommand {
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,

    #[command(flatten)]
    manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
struct SessionCommand {
    #[arg(value_name = "TARGET_OR_SESSION")]
    selector: Option<String>,

    /// Apply to every session recorded under editr's metadata directory.
    #[arg(long)]
    all: bool,

    #[command(flatten)]
    manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
struct StopCommand {
    #[arg(value_name = "TARGET_OR_SESSION")]
    selector: Option<String>,

    /// Stop every session recorded under editr's metadata directory.
    #[arg(long)]
    all: bool,

    #[command(flatten)]
    manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
struct WatchCommand {
    /// Polling interval, for example 60s or 2m.
    #[arg(long, value_name = "DURATION")]
    interval: Option<String>,

    /// Enable OS notifications for suspicious sessions.
    #[arg(long, conflicts_with = "no_notify")]
    notify: bool,

    /// Disable OS notifications.
    #[arg(long)]
    no_notify: bool,

    /// Run one scan and exit.
    #[arg(long)]
    once: bool,

    /// Emit machine-readable JSON for each scan.
    #[arg(long)]
    json: bool,

    /// Auto-stop hydration sessions older than this duration.
    #[arg(long, value_name = "DURATION")]
    auto_stop_hydration_after: Option<String>,

    #[command(flatten)]
    manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
struct ManagerOptions {
    /// Local mirror root. Used to find editr session metadata for --all.
    #[arg(long, short = 'L', value_name = "PATH")]
    local_root: Option<PathBuf>,

    /// Config file path.
    #[arg(long, short = 'C', value_name = "PATH")]
    config: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    local_root: Option<String>,
    editor: Option<String>,
    mode: Option<String>,
    sync_vcs: Option<bool>,
    keep_session: Option<bool>,
    hydrate: Option<HydrateConfig>,
    watcher: Option<WatcherConfig>,
    ignore: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Default, Deserialize)]
struct HydrateConfig {
    max_auto_size: Option<String>,
    default_mode: Option<HydrationMode>,
}

#[derive(Debug, Default, Deserialize)]
struct WatcherConfig {
    interval: Option<String>,
    notify: Option<bool>,
    auto_stop_hydration_after: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionMetadata {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    kind: SessionKind,
    session_name: String,
    target: String,
    host: String,
    remote_path: String,
    local_path: String,
    mode: String,
    sync_vcs: bool,
    #[serde(default)]
    keep_session: bool,
    #[serde(default)]
    project_session: Option<String>,
    #[serde(default)]
    relative_path: Option<String>,
    #[serde(default)]
    owner_pid: Option<u32>,
    #[serde(default)]
    created_at_unix: u64,
    #[serde(default)]
    updated_at_unix: u64,
    #[serde(default)]
    context_file: Option<String>,
    #[serde(default)]
    ignore_patterns: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionKind {
    #[default]
    Project,
    Bootstrap,
    Hydration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
enum HydrationMode {
    Oneshot,
    Live,
}

#[derive(Debug)]
enum MetadataState {
    Current(Box<SessionMetadata>),
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Target {
    host: String,
    path: String,
}

#[derive(Debug)]
struct ResolvedOpen {
    target: Target,
    local_root: PathBuf,
    local_path: PathBuf,
    session_name: String,
    metadata_file: PathBuf,
    context_file: PathBuf,
    editor: String,
    mode: String,
    sync_vcs: bool,
    keep_session: bool,
    ignore_patterns: Vec<String>,
    bootstrap: bool,
    create_remote: bool,
    allow_nonempty: bool,
    dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct EditrContext {
    schema_version: u32,
    session_name: String,
    target: String,
    host: String,
    remote_path: String,
    local_root: String,
    local_path: String,
    metadata_dir: String,
    context_file: String,
    editr_bin: String,
    ignore_patterns: Vec<String>,
    #[serde(default)]
    remote_path_aliases: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Capabilities {
    name: &'static str,
    version: &'static str,
    context_schema_version: u32,
    features: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct SessionRecord {
    session_name: String,
    kind: SessionKind,
    target: Option<String>,
    host: Option<String>,
    remote_path: Option<String>,
    local_path: Option<String>,
    project_session: Option<String>,
    owner_pid: Option<u32>,
    owner_alive: bool,
    keep_session: bool,
    mutagen_session: bool,
    classification: SessionClassification,
    age_seconds: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SessionClassification {
    Active,
    Persistent,
    Suspicious,
    Orphaned,
    StuckHydration,
}

#[derive(Debug, Serialize)]
struct HydrateResponse {
    remote_path: String,
    local_path: String,
    relative_path: String,
    size_bytes: Option<u64>,
    local_exists: bool,
    mode: HydrationMode,
    hydrated: bool,
    session_name: Option<String>,
    over_limit: bool,
}

struct HydrationRuntime {
    owner_pid: u32,
    quiet: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Open(open)) => {
            let mut options = cli.open_options;
            options.merge(open.options);
            open_target(&open.target, &options)
        }
        Some(Commands::Capabilities(command)) => capabilities(command),
        Some(Commands::Hydrate(command)) => hydrate(command),
        Some(Commands::List(command)) => list(command),
        Some(Commands::Status(command)) => status(command),
        Some(Commands::Flush(command)) => flush(command),
        Some(Commands::Stop(command)) => stop(command),
        Some(Commands::Watch(command)) => watch(command),
        None => {
            let Some(target) = cli.target else {
                Cli::command().print_help()?;
                println!();
                return Ok(());
            };
            open_target(&target, &cli.open_options)
        }
    }
}

impl OpenOptions {
    fn merge(&mut self, other: OpenOptions) {
        self.local_root = other.local_root.or_else(|| self.local_root.take());
        self.local_path = other.local_path.or_else(|| self.local_path.take());
        self.session_name = other.session_name.or_else(|| self.session_name.take());
        self.config = other.config.or_else(|| self.config.take());
        self.ignore.extend(other.ignore);
        self.mode = other.mode.or_else(|| self.mode.take());
        self.editor = other.editor.or_else(|| self.editor.take());
        self.no_bootstrap |= other.no_bootstrap;
        self.create_remote |= other.create_remote;
        self.allow_nonempty |= other.allow_nonempty;
        self.sync_vcs |= other.sync_vcs;
        self.ignore_vcs |= other.ignore_vcs;
        self.keep_session |= other.keep_session;
        self.dry_run |= other.dry_run;
    }
}

fn open_target(target: &str, options: &OpenOptions) -> Result<()> {
    let resolved = resolve_open(target, options)?;

    print_plan(&resolved);
    if resolved.dry_run {
        return Ok(());
    }

    require_command("mutagen")?;
    require_command("ssh")?;

    fs::create_dir_all(&resolved.local_path)
        .with_context(|| format!("failed to create {}", resolved.local_path.display()))?;
    let metadata_dir = resolved
        .metadata_file
        .parent()
        .context("metadata file has no parent directory")?;
    fs::create_dir_all(metadata_dir)
        .with_context(|| format!("failed to create {}", metadata_dir.display()))?;

    let session_exists = mutagen_session_exists(&resolved.session_name)?;
    let metadata_state = metadata_state(&resolved)?;
    let mut recreate_session = false;
    if session_exists {
        match &metadata_state {
            MetadataState::Current(metadata) => {
                recreate_session = session_needs_reconfigure(metadata, &resolved);
            }
            MetadataState::Missing => {
                if !resolved.allow_nonempty && directory_is_nonempty(&resolved.local_path)? {
                    bail!(
                        "refusing to resume an existing Mutagen session with an unmarked non-empty local mirror:\n  {}\npass --allow-nonempty once if this is already the correct editr mirror",
                        resolved.local_path.display()
                    );
                }
                recreate_session = true;
            }
        }
    } else if matches!(metadata_state, MetadataState::Current(_))
        && !resolved.allow_nonempty
        && directory_is_nonempty(&resolved.local_path)?
    {
        bail!(
            "found editr metadata for a previous session, but no active Mutagen session:\n  {}\nrefusing to bootstrap over a possibly dirty local mirror. Reopen with --no-bootstrap --allow-nonempty to reconcile, or inspect/remove the mirror intentionally.",
            resolved.local_path.display()
        );
    } else if !resolved.bootstrap
        && !resolved.allow_nonempty
        && directory_is_nonempty(&resolved.local_path)?
    {
        bail!(
            "refusing to create a new sync session with a non-empty local mirror:\n  {}\nenable the default bootstrap, move the directory, or pass --allow-nonempty intentionally",
            resolved.local_path.display()
        );
    }

    ensure_remote_directory(&resolved.target, resolved.create_remote)?;

    if session_exists {
        if recreate_session {
            reconfigure_session(&resolved)?;
        } else {
            run_mutagen_quiet(["sync", "resume", &resolved.session_name]);
            write_metadata(&resolved)?;
        }
    } else {
        if resolved.bootstrap {
            bootstrap_from_remote(&resolved)?;
        }
        create_steady_session(&resolved)?;
        write_metadata(&resolved)?;
    }

    run_mutagen_checked(
        ["sync", "flush", &resolved.session_name],
        "mutagen sync flush",
    )?;
    let context = write_context(&resolved)?;
    if !resolved.keep_session {
        install_signal_cleanup(&resolved)?;
    }
    let editor_result = open_editor(&resolved.editor, &resolved.local_path, &context);
    let cleanup_result = if resolved.keep_session {
        Ok(())
    } else {
        cleanup_session_after_editor(&resolved)
    };
    match (editor_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(editor_error), Ok(())) => Err(editor_error),
        (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
        (Err(editor_error), Err(cleanup_error)) => {
            eprintln!("warning: failed to clean up Mutagen session: {cleanup_error:#}");
            Err(editor_error)
        }
    }
}

fn resolve_open(target: &str, options: &OpenOptions) -> Result<ResolvedOpen> {
    let target = Target::parse(target)?;
    let config = load_config(options.config.as_deref())?;
    let local_root = options
        .local_root
        .clone()
        .or_else(|| config.local_root.as_deref().map(expand_path))
        .unwrap_or_else(default_local_root);
    let local_root = expand_path(local_root.to_string_lossy().as_ref());
    let local_path = options
        .local_path
        .clone()
        .map(|path| expand_path(path.to_string_lossy().as_ref()))
        .unwrap_or_else(|| local_path_for_target(&local_root, &target));
    let session_name = options
        .session_name
        .clone()
        .unwrap_or_else(|| session_name_for_target(&target));
    if session_name.trim().is_empty() {
        bail!("session name cannot be empty");
    }
    let metadata_dir = metadata_dir(&local_root);
    let metadata_file = metadata_dir.join(format!("{session_name}.toml"));
    let context_file = metadata_dir.join(format!("{session_name}.context.json"));
    let editor = options
        .editor
        .clone()
        .or_else(|| config.editor.clone())
        .unwrap_or_else(|| DEFAULT_EDITOR.to_string());
    let editor = editor.trim().to_string();
    if editor.is_empty() {
        bail!("editor command cannot be empty");
    }
    let mode = options
        .mode
        .clone()
        .or_else(|| config.mode.clone())
        .unwrap_or_else(|| DEFAULT_MODE.to_string());
    let sync_vcs = if options.ignore_vcs {
        false
    } else if options.sync_vcs {
        true
    } else {
        config.sync_vcs.unwrap_or(true)
    };
    let keep_session = options.keep_session || config.keep_session.unwrap_or(false);
    let mut ignore_patterns = config_ignore_patterns(&config, &target.to_string())?;
    ignore_patterns.extend(
        options
            .ignore
            .iter()
            .filter(|pattern| !pattern.is_empty())
            .cloned(),
    );
    dedupe_patterns(&mut ignore_patterns);

    Ok(ResolvedOpen {
        target,
        local_root,
        local_path,
        session_name,
        metadata_file,
        context_file,
        editor,
        mode,
        sync_vcs,
        keep_session,
        ignore_patterns,
        bootstrap: !options.no_bootstrap,
        create_remote: options.create_remote,
        allow_nonempty: options.allow_nonempty,
        dry_run: options.dry_run,
    })
}

fn print_plan(resolved: &ResolvedOpen) {
    println!("Mutagen session: {}", resolved.session_name);
    println!("Local mirror:    {}", resolved.local_path.display());
    println!("Remote path:     {}", resolved.target);
    println!("Editor:          {}", resolved.editor);
    println!("Mode:            {}", resolved.mode);
    println!(
        "VCS metadata:    {}",
        if resolved.sync_vcs { "sync" } else { "ignore" }
    );
    println!(
        "Session:         {}",
        if resolved.keep_session {
            "keep after editor exits"
        } else {
            "terminate after editor exits"
        }
    );
    if !resolved.ignore_patterns.is_empty() {
        println!("Ignore patterns: {}", resolved.ignore_patterns.join(", "));
    }
}

fn capabilities(command: CapabilitiesCommand) -> Result<()> {
    let capabilities = Capabilities {
        name: "editr",
        version: env!("CARGO_PKG_VERSION"),
        context_schema_version: CONTEXT_SCHEMA_VERSION,
        features: vec!["context-v1", "hydrate-v1", "list-json-v1", "watch-v1"],
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(&capabilities)?);
    } else {
        println!("editr {}", capabilities.version);
        println!("Context schema: {}", capabilities.context_schema_version);
        println!("Features: {}", capabilities.features.join(", "));
    }
    Ok(())
}

fn list(command: ListCommand) -> Result<()> {
    let records = session_records(&command.manager_options)?;
    if command.json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("No editr-tracked sessions found.");
        return Ok(());
    }
    for record in records {
        println!(
            "{}\t{:?}\t{:?}\t{}",
            record.session_name,
            record.kind,
            record.classification,
            record.local_path.unwrap_or_else(|| "-".to_string())
        );
    }
    Ok(())
}

fn hydrate(command: HydrateCommand) -> Result<()> {
    let context = load_context(&command.context)?;
    let config = load_config(None)?;
    let mode = command
        .mode
        .clone()
        .or_else(|| {
            config
                .hydrate
                .as_ref()
                .and_then(|hydrate| hydrate.default_mode.clone())
        })
        .unwrap_or(HydrationMode::Live);
    let max_size = command
        .max_size
        .as_deref()
        .or_else(|| {
            config
                .hydrate
                .as_ref()
                .and_then(|hydrate| hydrate.max_auto_size.as_deref())
        })
        .unwrap_or(DEFAULT_MAX_AUTO_HYDRATE_SIZE);
    let max_size = parse_size(max_size)?;

    let remote_path = normalize_remote_path(&command.remote_path);
    let relative_path = relative_remote_path_with_aliases(
        &context.remote_path,
        &context.remote_path_aliases,
        &remote_path,
    )?;
    let local_path = PathBuf::from(&context.local_path).join(&relative_path);
    let size_bytes = remote_file_size(&context.host, &remote_path)?;
    let local_exists = local_path.exists();
    let over_limit = size_bytes.is_some_and(|size| size > max_size);
    let session_name = hydration_session_name(&context.session_name, &remote_path);
    let response = HydrateResponse {
        remote_path: remote_path.clone(),
        local_path: local_path.display().to_string(),
        relative_path: relative_path.clone(),
        size_bytes,
        local_exists,
        mode: mode.clone(),
        hydrated: false,
        session_name: (mode == HydrationMode::Live).then(|| session_name.clone()),
        over_limit,
    };

    if command.check {
        print_hydrate_response(&response, command.json)?;
        return Ok(());
    }
    if over_limit && !command.allow_large {
        print_hydrate_response(&response, command.json)?;
        bail!(
            "remote file exceeds max size ({} > {})",
            format_size(size_bytes.unwrap_or(0)),
            format_size(max_size)
        );
    }
    if local_exists
        && !command.allow_existing
        && !existing_hydration_matches(&context, &session_name, &remote_path, &local_path)?
    {
        bail!(
            "refusing to hydrate over existing local file without --allow-existing:\n  {}",
            local_path.display()
        );
    }

    perform_hydration(
        &context,
        &remote_path,
        &relative_path,
        &local_path,
        &session_name,
        &mode,
        HydrationRuntime {
            owner_pid: command.owner_pid.unwrap_or_else(std::process::id),
            quiet: command.json,
        },
    )?;

    let response = HydrateResponse {
        hydrated: true,
        ..response
    };
    print_hydrate_response(&response, command.json)
}

fn print_hydrate_response(response: &HydrateResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(response)?);
    } else if response.hydrated {
        println!("Hydrated {}", response.local_path);
    } else {
        println!("Remote path: {}", response.remote_path);
        println!("Local path:  {}", response.local_path);
        if let Some(size) = response.size_bytes {
            println!("Size:        {}", format_size(size));
        }
        println!("Over limit:  {}", response.over_limit);
    }
    Ok(())
}

fn status(command: SessionCommand) -> Result<()> {
    if command.all && command.selector.is_some() {
        bail!("pass either --all or a target/session, not both");
    }
    if command.all {
        let sessions = tracked_sessions(&command.manager_options)?;
        if sessions.is_empty() {
            println!("No editr-tracked sessions found.");
            return Ok(());
        }
        for session in sessions {
            run_mutagen_checked(["sync", "list", session.as_str()], "mutagen sync list")?;
        }
        return Ok(());
    }
    match command.selector {
        Some(selector) => {
            let session = selector_to_session(&selector)?;
            run_mutagen_checked(["sync", "list", session.as_str()], "mutagen sync list")
        }
        None => run_mutagen_checked(["sync", "list"], "mutagen sync list"),
    }
}

fn flush(command: SessionCommand) -> Result<()> {
    if command.all && command.selector.is_some() {
        bail!("pass either --all or a target/session, not both");
    }
    if command.all {
        let sessions = tracked_sessions(&command.manager_options)?;
        if sessions.is_empty() {
            println!("No editr-tracked sessions found.");
            return Ok(());
        }
        for session in sessions {
            run_mutagen_checked(["sync", "flush", session.as_str()], "mutagen sync flush")?;
        }
        return Ok(());
    }
    let Some(selector) = command.selector else {
        bail!("flush requires a target/session, or --all");
    };
    let session = selector_to_session(&selector)?;
    run_mutagen_checked(["sync", "flush", session.as_str()], "mutagen sync flush")
}

fn stop(command: StopCommand) -> Result<()> {
    if command.all && command.selector.is_some() {
        bail!("pass either --all or a target/session, not both");
    }
    if command.all {
        let local_root = manager_local_root(&command.manager_options)?;
        let sessions = metadata_sessions(&local_root)?;
        if sessions.is_empty() {
            println!("No editr-tracked sessions found.");
            return Ok(());
        }
        for session in sessions {
            terminate_mutagen_session_if_present(&session)?;
            remove_session_files(&local_root, &session)?;
        }
        return Ok(());
    }
    let Some(selector) = command.selector else {
        bail!("stop requires a target/session, or --all");
    };
    let session = selector_to_session(&selector)?;
    terminate_mutagen_session_if_present(&session)?;
    let local_root = manager_local_root(&command.manager_options)?;
    remove_session_files(&local_root, &session)
}

fn watch(command: WatchCommand) -> Result<()> {
    let config = load_config(command.manager_options.config.as_deref())?;
    let interval = command
        .interval
        .as_deref()
        .or_else(|| {
            config
                .watcher
                .as_ref()
                .and_then(|watcher| watcher.interval.as_deref())
        })
        .unwrap_or(DEFAULT_WATCH_INTERVAL);
    let interval = parse_duration(interval)?;
    let auto_stop_after = command
        .auto_stop_hydration_after
        .as_deref()
        .or_else(|| {
            config
                .watcher
                .as_ref()
                .and_then(|watcher| watcher.auto_stop_hydration_after.as_deref())
        })
        .unwrap_or(DEFAULT_AUTO_STOP_HYDRATION_AFTER);
    let auto_stop_after = parse_duration(auto_stop_after)?;
    let notify = if command.no_notify {
        false
    } else if command.notify {
        true
    } else {
        config
            .watcher
            .as_ref()
            .and_then(|watcher| watcher.notify)
            .unwrap_or(true)
    };

    let mut notified_sessions = Vec::new();
    loop {
        let records = session_records(&command.manager_options)?;
        for record in &records {
            if record.classification == SessionClassification::StuckHydration
                && record
                    .age_seconds
                    .is_some_and(|age| age >= auto_stop_after.as_secs())
            {
                run_mutagen_quiet(["sync", "terminate", record.session_name.as_str()]);
                let local_root = manager_local_root(&command.manager_options)?;
                remove_session_files(&local_root, &record.session_name)?;
                continue;
            }
            if notify
                && matches!(
                    record.classification,
                    SessionClassification::Suspicious
                        | SessionClassification::Orphaned
                        | SessionClassification::StuckHydration
                )
            {
                let notification_key =
                    format!("{}:{:?}", record.session_name, record.classification);
                if notified_sessions.contains(&notification_key) {
                    continue;
                }
                notified_sessions.push(notification_key);
                notify_user(
                    "editr session needs attention",
                    &format!("{} is {:?}", record.session_name, record.classification),
                );
            }
        }
        if command.json {
            println!("{}", serde_json::to_string_pretty(&records)?);
        } else {
            for record in records.iter().filter(|record| {
                !matches!(
                    record.classification,
                    SessionClassification::Active | SessionClassification::Persistent
                )
            }) {
                println!("{}: {:?}", record.session_name, record.classification);
            }
        }
        if command.once {
            return Ok(());
        }
        thread::sleep(interval);
    }
}

fn load_config(path: Option<&Path>) -> Result<Config> {
    let path = config_path(path)?;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, DEFAULT_CONFIG)
            .with_context(|| format!("failed to create default config {}", path.display()))?;
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("invalid config TOML: {}", path.display()))
}

fn config_path(path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = path {
        return Ok(expand_path(path.to_string_lossy().as_ref()));
    }
    if let Ok(path) = env::var("EDITR_CONFIG")
        && !path.trim().is_empty()
    {
        return Ok(expand_path(&path));
    }
    let home = home_dir()?;
    Ok(home.join(".config").join("editr").join("config.toml"))
}

fn default_local_root() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("remote")
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .context("could not find the user home directory")
}

fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).into_owned())
}

fn config_ignore_patterns(config: &Config, target: &str) -> Result<Vec<String>> {
    let patterns_by_target = config.ignore.as_ref();
    let Some(patterns_by_target) = patterns_by_target else {
        return Ok(Vec::new());
    };
    let mut patterns = Vec::new();
    if let Some(global) = patterns_by_target.get("*") {
        patterns.extend(global.iter().cloned());
    }
    for (pattern, values) in patterns_by_target {
        if pattern == "*" {
            continue;
        }
        if wildcard_match(pattern, target) {
            patterns.extend(values.iter().cloned());
        }
    }
    Ok(patterns)
}

fn dedupe_patterns(patterns: &mut Vec<String>) {
    let mut deduped = Vec::with_capacity(patterns.len());
    for pattern in patterns.drain(..) {
        if !deduped.contains(&pattern) {
            deduped.push(pattern);
        }
    }
    *patterns = deduped;
}

impl Target {
    fn parse(input: &str) -> Result<Self> {
        let Some(index) = input.find(":/") else {
            bail!("target must use scp-style syntax: host:/absolute/path");
        };
        let (host, path_with_colon) = input.split_at(index);
        let path = &path_with_colon[1..];
        if host.is_empty() {
            bail!("target host cannot be empty");
        }
        if !path.starts_with('/') {
            bail!("target path must be absolute");
        }
        if path.contains('\0') {
            bail!("target path contains a NUL byte");
        }
        Ok(Self {
            host: host.to_string(),
            path: normalize_remote_path(path),
        })
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.host, self.path)
    }
}

fn normalize_remote_path(path: &str) -> String {
    let mut path = path.to_string();
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

fn local_path_for_target(local_root: &Path, target: &Target) -> PathBuf {
    let mut path = local_root.join(sanitize_component(&target.host));
    for component in target.path.trim_start_matches('/').split('/') {
        if !component.is_empty() {
            path.push(component);
        }
    }
    path
}

fn metadata_dir(local_root: &Path) -> PathBuf {
    local_root.join(".editr-sessions")
}

fn metadata_state(resolved: &ResolvedOpen) -> Result<MetadataState> {
    if resolved.metadata_file.exists() {
        let contents = fs::read_to_string(&resolved.metadata_file)
            .with_context(|| format!("failed to read {}", resolved.metadata_file.display()))?;
        let metadata: SessionMetadata = toml::from_str(&contents).with_context(|| {
            format!(
                "invalid metadata TOML: {}",
                resolved.metadata_file.display()
            )
        })?;
        return Ok(MetadataState::Current(Box::new(metadata)));
    }
    Ok(MetadataState::Missing)
}

fn session_needs_reconfigure(metadata: &SessionMetadata, resolved: &ResolvedOpen) -> bool {
    metadata.target != resolved.target.to_string()
        || metadata.local_path != resolved.local_path.display().to_string()
        || metadata.mode != resolved.mode
        || metadata.sync_vcs != resolved.sync_vcs
        || metadata.ignore_patterns != resolved.ignore_patterns
}

fn session_name_for_target(target: &Target) -> String {
    let mut hasher = Sha256::new();
    hasher.update(target.to_string());
    let digest = hasher.finalize();
    let hash = hex_prefix(&digest, 12);
    format!("editr-{}-{hash}", sanitize_component(&target.host))
}

fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(chars);
    for byte in bytes {
        if output.len() >= chars {
            break;
        }
        output.push(HEX[(byte >> 4) as usize] as char);
        if output.len() >= chars {
            break;
        }
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn sanitize_component(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            output.push(byte as char);
        } else {
            output.push('-');
        }
    }
    output.trim_matches('-').to_string()
}

fn selector_to_session(selector: &str) -> Result<String> {
    match Target::parse(selector) {
        Ok(target) => Ok(session_name_for_target(&target)),
        Err(_) => Ok(selector.to_string()),
    }
}

fn manager_local_root(options: &ManagerOptions) -> Result<PathBuf> {
    let config = load_config(options.config.as_deref())?;
    let local_root = options
        .local_root
        .clone()
        .or_else(|| config.local_root.as_deref().map(expand_path))
        .unwrap_or_else(default_local_root);
    Ok(expand_path(local_root.to_string_lossy().as_ref()))
}

fn tracked_sessions(options: &ManagerOptions) -> Result<Vec<String>> {
    metadata_sessions(&manager_local_root(options)?)
}

fn metadata_sessions(local_root: &Path) -> Result<Vec<String>> {
    let directory = metadata_dir(local_root);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("toml")) {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(OsStr::to_str) {
            sessions.push(stem.to_string());
        }
    }
    sessions.sort();
    Ok(sessions)
}

fn load_metadata_file(path: &Path) -> Result<SessionMetadata> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("invalid metadata TOML: {}", path.display()))
}

fn session_records(options: &ManagerOptions) -> Result<Vec<SessionRecord>> {
    let local_root = manager_local_root(options)?;
    let directory = metadata_dir(&local_root);
    let mut records = Vec::new();
    let mut seen = Vec::new();
    if directory.exists() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new("toml")) {
                continue;
            }
            let metadata = load_metadata_file(&path)?;
            seen.push(metadata.session_name.clone());
            records.push(record_for_metadata(&metadata)?);
        }
    }

    for session in mutagen_editr_sessions()? {
        if seen.contains(&session) {
            continue;
        }
        records.push(SessionRecord {
            session_name: session,
            kind: SessionKind::Project,
            target: None,
            host: None,
            remote_path: None,
            local_path: None,
            project_session: None,
            owner_pid: None,
            owner_alive: false,
            keep_session: false,
            mutagen_session: true,
            classification: SessionClassification::Orphaned,
            age_seconds: None,
        });
    }

    records.sort_by(|left, right| left.session_name.cmp(&right.session_name));
    Ok(records)
}

fn record_for_metadata(metadata: &SessionMetadata) -> Result<SessionRecord> {
    let mutagen_session = mutagen_session_exists(&metadata.session_name)?;
    let owner_alive = metadata.owner_pid.is_some_and(process_alive);
    let age_seconds =
        (metadata.created_at_unix > 0).then(|| unix_now().saturating_sub(metadata.created_at_unix));
    let classification =
        if metadata.kind == SessionKind::Hydration && (!owner_alive || !mutagen_session) {
            SessionClassification::StuckHydration
        } else if metadata.keep_session && mutagen_session {
            SessionClassification::Persistent
        } else if mutagen_session && owner_alive {
            SessionClassification::Active
        } else {
            SessionClassification::Suspicious
        };

    Ok(SessionRecord {
        session_name: metadata.session_name.clone(),
        kind: metadata.kind.clone(),
        target: Some(metadata.target.clone()),
        host: Some(metadata.host.clone()),
        remote_path: Some(metadata.remote_path.clone()),
        local_path: Some(metadata.local_path.clone()),
        project_session: metadata.project_session.clone(),
        owner_pid: metadata.owner_pid,
        owner_alive,
        keep_session: metadata.keep_session,
        mutagen_session,
        classification,
        age_seconds,
    })
}

fn mutagen_editr_sessions() -> Result<Vec<String>> {
    let output = Command::new("mutagen")
        .arg("sync")
        .arg("list")
        .arg("--template")
        .arg("{{range .}}{{.Name}}{{\"\\n\"}}{{end}}")
        .output()
        .context("failed to run mutagen sync list")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.starts_with("editr-") {
            sessions.push(line.to_string());
        }
    }
    Ok(sessions)
}

fn remove_if_exists(path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn remove_session_files(local_root: &Path, session: &str) -> Result<()> {
    let metadata_file = metadata_dir(local_root).join(format!("{session}.toml"));
    if metadata_file.exists()
        && let Ok(metadata) = load_metadata_file(&metadata_file)
        && let Some(context_file) = metadata.context_file
    {
        remove_if_exists(PathBuf::from(context_file))?;
    }
    remove_if_exists(metadata_dir(local_root).join(format!("{session}.context.json")))?;
    remove_if_exists(metadata_file)
}

fn directory_is_nonempty(path: &Path) -> Result<bool> {
    Ok(fs::read_dir(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .next()
        .transpose()?
        .is_some())
}

fn ensure_remote_directory(target: &Target, create: bool) -> Result<()> {
    let command = if create {
        format!("mkdir -p {}", shell_quote(&target.path))
    } else {
        format!("test -d {}", shell_quote(&target.path))
    };
    let description = if create {
        "create remote directory"
    } else {
        "check remote directory"
    };
    println!("{description}...");
    let status = ssh_probe_command(&target.host)
        .arg(command)
        .status()
        .with_context(|| format!("failed to start ssh for {}", target.host))?;
    if !status.success() {
        if create {
            bail!("failed to create remote directory: {}", target);
        }
        bail!(
            "remote directory does not exist: {}\ncreate it remotely first, or pass --create-remote intentionally",
            target
        );
    }
    println!("{description}: ok");
    Ok(())
}

fn ssh_probe_command(host: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(SSH_PROBE_OPTIONS).arg(host);
    command
}

fn bootstrap_from_remote(resolved: &ResolvedOpen) -> Result<()> {
    let bootstrap_name = format!("{}-bootstrap", resolved.session_name);
    run_mutagen_quiet(["sync", "terminate", bootstrap_name.as_str()]);
    println!("Refreshing local mirror from remote...");
    create_mutagen_session(
        &bootstrap_name,
        "one-way-replica",
        resolved.sync_vcs,
        &resolved.ignore_patterns,
        &mutagen_labels(
            &bootstrap_name,
            SessionKind::Bootstrap,
            Some(&resolved.session_name),
        ),
        &resolved.target.to_string(),
        resolved.local_path.to_string_lossy().as_ref(),
    )?;
    let flush_result = run_mutagen_checked(
        ["sync", "flush", bootstrap_name.as_str()],
        "mutagen sync flush bootstrap",
    );
    run_mutagen_quiet(["sync", "terminate", bootstrap_name.as_str()]);
    flush_result
}

fn reconfigure_session(resolved: &ResolvedOpen) -> Result<()> {
    println!("Session configuration changed; recreating Mutagen session.");
    run_mutagen_quiet(["sync", "resume", &resolved.session_name]);
    run_mutagen_checked(
        ["sync", "flush", &resolved.session_name],
        "mutagen sync flush before reconfigure",
    )?;
    run_mutagen_checked(
        ["sync", "terminate", &resolved.session_name],
        "mutagen sync terminate before reconfigure",
    )?;
    create_steady_session(resolved)?;
    write_metadata(resolved)
}

fn create_steady_session(resolved: &ResolvedOpen) -> Result<()> {
    create_mutagen_session(
        &resolved.session_name,
        &resolved.mode,
        resolved.sync_vcs,
        &resolved.ignore_patterns,
        &mutagen_labels(&resolved.session_name, SessionKind::Project, None),
        resolved.local_path.to_string_lossy().as_ref(),
        &resolved.target.to_string(),
    )
}

fn create_mutagen_session(
    name: &str,
    mode: &str,
    sync_vcs: bool,
    ignore_patterns: &[String],
    labels: &[(String, String)],
    alpha: &str,
    beta: &str,
) -> Result<()> {
    let mut command =
        mutagen_create_command(name, mode, sync_vcs, ignore_patterns, labels, alpha, beta);
    run_command_checked_with_output(&mut command, "mutagen sync create", false)
}

fn create_mutagen_session_quiet(
    name: &str,
    mode: &str,
    sync_vcs: bool,
    ignore_patterns: &[String],
    labels: &[(String, String)],
    alpha: &str,
    beta: &str,
) -> Result<()> {
    let mut command =
        mutagen_create_command(name, mode, sync_vcs, ignore_patterns, labels, alpha, beta);
    run_command_checked_with_output(&mut command, "mutagen sync create", true)
}

fn mutagen_create_command(
    name: &str,
    mode: &str,
    sync_vcs: bool,
    ignore_patterns: &[String],
    labels: &[(String, String)],
    alpha: &str,
    beta: &str,
) -> Command {
    let mut command = Command::new("mutagen");
    command
        .arg("sync")
        .arg("create")
        .arg("--name")
        .arg(name)
        .arg("--mode")
        .arg(mode);
    for (key, value) in labels {
        command.arg("--label").arg(format!("{key}={value}"));
    }
    if sync_vcs {
        command.arg("--no-ignore-vcs");
    } else {
        command.arg("--ignore-vcs");
    }
    for pattern in ignore_patterns {
        command.arg("-i").arg(pattern);
    }
    command.arg(alpha).arg(beta);
    command
}

fn perform_hydration(
    context: &EditrContext,
    remote_path: &str,
    relative_path: &str,
    local_path: &Path,
    session_name: &str,
    mode: &HydrationMode,
    runtime: HydrationRuntime,
) -> Result<()> {
    require_command("mutagen")?;
    require_command("ssh")?;
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let bootstrap_name = format!("{session_name}-bootstrap");
    run_mutagen_quiet(["sync", "terminate", bootstrap_name.as_str()]);
    let create_session = if runtime.quiet {
        create_mutagen_session_quiet
    } else {
        create_mutagen_session
    };
    create_session(
        &bootstrap_name,
        "one-way-replica",
        false,
        &[],
        &mutagen_labels(
            &bootstrap_name,
            SessionKind::Bootstrap,
            Some(&context.session_name),
        ),
        &format!("{}:{remote_path}", context.host),
        local_path.to_string_lossy().as_ref(),
    )?;
    let flush_result = run_mutagen_checked_with_output(
        ["sync", "flush", bootstrap_name.as_str()],
        "mutagen sync flush hydration bootstrap",
        runtime.quiet,
    );
    run_mutagen_quiet(["sync", "terminate", bootstrap_name.as_str()]);
    flush_result?;

    if *mode == HydrationMode::Live {
        if !mutagen_session_exists(session_name)? {
            create_session(
                session_name,
                "two-way-safe",
                false,
                &[],
                &mutagen_labels(
                    session_name,
                    SessionKind::Hydration,
                    Some(&context.session_name),
                ),
                local_path.to_string_lossy().as_ref(),
                &format!("{}:{remote_path}", context.host),
            )?;
        }
        write_hydration_metadata(
            context,
            remote_path,
            relative_path,
            local_path,
            session_name,
            runtime.owner_pid,
        )?;
        run_mutagen_checked_with_output(
            ["sync", "flush", session_name],
            "mutagen sync flush hydration session",
            runtime.quiet,
        )?;
    }
    Ok(())
}

fn write_hydration_metadata(
    context: &EditrContext,
    remote_path: &str,
    relative_path: &str,
    local_path: &Path,
    session_name: &str,
    owner_pid: u32,
) -> Result<()> {
    let metadata_dir = PathBuf::from(&context.metadata_dir);
    fs::create_dir_all(&metadata_dir)
        .with_context(|| format!("failed to create {}", metadata_dir.display()))?;
    let now = unix_now();
    let metadata = SessionMetadata {
        schema_version: CONTEXT_SCHEMA_VERSION,
        kind: SessionKind::Hydration,
        session_name: session_name.to_string(),
        target: context.target.clone(),
        host: context.host.clone(),
        remote_path: remote_path.to_string(),
        local_path: local_path.display().to_string(),
        mode: "two-way-safe".to_string(),
        sync_vcs: false,
        keep_session: false,
        project_session: Some(context.session_name.clone()),
        relative_path: Some(relative_path.to_string()),
        owner_pid: Some(owner_pid),
        created_at_unix: now,
        updated_at_unix: now,
        context_file: Some(context.context_file.clone()),
        ignore_patterns: Vec::new(),
    };
    let path = metadata_dir.join(format!("{session_name}.toml"));
    fs::write(&path, toml::to_string_pretty(&metadata)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn existing_hydration_matches(
    context: &EditrContext,
    session_name: &str,
    remote_path: &str,
    local_path: &Path,
) -> Result<bool> {
    let metadata_file = PathBuf::from(&context.metadata_dir).join(format!("{session_name}.toml"));
    if !metadata_file.exists() || !mutagen_session_exists(session_name)? {
        return Ok(false);
    }
    let metadata = load_metadata_file(&metadata_file)?;
    Ok(metadata.kind == SessionKind::Hydration
        && metadata.remote_path == remote_path
        && metadata.local_path == local_path.display().to_string())
}

fn remote_file_size(host: &str, remote_path: &str) -> Result<Option<u64>> {
    let quoted = shell_quote(remote_path);
    let command = format!(
        "p={quoted}; if [ -d \"$p\" ]; then printf 'directory\\n'; exit 3; fi; \
         if [ ! -e \"$p\" ]; then printf 'missing\\n'; exit 2; fi; \
         printf 'file\\t'; (stat -c %s \"$p\" 2>/dev/null || stat -f %z \"$p\" 2>/dev/null || wc -c < \"$p\")"
    );
    let output = ssh_probe_command(host)
        .arg(command)
        .output()
        .with_context(|| format!("failed to stat remote file on {host}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let message = stdout.trim();
        if message == "directory" {
            bail!("remote path is a directory: {remote_path}");
        }
        if message == "missing" {
            bail!("remote path does not exist: {remote_path}");
        }
        bail!("failed to stat remote path: {remote_path}");
    }
    let Some(size) = stdout.trim().strip_prefix("file\t") else {
        return Ok(None);
    };
    Ok(size.trim().parse::<u64>().ok())
}

fn remote_physical_path(host: &str, remote_path: &str) -> Result<Option<String>> {
    let quoted = shell_quote(remote_path);
    let command = format!("cd {quoted} 2>/dev/null && pwd -P");
    let output = ssh_probe_command(host)
        .arg(command)
        .output()
        .with_context(|| format!("failed to resolve remote path on {host}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(path))
    }
}

fn write_metadata(resolved: &ResolvedOpen) -> Result<()> {
    let now = unix_now();
    let metadata = SessionMetadata {
        schema_version: CONTEXT_SCHEMA_VERSION,
        kind: SessionKind::Project,
        session_name: resolved.session_name.clone(),
        target: resolved.target.to_string(),
        host: resolved.target.host.clone(),
        remote_path: resolved.target.path.clone(),
        local_path: resolved.local_path.display().to_string(),
        mode: resolved.mode.clone(),
        sync_vcs: resolved.sync_vcs,
        keep_session: resolved.keep_session,
        project_session: None,
        relative_path: None,
        owner_pid: Some(std::process::id()),
        created_at_unix: now,
        updated_at_unix: now,
        context_file: Some(resolved.context_file.display().to_string()),
        ignore_patterns: resolved.ignore_patterns.clone(),
    };
    let contents = toml::to_string_pretty(&metadata)?;
    fs::write(&resolved.metadata_file, contents)
        .with_context(|| format!("failed to write {}", resolved.metadata_file.display()))
}

fn editr_context(resolved: &ResolvedOpen) -> Result<EditrContext> {
    Ok(EditrContext {
        schema_version: CONTEXT_SCHEMA_VERSION,
        session_name: resolved.session_name.clone(),
        target: resolved.target.to_string(),
        host: resolved.target.host.clone(),
        remote_path: resolved.target.path.clone(),
        local_root: resolved.local_root.display().to_string(),
        local_path: resolved.local_path.display().to_string(),
        metadata_dir: metadata_dir(&resolved.local_root).display().to_string(),
        context_file: resolved.context_file.display().to_string(),
        editr_bin: current_executable(),
        ignore_patterns: resolved.ignore_patterns.clone(),
        remote_path_aliases: remote_path_aliases(&resolved.target),
    })
}

fn remote_path_aliases(target: &Target) -> Vec<String> {
    let Ok(Some(path)) = remote_physical_path(&target.host, &target.path) else {
        return Vec::new();
    };
    let path = normalize_remote_path(&path);
    if path == normalize_remote_path(&target.path) {
        Vec::new()
    } else {
        vec![path]
    }
}

fn write_context(resolved: &ResolvedOpen) -> Result<EditrContext> {
    let context = editr_context(resolved)?;
    let contents = serde_json::to_string_pretty(&context)?;
    fs::write(&resolved.context_file, contents)
        .with_context(|| format!("failed to write {}", resolved.context_file.display()))?;
    Ok(context)
}

fn install_signal_cleanup(resolved: &ResolvedOpen) -> Result<()> {
    let session_name = resolved.session_name.clone();
    ctrlc::set_handler(move || {
        eprintln!("\nSignal received; terminating Mutagen session {session_name}.");
        let _ = Command::new("mutagen")
            .arg("sync")
            .arg("terminate")
            .arg(&session_name)
            .status();
        std::process::exit(130);
    })
    .context("failed to install signal cleanup handler")
}

fn cleanup_session_after_editor(resolved: &ResolvedOpen) -> Result<()> {
    println!("Flushing Mutagen session before shutdown...");
    let flush_result = run_mutagen_checked(
        ["sync", "flush", &resolved.session_name],
        "mutagen sync flush after editor exit",
    );
    println!("Terminating Mutagen session...");
    let terminate_result = run_mutagen_checked(
        ["sync", "terminate", &resolved.session_name],
        "mutagen sync terminate after editor exit",
    );

    match (flush_result, terminate_result) {
        (Ok(()), Ok(())) => {
            remove_if_exists(resolved.metadata_file.clone())?;
            remove_if_exists(resolved.context_file.clone())?;
            Ok(())
        }
        (Err(flush_error), Ok(())) => {
            bail!(
                "final Mutagen flush failed, so the session was terminated and the local mirror was retained for recovery: {flush_error:#}"
            );
        }
        (Ok(()), Err(terminate_error)) => {
            bail!(
                "final Mutagen flush succeeded, but terminating the session failed: {terminate_error:#}"
            );
        }
        (Err(flush_error), Err(terminate_error)) => {
            bail!(
                "final Mutagen flush failed and terminating the session also failed: {flush_error:#}; {terminate_error:#}"
            );
        }
    }
}

fn open_editor(editor: &str, local_path: &Path, context: &EditrContext) -> Result<()> {
    let command = editor_shell_command(editor, ".");
    let mut process = Command::new("sh");
    process
        .arg("-c")
        .arg(&command)
        .current_dir(local_path)
        .env("EDITR", "1")
        .env("EDITR_CONTEXT", &context.context_file)
        .env("EDITR_SESSION", &context.session_name)
        .env("EDITR_REMOTE_TARGET", &context.target)
        .env("EDITR_REMOTE_HOST", &context.host)
        .env("EDITR_REMOTE_PATH", &context.remote_path)
        .env("EDITR_LOCAL_PATH", &context.local_path)
        .env("EDITR_BIN", &context.editr_bin);
    let status = process
        .status()
        .with_context(|| format!("failed to start editor command: {command}"))?;
    if !status.success() {
        bail!("editor exited with status {status}");
    }
    Ok(())
}

fn editor_shell_command(editor: &str, path_arg: &str) -> String {
    let editor = editor.trim();
    let path_arg = shell_quote(path_arg);
    if editor.contains("{path}") {
        editor.replace("{path}", &path_arg)
    } else {
        format!("{editor} {path_arg}")
    }
}

fn mutagen_session_exists(name: &str) -> Result<bool> {
    let output = Command::new("mutagen")
        .arg("sync")
        .arg("list")
        .arg(name)
        .output()
        .context("failed to run mutagen sync list")?;
    if !output.status.success() {
        return Ok(false);
    }
    let needle = format!("Name: {name}");
    Ok(String::from_utf8_lossy(&output.stdout).contains(&needle)
        || String::from_utf8_lossy(&output.stderr).contains(&needle))
}

fn terminate_mutagen_session_if_present(session: &str) -> Result<()> {
    if mutagen_session_exists(session)? {
        run_mutagen_checked(["sync", "terminate", session], "mutagen sync terminate")?;
    }
    Ok(())
}

fn run_mutagen_checked<const N: usize>(args: [&str; N], description: &str) -> Result<()> {
    run_mutagen_checked_with_output(args, description, false)
}

fn run_mutagen_checked_with_output<const N: usize>(
    args: [&str; N],
    description: &str,
    quiet: bool,
) -> Result<()> {
    let mut command = Command::new("mutagen");
    command.args(args);
    run_command_checked_with_output(&mut command, description, quiet)
}

fn run_mutagen_quiet<const N: usize>(args: [&str; N]) {
    let _ = Command::new("mutagen")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn run_command_checked_with_output(
    command: &mut Command,
    description: &str,
    quiet: bool,
) -> Result<()> {
    if quiet {
        let output = command
            .output()
            .with_context(|| format!("failed to start {description}"))?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let details = [stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if details.is_empty() {
                bail!("{description} failed with status {}", output.status);
            }
            bail!(
                "{description} failed with status {}:\n{}",
                output.status,
                details
            );
        }
        return Ok(());
    }

    let status = command
        .status()
        .with_context(|| format!("failed to start {description}"))?;
    if !status.success() {
        bail!("{description} failed with status {status}");
    }
    Ok(())
}

fn require_command(name: &str) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", shell_quote(name)))
        .status()
        .with_context(|| format!("failed to check for {name}"))?;
    if status.success() {
        return Ok(());
    }
    bail!("{name} is required but was not found on PATH");
}

fn load_context(path: &Path) -> Result<EditrContext> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("invalid context JSON: {}", path.display()))
}

fn relative_remote_path_with_aliases(
    remote_root: &str,
    remote_root_aliases: &[String],
    remote_path: &str,
) -> Result<String> {
    for root in std::iter::once(remote_root).chain(remote_root_aliases.iter().map(String::as_str)) {
        if let Some(relative) = relative_remote_path_for_root(root, remote_path)? {
            return Ok(relative);
        }
    }
    let remote_root = normalize_remote_path(remote_root);
    let remote_path = normalize_remote_path(remote_path);
    let aliases = if remote_root_aliases.is_empty() {
        String::new()
    } else {
        format!("\n  aliases: {}", remote_root_aliases.join(", "))
    };
    bail!(
        "remote path is outside editr root:\n  root: {}\n  path: {}{}",
        remote_root,
        remote_path,
        aliases
    );
}

fn relative_remote_path_for_root(remote_root: &str, remote_path: &str) -> Result<Option<String>> {
    let remote_root = normalize_remote_path(remote_root);
    let remote_path = normalize_remote_path(remote_path);
    if remote_path == remote_root {
        bail!("cannot hydrate the remote root as a file");
    }
    let prefix = format!("{remote_root}/");
    let Some(relative) = remote_path.strip_prefix(&prefix) else {
        return Ok(None);
    };
    if relative.is_empty() || relative.split('/').any(|component| component == "..") {
        bail!("invalid relative hydration path: {relative}");
    }
    Ok(Some(relative.to_string()))
}

fn hydration_session_name(project_session: &str, remote_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_session);
    hasher.update("\0");
    hasher.update(remote_path);
    let digest = hasher.finalize();
    let project = truncate_component(&sanitize_component(project_session), 40);
    format!("editr-hydrate-{project}-{}", hex_prefix(&digest, 12))
}

fn mutagen_labels(
    session_name: &str,
    kind: SessionKind,
    project_session: Option<&str>,
) -> Vec<(String, String)> {
    let mut labels = vec![
        ("editr".to_string(), "true".to_string()),
        (
            "editr.session".to_string(),
            mutagen_label_value(session_name),
        ),
        (
            "editr.kind".to_string(),
            session_kind_label(&kind).to_string(),
        ),
    ];
    if let Some(project_session) = project_session {
        labels.push((
            "editr.project".to_string(),
            mutagen_label_value(project_session),
        ));
    }
    labels
}

fn mutagen_label_value(value: &str) -> String {
    const MAX_LABEL_VALUE_LEN: usize = 63;
    let sanitized = sanitize_component(value);
    if sanitized.len() <= MAX_LABEL_VALUE_LEN {
        return sanitized;
    }

    let mut hasher = Sha256::new();
    hasher.update(value);
    let digest = hasher.finalize();
    let hash = hex_prefix(&digest, 12);
    let prefix = truncate_component(&sanitized, MAX_LABEL_VALUE_LEN - hash.len() - 1);
    format!("{prefix}-{hash}")
}

fn session_kind_label(kind: &SessionKind) -> &'static str {
    match kind {
        SessionKind::Project => "project",
        SessionKind::Bootstrap => "bootstrap",
        SessionKind::Hydration => "hydration",
    }
}

fn parse_size(input: &str) -> Result<u64> {
    let input = input.trim();
    if input.is_empty() {
        bail!("size cannot be empty");
    }
    let mut number = String::new();
    let mut unit = String::new();
    for character in input.chars() {
        if character.is_ascii_digit() || character == '.' {
            if !unit.trim().is_empty() {
                bail!("invalid size: {input}");
            }
            number.push(character);
        } else if !character.is_whitespace() {
            unit.push(character);
        }
    }
    let value = number
        .parse::<f64>()
        .with_context(|| format!("invalid size: {input}"))?;
    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1_000.0,
        "m" | "mb" => 1_000_000.0,
        "g" | "gb" => 1_000_000_000.0,
        "t" | "tb" => 1_000_000_000_000.0,
        "ki" | "kib" => 1024.0,
        "mi" | "mib" => 1024.0 * 1024.0,
        "gi" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "ti" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => bail!("unknown size unit: {unit}"),
    };
    Ok((value * multiplier).round() as u64)
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[(&str, f64)] = &[
        ("TB", 1_000_000_000_000.0),
        ("GB", 1_000_000_000.0),
        ("MB", 1_000_000.0),
        ("KB", 1_000.0),
    ];
    for (unit, size) in UNITS {
        if bytes as f64 >= *size {
            return format!("{:.1} {unit}", bytes as f64 / size);
        }
    }
    format!("{bytes} B")
}

fn parse_duration(input: &str) -> Result<Duration> {
    let input = input.trim();
    if input.is_empty() {
        bail!("duration cannot be empty");
    }
    let mut number = String::new();
    let mut unit = String::new();
    for character in input.chars() {
        if character.is_ascii_digit() || character == '.' {
            if !unit.trim().is_empty() {
                bail!("invalid duration: {input}");
            }
            number.push(character);
        } else if !character.is_whitespace() {
            unit.push(character);
        }
    }
    let value = number
        .parse::<f64>()
        .with_context(|| format!("invalid duration: {input}"))?;
    let seconds = match unit.to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
        "m" | "min" | "mins" | "minute" | "minutes" => value * 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => value * 60.0 * 60.0,
        _ => bail!("unknown duration unit: {unit}"),
    };
    Ok(Duration::from_secs_f64(seconds))
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|status| status.success())
}

fn notify_user(title: &str, message: &str) {
    let apple_script = format!(
        "display notification \"{}\" with title \"{}\"",
        applescript_escape(message),
        applescript_escape(title)
    );
    if Command::new("osascript")
        .arg("-e")
        .arg(&apple_script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    if Command::new("notify-send")
        .arg(title)
        .arg(message)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    eprintln!("{title}: {message}");
}

fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn current_executable() -> String {
    env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
        .unwrap_or_else(|| "editr".to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn default_schema_version() -> u32 {
    CONTEXT_SCHEMA_VERSION
}

fn truncate_component(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut output = String::with_capacity(value.len() + 2);
    output.push('\'');
    for character in value.chars() {
        if character == '\'' {
            output.push_str("'\\''");
        } else {
            output.push(character);
        }
    }
    output.push('\'');
    output
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_index, mut text_index) = (0, 0);
    let mut star_index = None;
    let mut star_text_index = 0;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            star_text_index = text_index;
            pattern_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_text_index += 1;
            text_index = star_text_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targets() {
        let target = Target::parse("user@host:/a/b/").unwrap();
        assert_eq!(target.host, "user@host");
        assert_eq!(target.path, "/a/b");
        assert_eq!(target.to_string(), "user@host:/a/b");
    }

    #[test]
    fn rejects_non_absolute_targets() {
        assert!(Target::parse("host:relative").is_err());
        assert!(Target::parse("/local/path").is_err());
    }

    #[test]
    fn builds_stable_session_names() {
        let target = Target::parse("host:/a/b").unwrap();
        assert_eq!(
            session_name_for_target(&target),
            session_name_for_target(&target)
        );
        assert!(session_name_for_target(&target).starts_with("editr-host-"));
    }

    #[test]
    fn wildcard_patterns_match_targets() {
        assert!(wildcard_match(
            "cluster:/home/user/project*",
            "cluster:/home/user/project/runs"
        ));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("host:/a/?", "host:/a/b"));
        assert!(!wildcard_match("host:/a/?", "host:/a/bc"));
    }

    #[test]
    fn merges_ignore_config() {
        let mut ignore = BTreeMap::new();
        ignore.insert("*".to_string(), vec!["node_modules/".to_string()]);
        ignore.insert("host:/project*".to_string(), vec!["target/".to_string()]);
        let config = Config {
            ignore: Some(ignore),
            ..Default::default()
        };
        let patterns = config_ignore_patterns(&config, "host:/project/src").unwrap();
        assert_eq!(patterns, vec!["node_modules/", "target/"]);
    }

    #[test]
    fn detects_session_reconfiguration_changes() {
        let target = Target::parse("host:/project").unwrap();
        let resolved = ResolvedOpen {
            target: target.clone(),
            local_root: PathBuf::from("/tmp"),
            local_path: PathBuf::from("/tmp/project"),
            session_name: "editr-host-test".to_string(),
            metadata_file: PathBuf::from("/tmp/project.toml"),
            context_file: PathBuf::from("/tmp/project.context.json"),
            editor: "nvim".to_string(),
            mode: "two-way-safe".to_string(),
            sync_vcs: true,
            keep_session: false,
            ignore_patterns: vec!["node_modules/".to_string()],
            bootstrap: true,
            create_remote: false,
            allow_nonempty: false,
            dry_run: false,
        };
        let mut metadata = SessionMetadata {
            schema_version: CONTEXT_SCHEMA_VERSION,
            kind: SessionKind::Project,
            session_name: resolved.session_name.clone(),
            target: target.to_string(),
            host: target.host,
            remote_path: target.path,
            local_path: resolved.local_path.display().to_string(),
            mode: resolved.mode.clone(),
            sync_vcs: resolved.sync_vcs,
            keep_session: resolved.keep_session,
            project_session: None,
            relative_path: None,
            owner_pid: None,
            created_at_unix: 0,
            updated_at_unix: 0,
            context_file: None,
            ignore_patterns: resolved.ignore_patterns.clone(),
        };

        assert!(!session_needs_reconfigure(&metadata, &resolved));
        metadata.ignore_patterns = vec!["data/".to_string()];
        assert!(session_needs_reconfigure(&metadata, &resolved));
    }

    #[test]
    fn appends_path_to_editor_shell_command() {
        assert_eq!(
            editor_shell_command("nvim --clean", "."),
            "nvim --clean '.'"
        );
    }

    #[test]
    fn substitutes_path_placeholder_in_editor_shell_command() {
        assert_eq!(
            editor_shell_command("nvim --cmd 'set number' {path}", "."),
            "nvim --cmd 'set number' '.'"
        );
    }

    #[test]
    fn shell_quotes_editor_path_argument() {
        assert_eq!(
            editor_shell_command("nvim", "dir/it's here"),
            "nvim 'dir/it'\\''s here'"
        );
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("25 MB").unwrap(), 25_000_000);
        assert_eq!(parse_size("1.5MiB").unwrap(), 1_572_864);
        assert!(parse_size("12 parsecs").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("60s").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert!(parse_duration("1 fortnight").is_err());
    }

    #[test]
    fn maps_relative_remote_paths() {
        assert_eq!(
            relative_remote_path_with_aliases("/repo", &[], "/repo/logs/run.txt").unwrap(),
            "logs/run.txt"
        );
        assert!(relative_remote_path_with_aliases("/repo", &[], "/other/run.txt").is_err());
        assert!(relative_remote_path_with_aliases("/repo", &[], "/repo").is_err());
    }

    #[test]
    fn maps_relative_remote_paths_through_aliases() {
        let aliases = vec!["/mnt/storage/user/project".to_string()];
        assert_eq!(
            relative_remote_path_with_aliases(
                "/home/user/project",
                &aliases,
                "/mnt/storage/user/project/README.md",
            )
            .unwrap(),
            "README.md"
        );
    }

    #[test]
    fn builds_hydration_session_names() {
        let left = hydration_session_name("editr-host-project", "/repo/a.txt");
        let right = hydration_session_name("editr-host-project", "/repo/a.txt");
        assert_eq!(left, right);
        assert!(left.starts_with("editr-hydrate-editr-host-project-"));
    }

    #[test]
    fn caps_mutagen_label_values() {
        let long = "editr-hydrate-editr-very-long-host-name-with-a-very-long-project-name-and-path-0123456789abcdef-bootstrap";
        let value = mutagen_label_value(long);
        assert!(value.len() <= 63);
        assert!(value.starts_with("editr-hydrate-editr-very-long-host-name"));
    }
}
