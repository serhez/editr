use crate::cli::ManagerOptions;
use crate::config::{config_local_root, load_config};
use crate::model::{
    MetadataState, ResolvedOpen, SessionClassification, SessionKind, SessionMetadata, SessionRecord,
};
use crate::mutagen::mutagen_session_exists;
use crate::target::metadata_dir;
use crate::util::{process_alive, unix_now};
use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn metadata_state(resolved: &ResolvedOpen) -> Result<MetadataState> {
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

pub(crate) fn session_needs_reconfigure(
    metadata: &SessionMetadata,
    resolved: &ResolvedOpen,
) -> bool {
    metadata.target != resolved.target.to_string()
        || metadata.local_path != resolved.local_path.display().to_string()
        || metadata.mode != resolved.mode
        || metadata.sync_vcs != resolved.sync_vcs
        || metadata.ignore_patterns != resolved.ignore_patterns
}

pub(crate) fn manager_local_root(options: &ManagerOptions) -> Result<PathBuf> {
    let config = load_config(options.config.as_deref())?;
    let local_root = options
        .local_root
        .clone()
        .unwrap_or_else(|| config_local_root(&config));
    Ok(crate::util::expand_path(
        local_root.to_string_lossy().as_ref(),
    ))
}

pub(crate) fn tracked_sessions(options: &ManagerOptions) -> Result<Vec<String>> {
    metadata_sessions(&manager_local_root(options)?)
}

pub(crate) fn metadata_sessions(local_root: &Path) -> Result<Vec<String>> {
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

pub(crate) fn load_metadata_file(path: &Path) -> Result<SessionMetadata> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("invalid metadata TOML: {}", path.display()))
}

pub(crate) fn session_records(options: &ManagerOptions) -> Result<Vec<SessionRecord>> {
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

pub(crate) fn remove_if_exists(path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

pub(crate) fn remove_session_files(local_root: &Path, session: &str) -> Result<()> {
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

pub(crate) fn directory_is_nonempty(path: &Path) -> Result<bool> {
    Ok(fs::read_dir(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .next()
        .transpose()?
        .is_some())
}

pub(crate) fn write_metadata(resolved: &ResolvedOpen) -> Result<()> {
    let now = unix_now();
    let metadata = SessionMetadata {
        schema_version: crate::constants::CONTEXT_SCHEMA_VERSION,
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
