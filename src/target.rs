use crate::model::Target;
use crate::util::{hex_prefix, sanitize_component};
use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

impl Target {
    pub(crate) fn parse(input: &str) -> Result<Self> {
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

pub(crate) fn normalize_remote_path(path: &str) -> String {
    let mut path = path.to_string();
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

pub(crate) fn local_path_for_target(local_root: &Path, target: &Target) -> PathBuf {
    let mut path = local_root.join(sanitize_component(&target.host));
    for component in target.path.trim_start_matches('/').split('/') {
        if !component.is_empty() {
            path.push(component);
        }
    }
    path
}

pub(crate) fn metadata_dir(local_root: &Path) -> PathBuf {
    local_root.join(".editr-sessions")
}

pub(crate) fn session_name_for_target(target: &Target) -> String {
    let mut hasher = Sha256::new();
    hasher.update(target.to_string());
    let digest = hasher.finalize();
    let hash = hex_prefix(&digest, 12);
    format!("editr-{}-{hash}", sanitize_component(&target.host))
}

pub(crate) fn selector_to_session(selector: &str) -> String {
    match Target::parse(selector) {
        Ok(target) => session_name_for_target(&target),
        Err(_) => selector.to_string(),
    }
}
