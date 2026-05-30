// Phase 5 - workspace & panes: implementation
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceSessionVisualState {
    #[default]
    Idle,
    Running,
    Completed,
    Waiting,
    Error,
    Detached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSessionTile {
    pub session_id: String,
    pub state: WorkspaceSessionVisualState,
}

impl WorkspaceSessionTile {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            state: WorkspaceSessionVisualState::Idle,
        }
    }
    pub fn with_state(session_id: impl Into<String>, state: WorkspaceSessionVisualState) -> Self {
        Self {
            session_id: session_id.into(),
            state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceRow {
    pub sessions: Vec<WorkspaceSessionTile>,
    pub last_focused: Option<usize>,
}

impl WorkspaceRow {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
    pub fn len(&self) -> usize {
        self.sessions.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisibleWorkspaceRow {
    pub id: String,
    pub name: String,
    pub sessions: Vec<WorkspaceSessionTile>,
    pub active_session_index: Option<usize>,
    pub is_visible: bool,
    /// Focused index within this row (used by tests)
    pub focused_index: Option<usize>,
}

impl VisibleWorkspaceRow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            sessions: Vec::new(),
            active_session_index: None,
            is_visible: true,
            focused_index: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceMap {
    pub workspaces: BTreeMap<String, WorkspaceRow>,
}

impl WorkspaceMap {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceMapModel {
    workspaces: BTreeMap<String, WorkspaceRow>,
    current_workspace: i32,
    focused_sessions: BTreeMap<String, usize>, // session_id -> workspace index
}

impl WorkspaceMapModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    pub fn focus_session_by_id(&mut self, session_id: &str) -> bool {
        // Find which workspace has this session
        let target_key = match self
            .workspaces
            .iter()
            .find(|(_, row)| row.sessions.iter().any(|t| t.session_id == session_id))
            .map(|(k, _)| k.clone()) {
            Some(k) => k,
            None => return false,
        };

        if let Some(row) = self.workspaces.get_mut(&target_key) {
            if let Some(idx) = row.sessions.iter().position(|t| t.session_id == session_id) {
                row.last_focused = Some(idx);
                self.current_workspace = self
                    .workspaces
                    .keys()
                    .position(|k| k == &target_key)
                    .unwrap_or(0) as i32;
                return true;
            }
        }
        false
    }

    pub fn visible_rows(&self, max_rows: usize) -> Vec<VisibleWorkspaceRow> {
        self.workspaces
            .iter()
            .take(max_rows)
            .map(|(key, row)| {
                let active_session_index = row.last_focused;
                VisibleWorkspaceRow {
                    id: key.clone(),
                    name: key.clone(),
                    sessions: row.sessions.clone(),
                    active_session_index,
                    is_visible: true,
                    focused_index: row.last_focused,
                }
            })
            .collect()
    }

    pub fn populated_workspaces(&self) -> Vec<String> {
        self.workspaces
            .iter()
            .filter(|(_, row)| !row.sessions.is_empty())
            .map(|(key, _)| key.clone())
            .collect()
    }

    pub fn current_workspace(&self) -> i32 {
        self.current_workspace
    }

    pub fn set_current_workspace(&mut self, idx: i32) {
        self.current_workspace = idx;
    }

    pub fn move_left(&mut self) -> bool {
        let current_idx = self.current_workspace as usize;
        if current_idx > 0 {
            self.current_workspace = (current_idx - 1) as i32;
            true
        } else {
            false
        }
    }

    pub fn move_right(&mut self) -> bool {
        let current_idx = self.current_workspace as usize;
        let max_idx = self.workspaces.len().saturating_sub(1);
        if current_idx < max_idx {
            self.current_workspace = (current_idx + 1) as i32;
            true
        } else {
            false
        }
    }

    pub fn current_focused_session_id(&self) -> Option<String> {
        let key = self.workspaces.keys().nth(self.current_workspace as usize)?;
        let row = self.workspaces.get(key)?;
        let idx = row.last_focused?;
        row.sessions.get(idx).map(|t| t.session_id.clone())
    }

    pub fn nearest_populated_workspace_above(&self) -> Option<i32> {
        let current = self.current_workspace;
        for i in (0..current).rev() {
            if let Some(key) = self.workspaces.keys().nth(i as usize) {
                if let Some(row) = self.workspaces.get(key) {
                    if !row.sessions.is_empty() {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    pub fn nearest_populated_workspace_below(&self) -> Option<i32> {
        let max = self.workspaces.len() as i32;
        for i in (self.current_workspace + 1)..max {
            if let Some(key) = self.workspaces.keys().nth(i as usize) {
                if let Some(row) = self.workspaces.get(key) {
                    if !row.sessions.is_empty() {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    pub fn focused_session_in_workspace(&self, workspace_idx: i32) -> Option<String> {
        let key = self.workspaces.keys().nth(workspace_idx as usize)?;
        let row = self.workspaces.get(key)?;
        let idx = row.last_focused?;
        row.sessions.get(idx).map(|t| t.session_id.clone())
    }

    pub fn locate_session(&self, session_id: &str) -> Option<(String, usize)> {
        for (key, row) in &self.workspaces {
            if let Some(idx) = row.sessions.iter().position(|t| t.session_id == session_id) {
                return Some((key.clone(), idx));
            }
        }
        None
    }

    pub fn set_row_sessions(
        &mut self,
        row: usize,
        tiles: Vec<WorkspaceSessionTile>,
        focused_index: Option<usize>,
    ) {
        let key = format!("workspace_{}", row);
        let workspace_row = WorkspaceRow {
            sessions: tiles,
            last_focused: focused_index,
        };
        self.workspaces.insert(key, workspace_row);
    }

    pub fn add_session_to_current_workspace(&mut self, tile: WorkspaceSessionTile) {
        let key = format!("workspace_{}", self.current_workspace);
        let row = self.workspaces.entry(key).or_insert_with(WorkspaceRow::default);
        row.sessions.push(tile);
    }

    pub fn insert_session_in_workspace(&mut self, workspace_idx: i32, tile: WorkspaceSessionTile) {
        let key = format!("workspace_{}", workspace_idx);
        let row = self.workspaces.entry(key).or_insert_with(WorkspaceRow::default);
        row.sessions.push(tile);
    }
}
