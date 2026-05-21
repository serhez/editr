use crate::model::HydrationMode;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "editr",
    version,
    about = "Open remote projects in local editor through a Mutagen-backed mirror",
    long_about = None,
    arg_required_else_help = true
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,

    #[arg(
        value_name = "TARGET",
        help = "Remote target, e.g. host:/absolute/path"
    )]
    pub(crate) target: Option<String>,

    #[command(flatten)]
    pub(crate) open_options: OpenOptions,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
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
pub(crate) struct OpenCommand {
    #[arg(
        value_name = "TARGET",
        help = "Remote target, e.g. host:/absolute/path"
    )]
    pub(crate) target: String,

    #[command(flatten)]
    pub(crate) options: OpenOptions,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct OpenOptions {
    /// Local mirror root.
    #[arg(long, short = 'L', value_name = "PATH")]
    pub(crate) local_root: Option<PathBuf>,

    /// Exact local mirror path. Advanced; normally derived from --local-root and TARGET.
    #[arg(long, value_name = "PATH")]
    pub(crate) local_path: Option<PathBuf>,

    /// Exact Mutagen session name. Advanced; normally derived from TARGET.
    #[arg(long, value_name = "NAME")]
    pub(crate) session_name: Option<String>,

    /// Config file path.
    #[arg(long, short = 'C', value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,

    /// Extra Mutagen ignore pattern. Can be passed multiple times.
    #[arg(long, short = 'i', value_name = "PATTERN")]
    pub(crate) ignore: Vec<String>,

    /// Mutagen mode for the steady-state session.
    #[arg(long, short = 'm', value_name = "MODE")]
    pub(crate) mode: Option<String>,

    /// Editor shell command to launch from the local mirror.
    #[arg(long, short = 'e', value_name = "COMMAND")]
    pub(crate) editor: Option<String>,

    /// Skip the initial remote-to-local refresh.
    #[arg(long)]
    pub(crate) no_bootstrap: bool,

    /// Create the remote directory if it does not exist.
    #[arg(long)]
    pub(crate) create_remote: bool,

    /// Allow starting/resuming when the local mirror is non-empty and unmarked.
    #[arg(long)]
    pub(crate) allow_nonempty: bool,

    /// Sync the remote .git directory and other VCS metadata.
    #[arg(long, conflicts_with = "ignore_vcs")]
    pub(crate) sync_vcs: bool,

    /// Ask Mutagen to ignore VCS metadata.
    #[arg(long)]
    pub(crate) ignore_vcs: bool,

    /// Keep the Mutagen session running after the editor exits.
    #[arg(long)]
    pub(crate) keep_session: bool,

    /// Print the resolved plan without creating sessions or opening the editor.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CapabilitiesCommand {
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct HydrateCommand {
    /// editr context JSON file, usually from EDITR_CONTEXT.
    #[arg(long, value_name = "PATH")]
    pub(crate) context: PathBuf,

    /// Absolute remote file path to hydrate.
    #[arg(long, value_name = "PATH")]
    pub(crate) remote_path: String,

    /// Hydration mode.
    #[arg(long, value_enum)]
    pub(crate) mode: Option<HydrationMode>,

    /// Maximum file size to hydrate without --allow-large.
    #[arg(long, value_name = "SIZE")]
    pub(crate) max_size: Option<String>,

    /// Report remote size and mapping without creating a Mutagen session.
    #[arg(long)]
    pub(crate) check: bool,

    /// Hydrate even when the remote file exceeds --max-size.
    #[arg(long)]
    pub(crate) allow_large: bool,

    /// Allow hydrating over an existing local file.
    #[arg(long)]
    pub(crate) allow_existing: bool,

    /// Process id that owns a live hydration session.
    #[arg(long, value_name = "PID")]
    pub(crate) owner_pid: Option<u32>,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ListCommand {
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,

    #[command(flatten)]
    pub(crate) manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
pub(crate) struct SessionCommand {
    #[arg(value_name = "TARGET_OR_SESSION")]
    pub(crate) selector: Option<String>,

    /// Apply to every session recorded under editr's metadata directory.
    #[arg(long)]
    pub(crate) all: bool,

    #[command(flatten)]
    pub(crate) manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
pub(crate) struct StopCommand {
    #[arg(value_name = "TARGET_OR_SESSION")]
    pub(crate) selector: Option<String>,

    /// Stop every session recorded under editr's metadata directory.
    #[arg(long)]
    pub(crate) all: bool,

    #[command(flatten)]
    pub(crate) manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
pub(crate) struct WatchCommand {
    /// Polling interval, for example 60s or 2m.
    #[arg(long, value_name = "DURATION")]
    pub(crate) interval: Option<String>,

    /// Enable OS notifications for suspicious sessions.
    #[arg(long, conflicts_with = "no_notify")]
    pub(crate) notify: bool,

    /// Disable OS notifications.
    #[arg(long)]
    pub(crate) no_notify: bool,

    /// Run one scan and exit.
    #[arg(long)]
    pub(crate) once: bool,

    /// Emit machine-readable JSON for each scan.
    #[arg(long)]
    pub(crate) json: bool,

    /// Auto-stop hydration sessions older than this duration.
    #[arg(long, value_name = "DURATION")]
    pub(crate) auto_stop_hydration_after: Option<String>,

    #[command(flatten)]
    pub(crate) manager_options: ManagerOptions,
}

#[derive(Debug, Args)]
pub(crate) struct ManagerOptions {
    /// Local mirror root. Used to find editr session metadata for --all.
    #[arg(long, short = 'L', value_name = "PATH")]
    pub(crate) local_root: Option<PathBuf>,

    /// Config file path.
    #[arg(long, short = 'C', value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
}

impl OpenOptions {
    pub(crate) fn merge(&mut self, other: OpenOptions) {
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
