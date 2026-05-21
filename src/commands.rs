use crate::cli::{CapabilitiesCommand, ListCommand, SessionCommand, StopCommand, WatchCommand};
use crate::config::load_config;
use crate::constants::{
    CONTEXT_SCHEMA_VERSION, DEFAULT_AUTO_STOP_HYDRATION_AFTER, DEFAULT_WATCH_INTERVAL,
};
use crate::model::{Capabilities, SessionClassification};
use crate::mutagen::{
    run_mutagen_checked, run_mutagen_quiet, terminate_mutagen_session_if_present,
};
use crate::session::{
    manager_local_root, metadata_sessions, remove_session_files, session_records, tracked_sessions,
};
use crate::target::selector_to_session;
use crate::util::{notify_user, parse_duration};
use anyhow::{Result, bail};
use std::thread;

pub(crate) fn capabilities(command: CapabilitiesCommand) -> Result<()> {
    let capabilities = Capabilities {
        name: "editr",
        version: env!("CARGO_PKG_VERSION"),
        context_schema_version: CONTEXT_SCHEMA_VERSION,
        features: vec!["context-v1", "hydrate-v1", "list-json-v1", "watch-v1"],
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(&capabilities)?);
    } else {
        println!("editr {}", capabilities.version);
        println!("Context schema: {}", capabilities.context_schema_version);
        println!("Features: {}", capabilities.features.join(", "));
    }
    Ok(())
}

pub(crate) fn list(command: ListCommand) -> Result<()> {
    let records = session_records(&command.manager_options)?;
    if command.json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("No editr-tracked sessions found.");
        return Ok(());
    }
    for record in records {
        println!(
            "{}\t{:?}\t{:?}\t{}",
            record.session_name,
            record.kind,
            record.classification,
            record.local_path.unwrap_or_else(|| "-".to_string())
        );
    }
    Ok(())
}

pub(crate) fn status(command: SessionCommand) -> Result<()> {
    if command.all && command.selector.is_some() {
        bail!("pass either --all or a target/session, not both");
    }
    if command.all {
        let sessions = tracked_sessions(&command.manager_options)?;
        if sessions.is_empty() {
            println!("No editr-tracked sessions found.");
            return Ok(());
        }
        for session in sessions {
            run_mutagen_checked(["sync", "list", session.as_str()], "mutagen sync list")?;
        }
        return Ok(());
    }
    match command.selector {
        Some(selector) => {
            let session = selector_to_session(&selector);
            run_mutagen_checked(["sync", "list", session.as_str()], "mutagen sync list")
        }
        None => run_mutagen_checked(["sync", "list"], "mutagen sync list"),
    }
}

pub(crate) fn flush(command: SessionCommand) -> Result<()> {
    if command.all && command.selector.is_some() {
        bail!("pass either --all or a target/session, not both");
    }
    if command.all {
        let sessions = tracked_sessions(&command.manager_options)?;
        if sessions.is_empty() {
            println!("No editr-tracked sessions found.");
            return Ok(());
        }
        for session in sessions {
            run_mutagen_checked(["sync", "flush", session.as_str()], "mutagen sync flush")?;
        }
        return Ok(());
    }
    let Some(selector) = command.selector else {
        bail!("flush requires a target/session, or --all");
    };
    let session = selector_to_session(&selector);
    run_mutagen_checked(["sync", "flush", session.as_str()], "mutagen sync flush")
}

pub(crate) fn stop(command: StopCommand) -> Result<()> {
    if command.all && command.selector.is_some() {
        bail!("pass either --all or a target/session, not both");
    }
    if command.all {
        let local_root = manager_local_root(&command.manager_options)?;
        let sessions = metadata_sessions(&local_root)?;
        if sessions.is_empty() {
            println!("No editr-tracked sessions found.");
            return Ok(());
        }
        for session in sessions {
            terminate_mutagen_session_if_present(&session)?;
            remove_session_files(&local_root, &session)?;
        }
        return Ok(());
    }
    let Some(selector) = command.selector else {
        bail!("stop requires a target/session, or --all");
    };
    let session = selector_to_session(&selector);
    terminate_mutagen_session_if_present(&session)?;
    let local_root = manager_local_root(&command.manager_options)?;
    remove_session_files(&local_root, &session)
}

pub(crate) fn watch(command: WatchCommand) -> Result<()> {
    let config = load_config(command.manager_options.config.as_deref())?;
    let interval = command
        .interval
        .as_deref()
        .or_else(|| {
            config
                .watcher
                .as_ref()
                .and_then(|watcher| watcher.interval.as_deref())
        })
        .unwrap_or(DEFAULT_WATCH_INTERVAL);
    let interval = parse_duration(interval)?;
    let auto_stop_after = command
        .auto_stop_hydration_after
        .as_deref()
        .or_else(|| {
            config
                .watcher
                .as_ref()
                .and_then(|watcher| watcher.auto_stop_hydration_after.as_deref())
        })
        .unwrap_or(DEFAULT_AUTO_STOP_HYDRATION_AFTER);
    let auto_stop_after = parse_duration(auto_stop_after)?;
    let notify = if command.no_notify {
        false
    } else if command.notify {
        true
    } else {
        config
            .watcher
            .as_ref()
            .and_then(|watcher| watcher.notify)
            .unwrap_or(true)
    };

    let mut notified_sessions = Vec::new();
    loop {
        let records = session_records(&command.manager_options)?;
        for record in &records {
            if record.classification == SessionClassification::StuckHydration
                && record
                    .age_seconds
                    .is_some_and(|age| age >= auto_stop_after.as_secs())
            {
                run_mutagen_quiet(["sync", "terminate", record.session_name.as_str()]);
                let local_root = manager_local_root(&command.manager_options)?;
                remove_session_files(&local_root, &record.session_name)?;
                continue;
            }
            if notify
                && matches!(
                    record.classification,
                    SessionClassification::Suspicious
                        | SessionClassification::Orphaned
                        | SessionClassification::StuckHydration
                )
            {
                let notification_key =
                    format!("{}:{:?}", record.session_name, record.classification);
                if notified_sessions.contains(&notification_key) {
                    continue;
                }
                notified_sessions.push(notification_key);
                notify_user(
                    "editr session needs attention",
                    &format!("{} is {:?}", record.session_name, record.classification),
                );
            }
        }
        if command.json {
            println!("{}", serde_json::to_string_pretty(&records)?);
        } else {
            for record in records.iter().filter(|record| {
                !matches!(
                    record.classification,
                    SessionClassification::Active | SessionClassification::Persistent
                )
            }) {
                println!("{}: {:?}", record.session_name, record.classification);
            }
        }
        if command.once {
            return Ok(());
        }
        thread::sleep(interval);
    }
}
