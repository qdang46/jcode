use super::{App, DisplayMessage, ProcessingStatus, RunResult};
use crate::replay::{PaneReplayInput, ReplayEvent, TimelineEvent};
use crate::tui::backend::RemoteEventState;
use anyhow::Result;
use std::time::{Duration, Instant};

pub(super) async fn run_replay(
    _app: App,
    _terminal: (),
    _timeline: Vec<TimelineEvent>,
    _speed: f64,
) -> Result<RunResult> {
    // Replay mode is not yet supported with frankentui - the main TUI uses tui::runtime::run_frankentui()
    anyhow::bail!("replay mode not yet supported with frankentui")
}

pub(super) async fn run_swarm_replay(
    _terminal: (),
    _panes: Vec<PaneReplayInput>,
    _speed: f64,
    _centered_override: Option<bool>,
) -> Result<()> {
    // Swarm replay mode is not yet supported with frankentui
    anyhow::bail!("swarm replay mode not yet supported with frankentui")
}

// Replay helper functions - these are kept for potential future use when frankentui replay is implemented

pub(super) fn apply_replay_event(
    app: &mut App,
    remote: &mut impl RemoteEventState,
    replay_event: &ReplayEvent,
    replay_turn_id: &mut u64,
    replay_processing_started_ms: Option<f64>,
) {
    match replay_event {
        ReplayEvent::UserMessage { text } => {
            app.push_display_message(DisplayMessage {
                role: "user".to_string(),
                content: text.clone(),
                tool_calls: vec![],
                duration_secs: None,
                title: None,
                tool_data: None,
            });
        }
        ReplayEvent::StartProcessing => {
            *replay_turn_id += 1;
            app.current_message_id = Some(*replay_turn_id);
            app.is_processing = true;
            app.processing_started = Some(Instant::now());
            app.status = ProcessingStatus::Thinking(Instant::now());
            app.streaming_tps_start = None;
            app.streaming_tps_elapsed = Duration::ZERO;
            app.streaming_tps_collect_output = false;
            app.streaming_total_output_tokens = 0;
            app.streaming_tps_observed_output_tokens = 0;
            app.streaming_tps_observed_elapsed = Duration::ZERO;
            app.replay_processing_started_ms = replay_processing_started_ms;
        }
        ReplayEvent::MemoryInjection {
            summary,
            content,
            count: _,
        } => {
            let display = DisplayMessage::memory(summary.clone(), content.clone());
            app.push_display_message(display);
        }
        ReplayEvent::DisplayMessage {
            role,
            title,
            content,
        } => {
            if role == "swarm" {
                app.swarm_enabled = true;
            }
            app.push_display_message(DisplayMessage {
                role: role.clone(),
                content: content.clone(),
                tool_calls: vec![],
                duration_secs: None,
                title: title.clone(),
                tool_data: None,
            });
        }
        ReplayEvent::SwarmStatus { members } => {
            app.swarm_enabled = true;
            app.remote_swarm_members = members.clone();
        }
        ReplayEvent::SwarmPlan {
            swarm_id,
            version,
            items,
        } => {
            app.swarm_enabled = true;
            app.swarm_plan_swarm_id = Some(swarm_id.clone());
            app.swarm_plan_version = Some(*version);
            app.swarm_plan_items = items.clone();
        }
        ReplayEvent::Server(server_event) => {
            if let crate::protocol::ServerEvent::TextDelta { text } = server_event {
                if !text.is_empty() {
                    app.append_streaming_text(text);
                    if matches!(app.status, ProcessingStatus::Thinking(_)) {
                        app.status = ProcessingStatus::Streaming;
                    }
                    app.last_stream_activity = Some(Instant::now());
                }
            } else {
                app.handle_server_event(server_event.clone(), remote);
            }
        }
    }
}

pub(super) fn update_replay_elapsed_override(app: &mut App, sim_time_ms: f64) {
    if let Some(start_ms) = app.replay_processing_started_ms {
        let elapsed_ms = (sim_time_ms - start_ms).max(0.0);
        app.replay_elapsed_override = Some(Duration::from_millis(elapsed_ms as u64));
    } else {
        app.replay_elapsed_override = None;
    }
}

fn schedule_replay_events(timeline: &[TimelineEvent]) -> Vec<(f64, ReplayEvent)> {
    let mut abs_time_ms = 0.0;
    crate::replay::timeline_to_replay_events(timeline)
        .into_iter()
        .map(|(delay_ms, event)| {
            abs_time_ms += delay_ms as f64;
            (abs_time_ms, event)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::schedule_replay_events;
    use crate::replay::{ReplayEvent, TimelineEvent, TimelineEventKind};

    #[test]
    fn schedule_replay_events_accumulates_relative_delays() {
        let timeline = vec![
            TimelineEvent {
                t: 0,
                kind: TimelineEventKind::UserMessage {
                    text: "hi".to_string(),
                },
            },
            TimelineEvent {
                t: 250,
                kind: TimelineEventKind::Thinking { duration: 250 },
            },
            TimelineEvent {
                t: 500,
                kind: TimelineEventKind::StreamText {
                    text: "there".to_string(),
                    speed: 80,
                },
            },
        ];

        let scheduled = schedule_replay_events(&timeline);
        assert_eq!(scheduled.len(), 4);
        assert_eq!(scheduled[0].0, 0.0);
        assert_eq!(scheduled[1].0, 250.0);
        assert_eq!(scheduled[2].0, 500.0);
        assert!(scheduled[3].0 > scheduled[2].0);
        assert!(matches!(scheduled[0].1, ReplayEvent::UserMessage { .. }));
        assert!(matches!(scheduled[1].1, ReplayEvent::StartProcessing));
        assert!(matches!(scheduled[2].1, ReplayEvent::Server(_)));
        assert!(matches!(scheduled[3].1, ReplayEvent::Server(_)));
    }
}
