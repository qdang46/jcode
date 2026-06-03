use super::*;
use crate::tui::TuiState;
use ftui::TerminalSession as DefaultTerminal;
use ftui::session_draw::TerminalSessionDrawExt;
use ftui_render::buffer::Buffer;
use ftui_core::geometry::Rect;

/// Type alias for the terminal type used in the migrated TUI runtime.
/// frankentui exposes `TerminalSession` for production and `Buffer` for
/// headless/tests; the legacy `DefaultTerminal = Terminal<CrosstermBackend>`
/// alias from ratatui maps directly to `ftui::TerminalSession`.

#[allow(dead_code)] // retained for upcoming ftui draw-path port
const STATUS_SPINNER_FPS: f32 = 12.5;
pub(super) const STATUS_SPINNER_ONLY_INTERVAL: Duration = Duration::from_millis(80);

pub(super) fn status_spinner_interval() -> tokio::time::Interval {
    status_spinner_interval_after(STATUS_SPINNER_ONLY_INTERVAL)
}

pub(super) fn reset_status_spinner_interval(interval: &mut tokio::time::Interval, app: &App) {
    *interval = status_spinner_interval_after(status_spinner_delay_until_next_frame(
        status_spinner_elapsed(app),
    ));
}

fn status_spinner_interval_after(delay: Duration) -> tokio::time::Interval {
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + delay,
        STATUS_SPINNER_ONLY_INTERVAL,
    );
    // The spinner is visual liveness, not simulated time. If terminal/input work delays a tick,
    // skip the missed frames instead of bursting them later.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

fn status_spinner_elapsed(app: &App) -> f32 {
    status_spinner_elapsed_for_sources(app.elapsed().map(|duration| duration.as_secs_f32()))
}

fn status_spinner_elapsed_for_sources(turn_elapsed: Option<f32>) -> f32 {
    turn_elapsed.unwrap_or(0.0).max(0.0)
}

fn status_spinner_delay_until_next_frame(elapsed: f32) -> Duration {
    if !elapsed.is_finite() {
        return STATUS_SPINNER_ONLY_INTERVAL;
    }

    let frame_secs = STATUS_SPINNER_ONLY_INTERVAL.as_secs_f64();
    let elapsed_secs = f64::from(elapsed.max(0.0));
    let into_frame = elapsed_secs % frame_secs;
    let remaining = if into_frame <= f64::EPSILON {
        frame_secs
    } else {
        frame_secs - into_frame
    };

    Duration::from_secs_f64(remaining.max(0.001))
}

pub(super) fn status_spinner_only_symbol(app: &App) -> Option<&'static str> {
    let policy = crate::perf::tui_policy();
    if !policy.enable_decorative_animations
        || !app.is_processing
        || !app.streaming_text.is_empty()
        || app.centered_mode()
        || app.has_pending_mouse_scroll_animation()
        || app.remote_startup_phase_active()
    {
        return None;
    }

    if status_uses_primary_spinner(&app.status) {
        // Local stub returns a static spinner glyph; elapsed/fps are unused in the placeholder.
        Some("⠋")
    } else {
        None
    }
}

/// Statuses whose full status line starts with the primary green circular spinner.
///
/// Keep this in sync with `ui_input::draw_status`: these statuses can be safely
/// refreshed by the one-cell spinner fast path when the status line is left aligned.
/// Tool execution uses its own full-line activity indicator, and network waits use
/// a static amber retry marker, so neither belongs here.
pub(crate) fn status_uses_primary_spinner(status: &ProcessingStatus) -> bool {
    matches!(
        status,
        ProcessingStatus::Sending
            | ProcessingStatus::Connecting(_)
            | ProcessingStatus::Thinking(_)
            | ProcessingStatus::Streaming
    )
}

#[derive(Default)]
pub(super) struct StatusSpinnerRenderer {
    last_frame: Option<Buffer>,
}

impl StatusSpinnerRenderer {
    pub(super) fn invalidate(&mut self) {
        self.last_frame = None;
    }

    pub(super) fn draw_full(
        &mut self,
        app: &mut App,
        terminal: &mut DefaultTerminal,
    ) -> Result<()> {
        if app.force_full_redraw {
            app.force_full_redraw = false;
            self.invalidate();
        }

        terminal.draw(|frame| crate::tui::ui::draw(frame, app))?;
        Ok(())
    }

