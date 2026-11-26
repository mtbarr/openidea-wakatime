//! WakaTime for OpenIDEA: sends heartbeats through `wakatime-cli`, which reads
//! the API key from `~/.wakatime.cfg` and queues heartbeats while offline.

use openidea_plugin::host::{self, Child, Command};
use openidea_plugin::{
    Context, Error, Event, EventKind, Invocation, LogLevel, NotificationLevel, Plugin, StatusItem, register_plugin,
};

const CLI: &str = "wakatime-cli";
const STATUS_ID: &str = "wakatime";
const DASHBOARD: &str = "https://wakatime.com/dashboard";
/// Same rule as the official plugins: a heartbeat on every save, on a file
/// switch, or after this many seconds on the same file.
const HEARTBEAT_EVERY: u64 = 120;
const TODAY_EVERY: u64 = 300;
/// The host allows 8 running programs per plugin; one is kept for `--today`.
const MAX_HEARTBEATS: usize = 7;

#[derive(Default)]
pub struct WakaTime {
    api_version: String,
    last_file: Option<String>,
    last_sent: u64,
    running: Vec<Child>,
    today: Option<(Child, String)>,
    today_at: u64,
    warned: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn should_send(last_file: Option<&str>, last_sent: u64, file: &str, is_write: bool, now: u64) -> bool {
    is_write || last_file != Some(file) || now.saturating_sub(last_sent) >= HEARTBEAT_EVERY
}

fn is_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with('/') || path.starts_with("\\\\") || (b.len() > 2 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

fn join(root: &str, rel: &str) -> String {
    let sep = if root.contains('\\') { '\\' } else { '/' };
    format!("{}{sep}{}", root.trim_end_matches(['/', '\\']), rel.trim_start_matches(['/', '\\']))
}

fn status_text(today: &str) -> String {
    let t = today.lines().next().unwrap_or("").trim();
    if t.is_empty() { "WakaTime".into() } else { format!("WakaTime: {t}") }
}

impl WakaTime {
    fn plugin_ua(&self) -> String {
        format!("openidea/{} openidea-wakatime/{}", self.api_version, env!("CARGO_PKG_VERSION"))
    }

    fn fail(&mut self, what: &str, e: Error) {
        host::log(LogLevel::Warn, &format!("{what}: {e}"));
        if !self.warned {
            self.warned = true;
            host::notify(
                NotificationLevel::Warning,
                "WakaTime",
                &format!(
                    "Could not run wakatime-cli ({e}). Put wakatime-cli on PATH, then allow \
                     process:wakatime-cli again in Settings › Plugins › WakaTime."
                ),
            );
        }
    }

    /// Drops the heartbeats that ended and reads the `--today` output once it is complete.
    fn reap(&mut self) {
        self.running.retain(|c| matches!(c.wait(0), Ok(chunk) if chunk.status.is_none()));
        let mut done = None;
        if let Some((child, out)) = &mut self.today {
            match child.wait(0) {
                Ok(chunk) => {
                    out.push_str(&chunk.stdout);
                    if chunk.status.is_some() {
                        done = Some(out.clone());
                    }
                }
                Err(_) => done = Some(String::new()),
            }
        }
        if let Some(out) = done {
            self.today = None;
            host::set_status_item(
                STATUS_ID,
                &StatusItem {
                    text: status_text(&out),
                    tooltip: Some("Coding time today (click: WakaTime dashboard)".into()),
                    command: Some("dashboard".into()),
                },
            );
        }
    }

    fn refresh_today(&mut self) {
        if self.today.is_some() {
            return;
        }
        self.today_at = now();
        let cmd = Command::new(CLI).args(["--today", "--today-hide-categories", "true", "--plugin", &self.plugin_ua()]);
        match host::start(&cmd, None) {
            Ok(child) => self.today = Some((child, String::new())),
            Err(e) => self.fail("--today", e),
        }
    }

    fn heartbeat(&mut self, file: String, is_write: bool) {
        let t = now();
        if !should_send(self.last_file.as_deref(), self.last_sent, &file, is_write, t) {
            return;
        }
        if self.running.len() >= MAX_HEARTBEATS {
            host::log(LogLevel::Warn, "wakatime-cli is slow: heartbeat skipped");
            return;
        }
        let mut cmd = Command::new(CLI).args(["--entity", &file, "--plugin", &self.plugin_ua()]);
        if is_write {
            cmd = cmd.arg("--write");
        }
        if let Some(root) = host::project_of(&file).and_then(|p| p.root().ok()) {
            cmd = cmd.args(["--project-folder", &root]);
        }
        match host::start(&cmd, None) {
            Ok(child) => {
                self.running.push(child);
                self.last_file = Some(file);
                self.last_sent = t;
            }
            Err(e) => self.fail("heartbeat", e),
        }
    }

    fn absolute(&self, path: &str, project: Option<&str>, ctx: &Invocation) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        if is_absolute(path) {
            return Some(path.to_owned());
        }
        let root = match project {
            Some(r) => r.to_owned(),
            None => ctx.project()?.root().ok()?,
        };
        Some(join(&root, path))
    }
}

impl Plugin for WakaTime {
    fn activate(&mut self, ctx: &Context) -> Result<(), Error> {
        self.api_version = ctx.api_version.clone();
        for kind in [EventKind::FileOpened, EventKind::FileSaved, EventKind::EditorFocused] {
            host::subscribe(kind)?;
        }
        // Typing: an edit counts as activity even before the file is saved.
        if let Err(e) = host::subscribe_paths("document-changed", &["**/*"]) {
            host::log(LogLevel::Info, &format!("document-changed unavailable: {e}"));
        }
        host::set_status_item(
            STATUS_ID,
            &StatusItem { text: "WakaTime".into(), tooltip: Some("WakaTime".into()), command: Some("dashboard".into()) },
        );
        self.refresh_today();
        Ok(())
    }

    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "dashboard" => {
                host::open_url(DASHBOARD)?;
                Ok(None)
            }
            "refresh" => {
                self.refresh_today();
                Ok(None)
            }
            _ => Err(Error::Invalid(format!("unknown command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        self.reap();
        for event in events {
            let (path, project, is_write) = match event {
                Event::FileSaved(p) => (p.as_str(), None, true),
                Event::FileOpened(p) | Event::EditorFocused(Some(p)) => (p.as_str(), None, false),
                Event::DocumentChanged(d) => (d.path.as_str(), d.project.as_deref(), false),
                _ => continue,
            };
            if let Some(file) = self.absolute(path, project, ctx) {
                self.heartbeat(file, is_write);
            }
        }
        if events.contains(&Event::Scheduled) || now().saturating_sub(self.today_at) >= TODAY_EVERY {
            self.refresh_today();
        }
    }
}

register_plugin!(WakaTime);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttles_like_the_official_plugins() {
        assert!(should_send(None, 0, "/a.rs", false, 10));
        assert!(!should_send(Some("/a.rs"), 100, "/a.rs", false, 150));
        assert!(should_send(Some("/a.rs"), 100, "/a.rs", false, 220));
        assert!(should_send(Some("/a.rs"), 100, "/b.rs", false, 101));
        assert!(should_send(Some("/a.rs"), 100, "/a.rs", true, 101));
    }

    #[test]
    fn recognises_absolute_paths() {
        assert!(is_absolute("/home/me/a.rs"));
        assert!(is_absolute("C:\\Users\\me\\a.rs"));
        assert!(is_absolute("C:/Users/me/a.rs"));
        assert!(!is_absolute("src/a.rs"));
        assert_eq!(join("C:\\p\\", "src/a.rs"), "C:\\p\\src/a.rs");
        assert_eq!(join("/p", "src/a.rs"), "/p/src/a.rs");
    }

    #[test]
    fn formats_today() {
        assert_eq!(status_text("2 hrs 5 mins\n"), "WakaTime: 2 hrs 5 mins");
        assert_eq!(status_text(""), "WakaTime");
    }
}
