use crate::constants::CONTEXT_SCHEMA_VERSION;
use crate::model::{EditrContext, ResolvedOpen, Target};
use crate::ssh::remote_physical_path;
use crate::target::{metadata_dir, normalize_remote_path};
use crate::util::current_executable;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

pub(crate) fn load_context(path: &Path) -> Result<EditrContext> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("invalid context JSON: {}", path.display()))
}

pub(crate) fn editr_context(resolved: &ResolvedOpen) -> Result<EditrContext> {
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

pub(crate) fn write_context(resolved: &ResolvedOpen) -> Result<EditrContext> {
    let context = editr_context(resolved)?;
    let contents = serde_json::to_string_pretty(&context)?;
    fs::write(&resolved.context_file, contents)
        .with_context(|| format!("failed to write {}", resolved.context_file.display()))?;
    Ok(context)
}

pub(crate) fn relative_remote_path_with_aliases(
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
