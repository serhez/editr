use crate::cli::OpenOptions;
use crate::config::{config_ignore_patterns, config_local_root, dedupe_patterns, load_config};
use crate::constants::{DEFAULT_EDITOR, DEFAULT_MODE};
use crate::context::write_context;
use crate::editor::{cleanup_session_after_editor, install_signal_cleanup, open_editor};
use crate::model::{MetadataState, ResolvedOpen, SessionKind, Target};
use crate::mutagen::{
    create_mutagen_session, mutagen_labels, mutagen_session_exists, run_mutagen_checked,
    run_mutagen_quiet,
};
use crate::process::require_command;
use crate::session::{
    directory_is_nonempty, metadata_state, session_needs_reconfigure, write_metadata,
};
use crate::ssh::ensure_remote_directory;
use crate::target::{local_path_for_target, metadata_dir, session_name_for_target};
use crate::util::expand_path;
use anyhow::{Context, Result, bail};
use std::fs;

pub(crate) fn open_target(target: &str, options: &OpenOptions) -> Result<()> {
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

pub(crate) fn resolve_open(target: &str, options: &OpenOptions) -> Result<ResolvedOpen> {
    let target = Target::parse(target)?;
    let config = load_config(options.config.as_deref())?;
    let local_root = options
        .local_root
        .clone()
        .unwrap_or_else(|| config_local_root(&config));
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
