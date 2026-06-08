//! Tmux pane layout — port of `team-layout-tmux/{layout,rebalance,sweep}.ts`.
//!
//! tmux is invoked by name via `std::process::Command` (PATH resolution), so no
//! extra dependency is needed. The pure decision helpers are unit-tested; the
//! command-issuing functions degrade gracefully when tmux is unavailable.

use std::collections::HashMap;
use std::collections::HashSet;
use std::process::Command;
use std::sync::OnceLock;

use crate::team::spec::{TeamError, TeamResult};

/// tmux session name prefix for team-owned sessions.
pub const TEAM_SESSION_PREFIX: &str = "jcode-team-";

/// `canVisualize()` — only attempt layout work inside a tmux client.
pub fn can_visualize() -> bool {
    std::env::var_os("TMUX").is_some()
}

/// Validate that `tmux_target` looks like a plausible tmux target string.
/// Returns an error with a clear message if the target is obviously malformed
/// (empty or contains control characters).
fn validate_tmux_target(target: &str, label: &str) -> TeamResult<()> {
    if target.is_empty() {
        return Err(TeamError::Tmux(format!(
            "{label} target string is empty; cannot issue tmux command",
        )));
    }
    if target.contains(char::is_control) {
        return Err(TeamError::Tmux(format!(
            "{label} target '{target}' contains control characters",
        )));
    }
    Ok(())
}

fn run_tmux(args: &[&str]) -> TeamResult<String> {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| TeamError::Tmux(format!("spawn tmux failed: {e}")))?;
    if !out.status.success() {
        return Err(TeamError::Tmux(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A member to place in the layout.
pub struct LayoutMember<'a> {
    pub name: &'a str,
    /// Command sent into the pane (e.g. `jcode attach --team ... --member ...`).
    pub attach_cmd: &'a str,
    pub cwd: &'a str,
}

/// Pure helper: the split flag for the Nth teammate pane. First pane splits the
/// caller horizontally; subsequent panes alternate vertical/horizontal.
/// Mirrors `buildSplitArgs` in layout.ts.
pub fn split_flag(existing_teammates: usize) -> &'static str {
    if existing_teammates == 0 {
        "-h"
    } else if existing_teammates % 2 == 1 {
        "-v"
    } else {
        "-h"
    }
}

/// Split the caller's window into one pane per member. Returns pane id by member
/// name. No-op (empty map) when tmux is unavailable.
pub fn create_team_layout(
    window_target: &str,
    caller_pane: &str,
    members: &[LayoutMember],
) -> TeamResult<HashMap<String, String>> {
    let mut panes = HashMap::new();
    if !can_visualize() || members.is_empty() {
        return Ok(panes);
    }
    // Coarse input validation on tmux target strings before issuing commands.
    validate_tmux_target(window_target, "window")?;
    validate_tmux_target(caller_pane, "caller_pane")?;
    let existing = list_panes(window_target)?;
    let mut teammates: Vec<String> = existing.into_iter().filter(|p| p != caller_pane).collect();

    for m in members {
        let flag = split_flag(teammates.len());
        let pane_id = if teammates.is_empty() {
            run_tmux(&[
                "split-window",
                "-t",
                caller_pane,
                flag,
                "-d",
                "-l",
                "70%",
                "-P",
                "-F",
                "#{pane_id}",
                "-c",
                m.cwd,
            ])?
        } else {
            let anchor = teammates[teammates.len() / 2].clone();
            run_tmux(&[
                "split-window",
                "-t",
                &anchor,
                flag,
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-c",
                m.cwd,
            ])?
        };
        teammates.push(pane_id.clone());
        panes.insert(m.name.to_string(), pane_id.clone());
        let _ = run_tmux(&["select-pane", "-t", &pane_id, "-T", m.name]);
        if !m.attach_cmd.is_empty() {
            let _ = run_tmux(&["send-keys", "-t", &pane_id, m.attach_cmd, "Enter"]);
        }
    }
    run_tmux(&["select-layout", "-t", window_target, "main-vertical"])?;
    run_tmux(&["resize-pane", "-t", caller_pane, "-x", "30%"])?;
    Ok(panes)
}

fn list_panes(window_target: &str) -> TeamResult<Vec<String>> {
    validate_tmux_target(window_target, "window")?;
    let out = run_tmux(&["list-panes", "-t", window_target, "-F", "#{pane_id}"])?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Tear down a team layout (port of `removeTeamLayout`). Prefers killing the
/// owned session, else kills the recorded panes.
pub fn remove_team_layout(
    owned_session: bool,
    target_session: &str,
    pane_ids: &[String],
) -> TeamResult<()> {
    if !can_visualize() {
        return Ok(());
    }
    if owned_session {
        validate_tmux_target(target_session, "session")?;
        let _ = run_tmux(&["kill-session", "-t", target_session]);
        return Ok(());
    }
    for pane in pane_ids {
        let _ = run_tmux(&["kill-pane", "-t", pane]); // best-effort
    }
    Ok(())
}

/// Close a single member's pane (port of `close-team-member-pane.ts`).
pub fn close_member_pane(pane_id: &str) -> TeamResult<()> {
    if !can_visualize() {
        return Ok(());
    }
    let _ = run_tmux(&["kill-pane", "-t", pane_id]);
    Ok(())
}

/// Re-tile a window (port of `rebalance-team-window.ts`).
pub fn rebalance(window_id: &str, tiled: bool) -> TeamResult<()> {
    if window_id.is_empty() || !can_visualize() {
        return Ok(());
    }
    validate_tmux_target(window_id, "window")?;
    let layout = if tiled { "tiled" } else { "main-vertical" };
    run_tmux(&["select-layout", "-t", window_id, layout])?;
    if !tiled {
        run_tmux(&[
            "set-window-option",
            "-t",
            window_id,
            "main-pane-width",
            "60%",
        ])?;
        run_tmux(&["select-layout", "-t", window_id, layout])?; // reapply after resize
    }
    Ok(())
}

fn team_session_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(&format!(
            r"^{}([0-9a-f]{{8}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{12}})$",
            regex::escape(TEAM_SESSION_PREFIX)
        ))
        .expect("valid team-session regex")
    })
}