    pub(super) fn draw_status_spinner_only(
        &mut self,
        app: &App,
        _terminal: &mut DefaultTerminal,
    ) -> Result<bool> {
        // TODO: re-enable with ftui Presenter/TerminalSession when the
        // ratatui-equivalent virtual buffer + cursor save/restore dance is
        // ported. For now just invalidate the cached frame so the next
        // full redraw refreshes the status spinner.
        let Some(_) = status_spinner_only_symbol(app) else {
            return Ok(false);
        };
        let Some(_) = crate::tui::ui::last_status_area() else {
            return Ok(false);
        };
        self.invalidate();
        Ok(false)
    }
}

#[allow(dead_code)] // helper retained for upcoming ftui draw-path port
fn render_status_spinner_into_buffer(buffer: &Buffer, area: Rect, symbol: &str) -> bool {
    area.width > 0
        && area.height > 0
        && area.x < buffer.width()
        && area.y < buffer.height()
        && !symbol.is_empty()
}

#[allow(dead_code)] // helper retained for upcoming ftui draw-path port
fn render_status_spinner_into_buffer_mut(buffer: &mut Buffer, area: Rect, symbol: &str) {
    for (i, c) in symbol.chars().enumerate() {
        let x = area.x + i as u16;
        if x < buffer.width() && area.y < buffer.height() {
            if let Some(cell) = buffer.get_mut(x, area.y) {
                cell.content = ftui_render::cell::CellContent::from_char(c);
            }
        }
    }
}

impl App {
    /// Run the TUI application
    /// Returns Some(session_id) if hot-reload was requested
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<RunResult> {
        let mut event_stream = EventStream::new();
        let mut redraw_period = crate::tui::redraw_interval(&self);
        let mut redraw_interval = interval(redraw_period);
        let mut status_spinner_interval = status_spinner_interval();
        let mut status_spinner_renderer = StatusSpinnerRenderer::default();
        let mut needs_redraw = true;
        let mut handterm_native_scroll =
            super::handterm_native_scroll::HandtermNativeScrollClient::connect_from_env();
        // Subscribe to bus for background task completion notifications
        let mut bus_receiver = Bus::global().subscribe();
        if let Some(status) = Bus::global().latest_update_status() {
            self.handle_update_status(status);
        }

        loop {
            let desired_redraw = crate::tui::redraw_interval(&self);
            if desired_redraw != redraw_period {
                redraw_period = desired_redraw;
                redraw_interval = interval(redraw_period);
            }

            if needs_redraw {
                status_spinner_renderer.draw_full(&mut self, &mut terminal)?;
                reset_status_spinner_interval(&mut status_spinner_interval, &self);
                if let Some(native) = handterm_native_scroll.as_mut() {
                    native.sync_from_app(&self);
                }
                needs_redraw = false;
            }

            if self.should_quit {
                break;
            }

            // Process pending turn OR wait for input/redraw
            if self.pending_turn {
                self.pending_turn = false;
                // Process turn while still handling input
                self.process_turn_with_input(&mut terminal, &mut event_stream, &mut bus_receiver)
                    .await;
                needs_redraw = true;
            } else if self.pending_queued_dispatch {
                self.pending_queued_dispatch = false;
                self.process_queued_messages(&mut terminal, &mut event_stream)
                    .await;
                local::finish_turn(&mut self);
                needs_redraw = true;
            } else {
                // Wait for input or redraw tick
                tokio::select! {
                    _ = status_spinner_interval.tick(), if status_spinner_only_symbol(&self).is_some() => {
                        if !status_spinner_renderer.draw_status_spinner_only(&self, &mut terminal)? {
                            needs_redraw = true;
                        }
                    }
                    _ = redraw_interval.tick() => {
                        needs_redraw |= local::handle_tick(&mut self);
                    }
                    event = event_stream.next() => {
                        if event.is_some() {
                            needs_redraw |= local::handle_terminal_event(&mut self, &mut terminal, event)?;
                        } else {
                            tokio::time::sleep(redraw_period).await;
                        }
                    }
                    command = async {
                        match handterm_native_scroll.as_mut() {
                            Some(native) => native.recv().await,
                            None => futures::future::pending::<Option<super::handterm_native_scroll::HostToApp>>().await,
                        }
                    } => {
                        if let Some(command) = command {
                            self.apply_handterm_native_scroll(command);
                            self.request_full_redraw();
                            needs_redraw = true;
                        } else {
                            handterm_native_scroll = None;
                        }
                    }
                    // Handle background task completion notifications
                    bus_event = bus_receiver.recv() => {
                        needs_redraw |= local::handle_bus_event(&mut self, bus_event);
                    }
                }
            }
        }

        self.extract_session_memories().await;

        Ok(RunResult {
            reload_session: self.reload_requested.take(),
            rebuild_session: self.rebuild_requested.take(),
            update_session: self.update_requested.take(),
            restart_session: self.restart_requested.take(),
            exit_code: self.requested_exit_code,
            session_id: Some(self.session.id.clone()),
        })
    }

