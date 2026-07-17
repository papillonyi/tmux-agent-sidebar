use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::git::{self, GitData};
use crate::session;
use crate::state::AppState;
use crate::tmux;
use crate::version::{self, UpdateNotice};

/// Channels produced by [`spawn`] that the main event loop drains every tick.
pub(super) struct Workers {
    pub git_rx: Receiver<GitData>,
    pub session_rx: Receiver<HashMap<String, String>>,
    pub version_rx: Receiver<UpdateNotice>,
}

fn git_polling_enabled(bottom_panel_height: u16) -> bool {
    bottom_panel_height > 0
}

/// Spawn the background threads (git polling, session-name polling, version
/// notice fetch) that feed the event loop.
pub(super) fn spawn(state: &AppState) -> Workers {
    let (git_tx, git_rx) = mpsc::channel::<GitData>();
    let (session_tx, session_rx) = mpsc::channel::<HashMap<String, String>>();
    let (version_tx, version_rx) = mpsc::channel::<UpdateNotice>();
    if git_polling_enabled(state.bottom_panel_height) {
        let tmux_pane = state.tmux_pane.clone();
        std::thread::spawn(move || {
            git_poll_loop(&tmux_pane, &git_tx);
        });
    }
    std::thread::spawn(move || {
        session_poll_loop(&session_tx);
    });
    std::thread::spawn(move || {
        if let Some(notice) = version::fetch_update_notice() {
            let _ = version_tx.send(notice);
        }
    });

    Workers {
        git_rx,
        session_rx,
        version_rx,
    }
}

/// Session name polling thread. Scans `~/.claude/sessions/*.json` every 10
/// seconds so the main TUI thread never performs blocking filesystem I/O
/// to refresh `/rename`-assigned labels.
pub(super) fn session_poll_loop(tx: &mpsc::Sender<HashMap<String, String>>) {
    loop {
        std::thread::sleep(Duration::from_secs(10));
        let names = session::scan_session_names();
        if tx.send(names).is_err() {
            return;
        }
    }
}

/// Git data polling thread. Fetches git status every 2 seconds while the
/// bottom panels are visible. PR numbers go through an in-memory
/// `(path, branch)`-keyed cache so `gh pr view` (the only hop that costs
/// GitHub API quota) runs at most once per `PR_CACHE_TTL` instead of every
/// tick.
pub(super) fn git_poll_loop(tmux_pane: &str, git_tx: &mpsc::Sender<GitData>) {
    let mut last_path: Option<String> = None;
    let mut pr_cache = git::PrCache::new();
    loop {
        std::thread::sleep(Duration::from_secs(2));

        // When the sidebar has focus, focused_pane_path returns None.
        // Reuse the last known path so git data keeps updating.
        if let Some(p) = tmux::focused_pane_path(tmux_pane) {
            last_path = Some(p);
        }
        if let Some(ref path) = last_path {
            let mut data = git::fetch_git_data(path);
            data.pr_number = pr_cache.get_or_fetch(
                path,
                &data.branch,
                std::time::Instant::now(),
                git::fetch_pr_number,
            );
            if git_tx.send(data).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_polling_is_enabled_when_bottom_panels_are_visible() {
        assert!(git_polling_enabled(1));
        assert!(git_polling_enabled(20));
    }

    #[test]
    fn git_polling_is_disabled_when_bottom_panels_are_hidden() {
        assert!(!git_polling_enabled(0));
    }

    #[test]
    fn test_git_poll_stops_on_sender_closed() {
        let (tx, rx) = mpsc::channel::<GitData>();
        drop(rx); // Close receiver

        let result = tx.send(GitData::default());
        assert!(result.is_err(), "send should fail when receiver is dropped");
    }
}
