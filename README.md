# WakaTime for OpenIDEA

Sends coding activity to [WakaTime](https://wakatime.com) through `wakatime-cli`
and shows today's time in the status bar.

- Heartbeat on save, on switching files, and every 2 minutes while you edit or focus the same file.
- Uses `~/.wakatime.cfg` (API key) and the CLI's offline queue.
- Commands: **WakaTime: Open Dashboard**, **WakaTime: Refresh Today's Time**.

## Requirements

- OpenIDEA 0.1.51+ (plugin API 1.15).
- `wakatime-cli` on `PATH` **before** you allow the `process:wakatime-cli` permission
  (OpenIDEA pins the program's path and hash when you allow it).
  On Windows: copy `%USERPROFILE%\.wakatime\wakatime-cli-windows-amd64.exe` to
  `%USERPROFILE%\.wakatime\wakatime-cli.exe` and add `%USERPROFILE%\.wakatime` to your user `PATH`.

## Build

GitHub Actions builds on every push (`.github/workflows/build.yml`): download the
`openidea-wakatime` artifact, or push a `v*` tag to get a release.

Install in OpenIDEA: **Settings › Plugins › Install Plugin from Disk…** → `community.wakatime.oiplugin`,
then **Review Permissions… → Allow**.
