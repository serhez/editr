use crate::util::shell_quote;
use anyhow::{Context, Result, bail};
use std::process::Command;

pub(crate) fn run_command_checked_with_output(
    command: &mut Command,
    description: &str,
    quiet: bool,
) -> Result<()> {
    if quiet {
        let output = command
            .output()
            .with_context(|| format!("failed to start {description}"))?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let details = [stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if details.is_empty() {
                bail!("{description} failed with status {}", output.status);
            }
            bail!(
                "{description} failed with status {}:\n{}",
                output.status,
                details
            );
        }
        return Ok(());
    }

    let status = command
        .status()
        .with_context(|| format!("failed to start {description}"))?;
    if !status.success() {
        bail!("{description} failed with status {status}");
    }
    Ok(())
}

pub(crate) fn require_command(name: &str) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", shell_quote(name)))
        .status()
        .with_context(|| format!("failed to check for {name}"))?;
    if status.success() {
        return Ok(());
    }
    bail!("{name} is required but was not found on PATH");
}