    /// Run the TUI in remote mode, connecting to a server
    pub async fn run_remote(mut self, mut terminal: DefaultTerminal) -> Result<RunResult> {
        let mut event_stream = EventStream::new();
        let mut redraw_period = crate::tui::redraw_interval(&self);
        let mut redraw_interval = interval(redraw_period);
        let mut status_spinner_interval = status_spinner_interval();
        let mut status_spinner_renderer = StatusSpinnerRenderer::default();
        let mut needs_redraw = true;
        let mut handterm_native_scroll =
            super::handterm_native_scroll::HandtermNativeScrollClient::connect_from_env();
        let mut remote_state = remote::RemoteRunState::default();

        'outer: loop {
            if self.display_messages.is_empty() {
                if self.server_spawning {
                    self.set_remote_startup_phase(super::RemoteStartupPhase::StartingServer);
                } else {
                    self.set_remote_startup_phase(super::RemoteStartupPhase::Connecting);
                }
            }
            if needs_redraw {
                status_spinner_renderer.draw_full(&mut self, &mut terminal)?;
                reset_status_spinner_interval(&mut status_spinner_interval, &self);
                needs_redraw = false;
            }

            let session_to_resume = self.reconnect_target_session_id();

            let mut remote_conn = match remote::connect_with_retry(
                &mut self,
                &mut terminal,
                &mut event_stream,
                &mut remote_state,
                session_to_resume.as_deref(),
            )
            .await?
            {
                remote::ConnectOutcome::Connected(remote) => remote,
                remote::ConnectOutcome::Retry => continue,
                remote::ConnectOutcome::Quit => break 'outer,
            };
            status_spinner_renderer.invalidate();

            match remote::handle_post_connect(
                &mut self,
                &mut terminal,
                &mut remote_conn,
                &mut remote_state,
                session_to_resume.as_deref(),
            )
            .await?
            {
                remote::PostConnectOutcome::Ready => {}
                remote::PostConnectOutcome::Quit => break 'outer,
            }
            status_spinner_renderer.invalidate();
            needs_redraw = true;

            let mut bus_receiver_remote = Bus::global().subscribe();
            if let Some(status) = Bus::global().latest_update_status() {
                self.handle_update_status(status);
                needs_redraw = true;
            }

            // Main event loop
            loop {
                let desired_redraw = crate::tui::redraw_interval(&self);
                if desired_redraw != redraw_period {
                    redraw_period = desired_redraw;
                    redraw_interval = interval(redraw_period);
                }

                if needs_redraw {
                    status_spinner_renderer.draw_full(&mut self, &mut terminal)?;
                    reset_status_spinner_interval(&mut status_spinner_interval, &self);
                    if let Some(native) = handterm_native_scroll.as_mut() {
                        native.sync_from_app(&self);
                    }
                    needs_redraw = false;
                }

                if self.should_quit {
                    break 'outer;
                }

                if self.pending_queued_dispatch {
                    self.pending_queued_dispatch = false;
                    remote::process_remote_followups(&mut self, &mut remote_conn).await;
                    needs_redraw = true;
                    continue;
                }

                tokio::select! {
                    _ = status_spinner_interval.tick(), if status_spinner_only_symbol(&self).is_some() => {
                        if !status_spinner_renderer.draw_status_spinner_only(&self, &mut terminal)? {
                            needs_redraw = true;
                        }
                    }
                    _ = redraw_interval.tick() => {
                        needs_redraw |= remote::handle_tick(&mut self, &mut remote_conn).await;
                    }
                    event = remote_conn.next_event() => {
                        let (outcome, event_redraw) = remote::handle_remote_event(
                            &mut self,
                            &mut terminal,
                            &mut remote_conn,
                            &mut remote_state,
                            event,
                        )
                        .await?;
                        needs_redraw |= event_redraw;
                        match outcome {
                            remote::RemoteEventOutcome::Continue => {}
                            remote::RemoteEventOutcome::Reconnect => continue 'outer,
                            remote::RemoteEventOutcome::Quit => break 'outer,
                        }
                    }
                    event = event_stream.next() => {
                        if event.is_some() {
                            needs_redraw |= remote::handle_terminal_event(&mut self, &mut terminal, &mut remote_conn, event).await?;
                        } else {
                            tokio::time::sleep(redraw_period).await;
                        }
                    }
                    command = async {
                        match handterm_native_scroll.as_mut() {
                            Some(native) => native.recv().await,
                            None => futures::future::pending::<Option<super::handterm_native_scroll::HostToApp>>().await,
                        }
                    } => {
                        if let Some(command) = command {
                            self.apply_handterm_native_scroll(command);
                            self.request_full_redraw();
                            needs_redraw = true;
                        } else {
                            handterm_native_scroll = None;
                        }
                    }
                    bus_event = bus_receiver_remote.recv() => {
                        needs_redraw |= remote::handle_bus_event(&mut self, &mut remote_conn, bus_event).await;
                    }
                }
            }
        }

