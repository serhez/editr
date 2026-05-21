# Session Watcher

`editr watch` periodically classifies editr-owned Mutagen sessions:

```sh
editr watch
editr watch --once --json
editr watch --interval 30s --auto-stop-hydration-after 5m
```

Classifications:

- `active`: Mutagen session exists and the recorded owner process is alive.
- `persistent`: Mutagen session exists and was explicitly created with
  `keep_session`.
- `suspicious`: metadata and Mutagen state disagree, or the owner process is
  gone for a non-persistent project session.
- `orphaned`: a Mutagen session looks like editr but no editr metadata exists.
- `stuck_hydration`: a live per-file hydration session outlived its owner.

The watcher notifies about suspicious sessions. It only auto-stops stale
hydration sessions, because hydration sessions should be short-lived and owned
by an editor buffer. Project sessions are notification-only unless the user
explicitly runs `editr stop`.

## macOS LaunchAgent Example

Create `~/Library/LaunchAgents/dev.editr.watch.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>dev.editr.watch</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/you/.cargo/bin/editr</string>
    <string>watch</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
</dict>
</plist>
```

Load it with:

```sh
launchctl load ~/Library/LaunchAgents/dev.editr.watch.plist
```

## systemd User Service Example

```ini
[Unit]
Description=editr session watcher

[Service]
ExecStart=%h/.cargo/bin/editr watch
Restart=always

[Install]
WantedBy=default.target
```

Enable it with:

```sh
systemctl --user enable --now editr-watch.service
```
