use crate::model::{EditrContext, ResolvedOpen};
use crate::mutagen::run_mutagen_checked;
use crate::session::remove_if_exists;
use crate::util::shell_quote;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

pub(crate) fn install_signal_cleanup(resolved: &ResolvedOpen) -> Result<()> {
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

pub(crate) fn cleanup_session_after_editor(resolved: &ResolvedOpen) -> Result<()> {
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

pub(crate) fn open_editor(editor: &str, local_path: &Path, context: &EditrContext) -> Result<()> {
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

pub(crate) fn editor_shell_command(editor: &str, path_arg: &str) -> String {
    let editor = editor.trim();
    let path_arg = shell_quote(path_arg);
    if editor.contains("{path}") {
        editor.replace("{path}", &path_arg)
    } else {
        format!("{editor} {path_arg}")
    }
}
