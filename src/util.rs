use crate::constants::CONTEXT_SCHEMA_VERSION;
use anyhow::{Context, Result, bail};
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(chars);
    for byte in bytes {
        if output.len() >= chars {
            break;
        }
        output.push(HEX[(byte >> 4) as usize] as char);
        if output.len() >= chars {
            break;
        }
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn sanitize_component(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            output.push(byte as char);
        } else {
            output.push('-');
        }
    }
    output.trim_matches('-').to_string()
}

pub(crate) fn parse_size(input: &str) -> Result<u64> {
    let input = input.trim();
    if input.is_empty() {
        bail!("size cannot be empty");
    }
    let mut number = String::new();
    let mut unit = String::new();
    for character in input.chars() {
        if character.is_ascii_digit() || character == '.' {
            if !unit.trim().is_empty() {
                bail!("invalid size: {input}");
            }
            number.push(character);
        } else if !character.is_whitespace() {
            unit.push(character);
        }
    }
    let value = number
        .parse::<f64>()
        .with_context(|| format!("invalid size: {input}"))?;
    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1_000.0,
        "m" | "mb" => 1_000_000.0,
        "g" | "gb" => 1_000_000_000.0,
        "t" | "tb" => 1_000_000_000_000.0,
        "ki" | "kib" => 1024.0,
        "mi" | "mib" => 1024.0 * 1024.0,
        "gi" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "ti" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => bail!("unknown size unit: {unit}"),
    };
    Ok((value * multiplier).round() as u64)
}

pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: &[(&str, f64)] = &[
        ("TB", 1_000_000_000_000.0),
        ("GB", 1_000_000_000.0),
        ("MB", 1_000_000.0),
        ("KB", 1_000.0),
    ];
    for (unit, size) in UNITS {
        if bytes as f64 >= *size {
            return format!("{:.1} {unit}", bytes as f64 / size);
        }
    }
    format!("{bytes} B")
}

pub(crate) fn parse_duration(input: &str) -> Result<Duration> {
    let input = input.trim();
    if input.is_empty() {
        bail!("duration cannot be empty");
    }
    let mut number = String::new();
    let mut unit = String::new();
    for character in input.chars() {
        if character.is_ascii_digit() || character == '.' {
            if !unit.trim().is_empty() {
                bail!("invalid duration: {input}");
            }
            number.push(character);
        } else if !character.is_whitespace() {
            unit.push(character);
        }
    }
    let value = number
        .parse::<f64>()
        .with_context(|| format!("invalid duration: {input}"))?;
    let seconds = match unit.to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
        "m" | "min" | "mins" | "minute" | "minutes" => value * 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => value * 60.0 * 60.0,
        _ => bail!("unknown duration unit: {unit}"),
    };
    Ok(Duration::from_secs_f64(seconds))
}

pub(crate) fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|status| status.success())
}

pub(crate) fn notify_user(title: &str, message: &str) {
    let apple_script = format!(
        "display notification \"{}\" with title \"{}\"",
        applescript_escape(message),
        applescript_escape(title)
    );
    if Command::new("osascript")
        .arg("-e")
        .arg(&apple_script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    if Command::new("notify-send")
        .arg(title)
        .arg(message)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    eprintln!("{title}: {message}");
}

fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn current_executable() -> String {
    env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
        .unwrap_or_else(|| "editr".to_string())
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn default_schema_version() -> u32 {
    CONTEXT_SCHEMA_VERSION
}

pub(crate) fn truncate_component(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

pub(crate) fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut output = String::with_capacity(value.len() + 2);
    output.push('\'');
    for character in value.chars() {
        if character == '\'' {
            output.push_str("'\\''");
        } else {
            output.push(character);
        }
    }
    output.push('\'');
    output
}

pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_index, mut text_index) = (0, 0);
    let mut star_index = None;
    let mut star_text_index = 0;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            star_text_index = text_index;
            pattern_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_text_index += 1;
            text_index = star_text_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

pub(crate) fn default_local_root() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("remote")
}

pub(crate) fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .context("could not find the user home directory")
}

pub(crate) fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).into_owned())
}
