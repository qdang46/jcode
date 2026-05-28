// Phase 5 - workspace & panes: implementation using frankentui panes
use crate::workspace_map::{VisibleWorkspaceRow, WorkspaceSessionVisualState};
use ftui_core::geometry::Rect;
use ftui_render::buffer::Buffer;
use ftui_render::cell::{Cell, CellAttrs, PackedRgba, StyleFlags};

const TILE_WIDTH: u16 = 1;
const TILE_HEIGHT: u16 = 1;
const COL_GAP: u16 = 1;
const ROW_GAP: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceTilePlacement {
    pub workspace: i32,
    pub session_index: usize,
    pub rect: Rect,
    pub focused: bool,
    pub current_workspace: bool,
    pub state: WorkspaceSessionVisualState,
}

/// Compute the preferred size for a workspace map given the rows.
/// Returns (width, height) in cells.
pub fn preferred_size(rows: &[VisibleWorkspaceRow]) -> (u16, u16) {
    let max_tiles = rows.iter().map(|row| row.sessions.len()).max().unwrap_or(0) as u16;
    let width = if max_tiles == 0 {
        TILE_WIDTH
    } else {
        max_tiles * TILE_WIDTH + max_tiles.saturating_sub(1) * COL_GAP
    };
    let height = rows.len() as u16 * TILE_HEIGHT + rows.len().saturating_sub(1) as u16 * ROW_GAP;
    (width, height)
}

/// Compute tile placements for all sessions in all visible rows.
/// Returns a vector of tile placements with computed rects and state info.
pub fn compute_workspace_tile_placements(
    area: Rect,
    rows: &[VisibleWorkspaceRow],
) -> Vec<WorkspaceTilePlacement> {
    let mut placements = Vec::new();

    for (row_idx, row) in rows.iter().enumerate() {
        for (session_idx, session) in row.sessions.iter().enumerate() {
            let x = session_idx as u16 * (TILE_WIDTH + COL_GAP);
            let y = row_idx as u16 * (TILE_HEIGHT + ROW_GAP);

            // Clamp to area bounds
            if x >= area.width || y >= area.height {
                continue;
            }

            let rect = Rect::new(
                area.x + x,
                area.y + y,
                TILE_WIDTH.min(area.width.saturating_sub(x)),
                TILE_HEIGHT.min(area.height.saturating_sub(y)),
            );

            let is_current_workspace = row.active_session_index == Some(session_idx);

            placements.push(WorkspaceTilePlacement {
                workspace: row_idx as i32,
                session_index: session_idx,
                rect,
                focused: false,
                current_workspace: is_current_workspace,
                state: session.state,
            });
        }
    }

    placements
}

fn state_color(state: WorkspaceSessionVisualState) -> PackedRgba {
    match state {
        WorkspaceSessionVisualState::Idle => PackedRgba::rgb(127, 127, 127),     // Dim gray
        WorkspaceSessionVisualState::Running => PackedRgba::rgb(0, 200, 0),     // Green
        WorkspaceSessionVisualState::Completed => PackedRgba::rgb(0, 0, 205),    // Blue
        WorkspaceSessionVisualState::Waiting => PackedRgba::rgb(205, 205, 0),    // Yellow
        WorkspaceSessionVisualState::Error => PackedRgba::rgb(205, 0, 0),        // Red
        WorkspaceSessionVisualState::Detached => PackedRgba::rgb(128, 128, 128), // Darker gray
    }
}

/// Render the workspace map widget into the buffer.
/// Draws colored tiles for each session in each visible workspace row.
pub fn render_workspace_map(
    buf: &mut Buffer,
    area: Rect,
    rows: &[VisibleWorkspaceRow],
    _animation_tick: u64,
) {
    if rows.is_empty() || area.width == 0 || area.height == 0 {
        return;
    }

    // Clear the area first
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = Cell::from_char(' ');
            buf.set(area.x + x, area.y + y, cell);
        }
    }

    // Render each session tile
    for (row_idx, row) in rows.iter().enumerate() {
        for (session_idx, session) in row.sessions.iter().enumerate() {
            let x = session_idx as u16 * (TILE_WIDTH + COL_GAP);
            let y = row_idx as u16 * (TILE_HEIGHT + ROW_GAP);

            // Skip if outside area
            if x >= area.width || y >= area.height {
                continue;
            }

            let color = state_color(session.state);
            let is_active = row.active_session_index == Some(session_idx);

            // Apply subtle styling for active session
            let cell = if is_active {
                Cell::from_char('●')
                    .with_fg(color)
                    .with_attrs(CellAttrs::new(StyleFlags::BOLD, 0))
            } else {
                Cell::from_char('●').with_fg(color)
            };
            buf.set(area.x + x, area.y + y, cell);
        }
    }
}

/// Placeholder function - rendering is done in the TUI layer.
/// Kept for API compatibility.
pub fn render_workspace_map_widget(
    _buf: &mut Buffer,
    _area: Rect,
    _rows: &[VisibleWorkspaceRow],
    _focused_workspace: Option<&str>,
) {
    // Rendering is handled by render_workspace_map in the TUI layer
}
