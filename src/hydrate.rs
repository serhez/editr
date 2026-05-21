use crate::cli::HydrateCommand;
use crate::config::load_config;
use crate::constants::{CONTEXT_SCHEMA_VERSION, DEFAULT_MAX_AUTO_HYDRATE_SIZE};
use crate::context::{load_context, relative_remote_path_with_aliases};
use crate::model::{
    EditrContext, HydrateResponse, HydrationMode, HydrationRuntime, SessionKind, SessionMetadata,
};
use crate::mutagen::{
    create_mutagen_session, create_mutagen_session_quiet, mutagen_labels, mutagen_session_exists,
    run_mutagen_checked_with_output, run_mutagen_quiet,
};
use crate::process::require_command;
use crate::session::load_metadata_file;
use crate::ssh::remote_file_size;
use crate::target::normalize_remote_path;
use crate::util::{
    format_size, hex_prefix, parse_size, sanitize_component, truncate_component, unix_now,
};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn hydrate(command: HydrateCommand) -> Result<()> {
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

pub(crate) fn hydration_session_name(project_session: &str, remote_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_session);
    hasher.update("\0");
    hasher.update(remote_path);
    let digest = hasher.finalize();
    let project = truncate_component(&sanitize_component(project_session), 40);
    format!("editr-hydrate-{project}-{}", hex_prefix(&digest, 12))
}
