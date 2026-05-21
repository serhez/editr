use crate::config::config_ignore_patterns;
use crate::constants::CONTEXT_SCHEMA_VERSION;
use crate::context::relative_remote_path_with_aliases;
use crate::editor::editor_shell_command;
use crate::hydrate::hydration_session_name;
use crate::model::{Config, ResolvedOpen, SessionKind, SessionMetadata, Target};
use crate::mutagen::mutagen_label_value;
use crate::session::session_needs_reconfigure;
use crate::target::session_name_for_target;
use crate::util::{parse_duration, parse_size, wildcard_match};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn parses_targets() {
    let target = Target::parse("user@host:/a/b/").unwrap();
    assert_eq!(target.host, "user@host");
    assert_eq!(target.path, "/a/b");
    assert_eq!(target.to_string(), "user@host:/a/b");
}

#[test]
fn rejects_non_absolute_targets() {
    assert!(Target::parse("host:relative").is_err());
    assert!(Target::parse("/local/path").is_err());
}

#[test]
fn builds_stable_session_names() {
    let target = Target::parse("host:/a/b").unwrap();
    assert_eq!(
        session_name_for_target(&target),
        session_name_for_target(&target)
    );
    assert!(session_name_for_target(&target).starts_with("editr-host-"));
}

#[test]
fn wildcard_patterns_match_targets() {
    assert!(wildcard_match(
        "cluster:/home/user/project*",
        "cluster:/home/user/project/runs"
    ));
    assert!(wildcard_match("*", "anything"));
    assert!(wildcard_match("host:/a/?", "host:/a/b"));
    assert!(!wildcard_match("host:/a/?", "host:/a/bc"));
}

#[test]
fn merges_ignore_config() {
    let mut ignore = BTreeMap::new();
    ignore.insert("*".to_string(), vec!["node_modules/".to_string()]);
    ignore.insert("host:/project*".to_string(), vec!["target/".to_string()]);
    let config = Config {
        ignore: Some(ignore),
        ..Default::default()
    };
    let patterns = config_ignore_patterns(&config, "host:/project/src").unwrap();
    assert_eq!(patterns, vec!["node_modules/", "target/"]);
}

#[test]
fn detects_session_reconfiguration_changes() {
    let target = Target::parse("host:/project").unwrap();
    let resolved = ResolvedOpen {
        target: target.clone(),
        local_root: PathBuf::from("/tmp"),
        local_path: PathBuf::from("/tmp/project"),
        session_name: "editr-host-test".to_string(),
        metadata_file: PathBuf::from("/tmp/project.toml"),
        context_file: PathBuf::from("/tmp/project.context.json"),
        editor: "nvim".to_string(),
        mode: "two-way-safe".to_string(),
        sync_vcs: true,
        keep_session: false,
        ignore_patterns: vec!["node_modules/".to_string()],
        bootstrap: true,
        create_remote: false,
        allow_nonempty: false,
        dry_run: false,
    };
    let mut metadata = SessionMetadata {
        schema_version: CONTEXT_SCHEMA_VERSION,
        kind: SessionKind::Project,
        session_name: resolved.session_name.clone(),
        target: target.to_string(),
        host: target.host,
        remote_path: target.path,
        local_path: resolved.local_path.display().to_string(),
        mode: resolved.mode.clone(),
        sync_vcs: resolved.sync_vcs,
        keep_session: resolved.keep_session,
        project_session: None,
        relative_path: None,
        owner_pid: None,
        created_at_unix: 0,
        updated_at_unix: 0,
        context_file: None,
        ignore_patterns: resolved.ignore_patterns.clone(),
    };

    assert!(!session_needs_reconfigure(&metadata, &resolved));
    metadata.ignore_patterns = vec!["data/".to_string()];
    assert!(session_needs_reconfigure(&metadata, &resolved));
}

#[test]
fn appends_path_to_editor_shell_command() {
    assert_eq!(
        editor_shell_command("nvim --clean", "."),
        "nvim --clean '.'"
    );
}

#[test]
fn substitutes_path_placeholder_in_editor_shell_command() {
    assert_eq!(
        editor_shell_command("nvim --cmd 'set number' {path}", "."),
        "nvim --cmd 'set number' '.'"
    );
}

#[test]
fn shell_quotes_editor_path_argument() {
    assert_eq!(
        editor_shell_command("nvim", "dir/it's here"),
        "nvim 'dir/it'\\''s here'"
    );
}

#[test]
fn parses_sizes() {
    assert_eq!(parse_size("25 MB").unwrap(), 25_000_000);
    assert_eq!(parse_size("1.5MiB").unwrap(), 1_572_864);
    assert!(parse_size("12 parsecs").is_err());
}

#[test]
fn parses_durations() {
    assert_eq!(parse_duration("60s").unwrap(), Duration::from_secs(60));
    assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
    assert!(parse_duration("1 fortnight").is_err());
}

#[test]
fn maps_relative_remote_paths() {
    assert_eq!(
        relative_remote_path_with_aliases("/repo", &[], "/repo/logs/run.txt").unwrap(),
        "logs/run.txt"
    );
    assert!(relative_remote_path_with_aliases("/repo", &[], "/other/run.txt").is_err());
    assert!(relative_remote_path_with_aliases("/repo", &[], "/repo").is_err());
}

#[test]
fn maps_relative_remote_paths_through_aliases() {
    let aliases = vec!["/mnt/storage/user/project".to_string()];
    assert_eq!(
        relative_remote_path_with_aliases(
            "/home/user/project",
            &aliases,
            "/mnt/storage/user/project/README.md",
        )
        .unwrap(),
        "README.md"
    );
}

#[test]
fn builds_hydration_session_names() {
    let left = hydration_session_name("editr-host-project", "/repo/a.txt");
    let right = hydration_session_name("editr-host-project", "/repo/a.txt");
    assert_eq!(left, right);
    assert!(left.starts_with("editr-hydrate-editr-host-project-"));
}

#[test]
fn caps_mutagen_label_values() {
    let long = "editr-hydrate-editr-very-long-host-name-with-a-very-long-project-name-and-path-0123456789abcdef-bootstrap";
    let value = mutagen_label_value(long);
    assert!(value.len() <= 63);
    assert!(value.starts_with("editr-hydrate-editr-very-long-host-name"));
}