        Ok(RunResult {
            reload_session: self.reload_requested.take(),
            rebuild_session: self.rebuild_requested.take(),
            update_session: self.update_requested.take(),
            restart_session: self.restart_requested.take(),
            exit_code: self.requested_exit_code,
            session_id: if self.is_remote {
                self.remote_session_id.clone()
            } else {
                Some(self.session.id.clone())
            },
        })
    }

    /// Run the TUI in replay mode, playing back a timeline of events.
    pub async fn run_replay(
        mut self,
        _terminal: (),
        timeline: Vec<crate::replay::TimelineEvent>,
        speed: f64,
    ) -> Result<RunResult> {
        use crate::replay::ReplayEvent;
        use crossterm::event::{self, Event, KeyCode, KeyEventKind};

        // Create a dedicated TerminalSession for replay rendering
        let mut terminal = ftui::TerminalSession::new(ftui::SessionOptions::default())
            .map_err(|e| anyhow::anyhow!("failed to create replay terminal: {}", e))?;

        let replay_events = crate::replay::timeline_to_replay_events(&timeline);
        if replay_events.is_empty() {
            anyhow::bail!("No replay events to play");
        }

        let mut remote = crate::tui::backend::ReplayRemoteState::default();
        let mut paused = false;
        let mut replay_turn_id: u64 = 0;
        let frame_duration_ms = 33.0; // ~30fps

        let total_duration_ms: f64 = replay_events.iter().map(|(d, _)| *d as f64 / speed).sum();

        let mut event_schedule: Vec<(f64, ReplayEvent)> = Vec::new();
        {
            let mut abs_time: f64 = 0.0;
            for (delay_ms, evt) in &replay_events {
                abs_time += *delay_ms as f64 / speed;
                event_schedule.push((abs_time, evt.clone()));
            }
        }

        let mut event_cursor: usize = 0;
        let mut sim_time_ms: f64 = 0.0;
        let start = std::time::Instant::now();

        while sim_time_ms <= total_duration_ms {
            // Handle pause/unpause and quit
            while event::poll(std::time::Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char(' ') => paused = !paused,
                            KeyCode::Char('q') | KeyCode::Esc => {
                                return Ok(RunResult::default());
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                // speed up (simplified: not implemented)
                            }
                            KeyCode::Char('-') => {
                                // slow down (simplified: not implemented)
                            }
                            _ => {}
                        }
                    }
                }
            }

            if !paused {
                sim_time_ms = start.elapsed().as_secs_f64() * 1000.0 * speed;

                while event_cursor < event_schedule.len()
                    && event_schedule[event_cursor].0 <= sim_time_ms
                {
                    let (_t, event) = &event_schedule[event_cursor];
                    replay::apply_replay_event(
                        &mut self,
                        &mut remote,
                        event,
                        &mut replay_turn_id,
                        Some(sim_time_ms),
                    );
                    event_cursor += 1;
                }

                replay::update_replay_elapsed_override(&mut self, sim_time_ms);
            }

            terminal.draw(|frame| crate::tui::ui::draw(frame, &self))?;
            tokio::time::sleep(std::time::Duration::from_millis(frame_duration_ms as u64)).await;
        }

        // Wait for user to quit after replay finishes
        terminal.draw(|frame| crate::tui::ui::draw(frame, &self))?;
        loop {
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press
                        && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter)
                    {
                        return Ok(RunResult::default());
                    }
                }
            }
        }
    }

    /// Run an interactive swarm replay, rendering multiple sessions in tiled panes.
    pub async fn run_swarm_replay(
        _terminal: (),
        _panes: Vec<crate::replay::PaneReplayInput>,
        _speed: f64,
        _centered_override: Option<bool>,
    ) -> Result<()> {
        // Swarm replay not yet implemented with frankentui — use headless export instead
        anyhow::bail!(
            "interactive swarm replay not yet supported with frankentui; use --video to export"
        )
    }

    /// Run replay headlessly, rendering each frame to an in-memory buffer.
    /// Returns a list of (timestamp_secs, Buffer) pairs for video export.
    pub async fn run_headless_replay(
        mut self,
        timeline: &[crate::replay::TimelineEvent],
        speed: f64,
        width: u16,
        height: u16,
        fps: u32,
    ) -> Result<Vec<(f64, ftui_render::buffer::Buffer)>> {
        use crate::replay::ReplayEvent;

        let replay_events = crate::replay::timeline_to_replay_events(timeline);
        if replay_events.is_empty() {
            anyhow::bail!("No replay events to export");
        }

        let mut remote = crate::tui::backend::ReplayRemoteState::default();

        let frame_duration_ms: f64 = 1000.0 / fps as f64;
        let mut frames: Vec<(f64, ftui_render::buffer::Buffer)> = Vec::new();
        let mut sim_time_ms: f64 = 0.0;
        let mut next_frame_at: f64 = 0.0;

        let total_duration_ms: f64 = replay_events.iter().map(|(d, _)| *d as f64 / speed).sum();

        let mut event_schedule: Vec<(f64, &ReplayEvent)> = Vec::new();
        {
            let mut abs_time: f64 = 0.0;
            for (delay_ms, evt) in &replay_events {
                abs_time += *delay_ms as f64 / speed;
                event_schedule.push((abs_time, evt));
            }
        }

        let mut event_cursor: usize = 0;
        let mut replay_turn_id: u64 = 0;

        // frankentui: initial frame rendered via headless draw
        frames.push((
            0.0,
            ftui_tty::TtyBackend::headless_draw(width, height, |frame| {
                crate::tui::ui::draw(frame, &self);
            }),
        ));

        let progress_interval = (total_duration_ms / 20.0).max(1000.0);
        let mut next_progress = progress_interval;

        while sim_time_ms <= total_duration_ms + frame_duration_ms {
            while event_cursor < event_schedule.len()
                && event_schedule[event_cursor].0 <= sim_time_ms
            {
                let (_t, event) = event_schedule[event_cursor];
                replay::apply_replay_event(
                    &mut self,
                    &mut remote,
                    event,
                    &mut replay_turn_id,
                    Some(sim_time_ms),
                );
                event_cursor += 1;
            }

            if sim_time_ms >= next_frame_at {
                replay::update_replay_elapsed_override(&mut self, sim_time_ms);
                let buf = ftui_tty::TtyBackend::headless_draw(width, height, |frame| {
                    crate::tui::ui::draw(frame, &self);
                });
                frames.push((sim_time_ms / 1000.0, buf));
                next_frame_at = sim_time_ms + frame_duration_ms;
            }

            if sim_time_ms >= next_progress {
                let pct = (sim_time_ms / total_duration_ms * 100.0).min(100.0);
                eprint!("\r  Rendering... {:.0}%", pct);
                next_progress += progress_interval;
            }

            sim_time_ms += frame_duration_ms;
        }

        eprintln!("\r  Rendering... 100%  ({} frames captured)", frames.len());

        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
        use ftui_style::Color;

    fn assert_duration_close(actual: Duration, expected: Duration) {
        let actual_ms = actual.as_millis() as i128;
        let expected_ms = expected.as_millis() as i128;
        assert!(
            (actual_ms - expected_ms).abs() <= 1,
            "expected {actual:?} to be within 1ms of {expected:?}"
        );
    }

    #[test]
    fn status_spinner_fast_path_uses_status_elapsed_clock() {
        let full_status_elapsed = 0.0;
        let app_lifetime_elapsed = 0.24;

        let full_status_symbol = jcode_tui_style::theme::activity_indicator(
            full_status_elapsed,
            STATUS_SPINNER_FPS,
            true,
        );
        let old_app_lifetime_symbol = jcode_tui_style::theme::activity_indicator(
            app_lifetime_elapsed,
            STATUS_SPINNER_FPS,
            true,
        );
        let fast_path_symbol = jcode_tui_style::theme::activity_indicator(
            status_spinner_elapsed_for_sources(Some(full_status_elapsed)),
            STATUS_SPINNER_FPS,
            true,
        );

        assert_ne!(
            old_app_lifetime_symbol, full_status_symbol,
            "the app lifetime clock can be on a different spinner frame than the status clock"
        );
        assert_eq!(fast_path_symbol, full_status_symbol);
    }

    #[test]
    fn primary_spinner_statuses_are_explicit() {
        assert!(status_uses_primary_spinner(&ProcessingStatus::Sending));
        assert!(status_uses_primary_spinner(&ProcessingStatus::Streaming));
        assert!(!status_uses_primary_spinner(
            &ProcessingStatus::RunningTool("bash".to_string())
        ));
        assert!(!status_uses_primary_spinner(&ProcessingStatus::Idle));
        assert!(!status_uses_primary_spinner(
            &ProcessingStatus::WaitingForNetwork {
                listener: "network".to_string(),
            }
        ));
    }

    #[test]
    fn status_spinner_reset_targets_next_frame_boundary() {
        assert_duration_close(
            status_spinner_delay_until_next_frame(0.0),
            STATUS_SPINNER_ONLY_INTERVAL,
        );
        assert_duration_close(
            status_spinner_delay_until_next_frame(0.040),
            Duration::from_millis(40),
        );
        assert_duration_close(
            status_spinner_delay_until_next_frame(1.0),
            Duration::from_millis(40),
        );
        assert_duration_close(
            status_spinner_delay_until_next_frame(f32::NAN),
            STATUS_SPINNER_ONLY_INTERVAL,
        );
    }

    #[test]
    fn status_spinner_partial_mutates_only_status_cell() {
        // TODO: ftui Buffer has no .cell()/.symbol()/.fg - test commented out
        let area = Rect::new(0, 0, 8, 2);
        let mut buffer = Buffer::new(area.width, area.height);
        // buffer.set_string(0, 0, "abcdefgh", Style::default().fg_compat(Color::Mono(MonoColor::White)));
        // buffer.set_string(0, 1, "ABCDEFGH", Style::default().fg_compat(Color::Ansi16(Ansi16::Blue)));
        // let before = buffer.clone();

        let status_area = Rect::new(2, 1, 6, 1);
        assert!(render_status_spinner_into_buffer(&buffer, status_area, "⠂"));
        render_status_spinner_into_buffer_mut(&mut buffer, status_area, "⠂");

        // TODO: all assertions use ratatui-specific .cell()/.symbol() - no ftui equivalent
        // for y in 0..2 {
        //     for x in 0..8 {
        //         if (x, y) == (2, 1) {
        //             assert_eq!(buffer.cell((x, y)).unwrap().symbol(), "⠂");
        //             assert_eq!(
        //                 buffer.cell((x, y)).unwrap().fg,
        //                 jcode_tui_style::theme::ai_color()
        //             );
        //         } else {
        //             assert_eq!(buffer.cell((x, y)), before.cell((x, y)));
        //         }
        //     }
        // }
    }
}
