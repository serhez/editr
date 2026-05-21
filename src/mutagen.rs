use crate::model::SessionKind;
use crate::process::run_command_checked_with_output;
use crate::util::{hex_prefix, sanitize_component, truncate_component};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::process::{Command, Stdio};

pub(crate) fn create_mutagen_session(
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

pub(crate) fn create_mutagen_session_quiet(
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

pub(crate) fn mutagen_session_exists(name: &str) -> Result<bool> {
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

pub(crate) fn terminate_mutagen_session_if_present(session: &str) -> Result<()> {
    if mutagen_session_exists(session)? {
        run_mutagen_checked(["sync", "terminate", session], "mutagen sync terminate")?;
    }
    Ok(())
}

pub(crate) fn run_mutagen_checked<const N: usize>(
    args: [&str; N],
    description: &str,
) -> Result<()> {
    run_mutagen_checked_with_output(args, description, false)
}

pub(crate) fn run_mutagen_checked_with_output<const N: usize>(
    args: [&str; N],
    description: &str,
    quiet: bool,
) -> Result<()> {
    let mut command = Command::new("mutagen");
    command.args(args);
    run_command_checked_with_output(&mut command, description, quiet)
}

pub(crate) fn run_mutagen_quiet<const N: usize>(args: [&str; N]) {
    let _ = Command::new("mutagen")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub(crate) fn mutagen_labels(
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

pub(crate) fn mutagen_label_value(value: &str) -> String {
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
