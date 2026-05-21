use crate::constants::SSH_PROBE_OPTIONS;
use crate::model::Target;
use crate::target::normalize_remote_path;
use crate::util::shell_quote;
use anyhow::{Context, Result, bail};
use std::process::Command;

pub(crate) fn ensure_remote_directory(target: &Target, create: bool) -> Result<()> {
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

pub(crate) fn ssh_probe_command(host: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(SSH_PROBE_OPTIONS).arg(host);
    command
}

pub(crate) fn remote_file_size(host: &str, remote_path: &str) -> Result<Option<u64>> {
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

pub(crate) fn remote_physical_path(host: &str, remote_path: &str) -> Result<Option<String>> {
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
        Ok(Some(normalize_remote_path(&path)))
    }
}
