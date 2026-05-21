use crate::constants::DEFAULT_CONFIG;
use crate::model::Config;
use crate::util::{default_local_root, expand_path, home_dir, wildcard_match};
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn load_config(path: Option<&Path>) -> Result<Config> {
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

pub(crate) fn config_local_root(config: &Config) -> PathBuf {
    config
        .local_root
        .as_deref()
        .map(expand_path)
        .unwrap_or_else(default_local_root)
}

pub(crate) fn config_ignore_patterns(config: &Config, target: &str) -> Result<Vec<String>> {
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

pub(crate) fn dedupe_patterns(patterns: &mut Vec<String>) {
    let mut deduped = Vec::with_capacity(patterns.len());
    for pattern in patterns.drain(..) {
        if !deduped.contains(&pattern) {
            deduped.push(pattern);
        }
    }
    *patterns = deduped;
}
