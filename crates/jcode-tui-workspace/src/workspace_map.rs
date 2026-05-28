// Phase 5 - workspace & panes: stubbed for Phase 1.3 compilation
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
}

impl VisibleWorkspaceRow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            sessions: Vec::new(),
            active_session_index: None,
            is_visible: true,
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
pub struct WorkspaceMapModel;