/// Pure helper: extract the run id from a `jcode-team-{uuid}` session name.
pub fn parse_team_run_id(session_name: &str) -> Option<String> {
    team_session_regex()
        .captures(session_name)
        .map(|c| c[1].to_string())
}

/// Kill `jcode-team-{uuid}` tmux sessions whose run id is not in `active_run_ids`
/// (port of `sweep-stale-team-sessions.ts`).
pub fn sweep_stale_team_sessions(active_run_ids: &HashSet<String>) -> TeamResult<Vec<String>> {
    if !can_visualize() {
        return Ok(vec![]);
    }
    let listing = match run_tmux(&["list-sessions", "-F", "#{session_name}"]) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let mut killed = Vec::new();
    for line in listing.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(run_id) = parse_team_run_id(line)
            && !active_run_ids.contains(&run_id)
            && run_tmux(&["kill-session", "-t", line]).is_ok()
        {
            killed.push(line.to_string());
        }
    }
    Ok(killed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_flag_alternates() {
        assert_eq!(split_flag(0), "-h"); // first split off caller
        assert_eq!(split_flag(1), "-v");
        assert_eq!(split_flag(2), "-h");
        assert_eq!(split_flag(3), "-v");
    }

    #[test]
    fn parse_team_run_id_matches_only_team_sessions() {
        let uuid = "11111111-2222-4333-8444-555555555555";
        assert_eq!(
            parse_team_run_id(&format!("jcode-team-{uuid}")).as_deref(),
            Some(uuid)
        );
        assert!(parse_team_run_id("jcode-team-not-a-uuid").is_none());
        assert!(parse_team_run_id("some-other-session").is_none());
        assert!(parse_team_run_id(&format!("prefix-jcode-team-{uuid}")).is_none());
    }
}
