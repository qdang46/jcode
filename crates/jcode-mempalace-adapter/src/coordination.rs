//! Coordination adapter for jcode — bridges mempalace's coordination module
//! into jcode's tool system.
//!
//! Provides access to:
//! - SignalStore (inter-agent messaging)
//! - LeaseStore (ownership leases)
//! - ActionStore (action lifecycle with two-phase claim)
//! - FileReservationStore (file-level locks)
//! - LiveDelivery (message injection at turn boundaries)
//! - SaturationDetector (coordination problem detection)
//! - ArtifactStore (large payload handoff)

use anyhow::Result;
use std::path::Path;

// Re-export mempalace coordination types
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::actions::{ActionStore, ClaimResult};
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::artifacts::{ArtifactStore, Retention};
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::file_reservations::{
    FileReservation, FileReservationStore, ReservationConflict, ReservationMode,
};
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::leases::LeaseStore;
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::live_delivery::{LiveDelivery, PendingDelivery};
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::saturation::{
    SaturationConfig, SaturationDetector, SaturationReport, SaturationSignal,
};
#[cfg(feature = "backend")]
pub use mempalace_core::coordination::signals::SignalStore;
#[cfg(feature = "backend")]
pub use mempalace_core::types::{Action, ActionStatus, Signal, SignalType};

/// Coordination adapter providing access to all coordination stores.
#[cfg(feature = "backend")]
pub struct CoordinationAdapter {
    pub signals: SignalStore,
    pub leases: LeaseStore,
    pub actions: ActionStore,
    pub reservations: FileReservationStore,
    pub live_delivery: LiveDelivery,
    pub saturation: SaturationDetector,
    pub artifacts: ArtifactStore,
}

#[cfg(feature = "backend")]
impl CoordinationAdapter {
    /// Open or create all coordination stores at the given base path.
    pub fn open(base_path: &Path) -> Result<Self> {
        let coord_dir = base_path.join("coordination");
        std::fs::create_dir_all(&coord_dir)?;

        let signals = SignalStore::open(&coord_dir.join("signals.db"))?;
        let leases = LeaseStore::open(&coord_dir.join("leases.db"))?;
        let actions = ActionStore::open(&coord_dir.join("actions.db"))?;
        let reservations = FileReservationStore::open(&coord_dir.join("reservations.db"))?;
        let live_delivery = LiveDelivery::new();
        let saturation = SaturationDetector::with_defaults();
        let artifacts = ArtifactStore::new(base_path.to_path_buf());

        Ok(Self {
            signals,
            leases,
            actions,
            reservations,
            live_delivery,
            saturation,
            artifacts,
        })
    }

    // === Signal operations ===

    /// Send a signal to another agent.
    pub fn send_signal(
        &self,
        from: &str,
        to: &str,
        content: &str,
        signal_type: SignalType,
    ) -> Result<String> {
        let id = format!("sig-{}", uuid::Uuid::new_v4());
        let signal = Signal {
            id: id.clone(),
            from: from.to_string(),
            to: to.to_string(),
            thread_id: None,
            reply_to: None,
            signal_type,
            content: content.to_string(),
            metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
            read_at: None,
            expires_at: None,
        };
        self.signals.send(&signal)?;

        // Queue for live delivery
        self.live_delivery.queue(&signal)?;

        Ok(id)
    }

    /// Read signals for an agent.
    pub fn read_signals(&self, agent_id: &str, unread_only: bool) -> Result<Vec<Signal>> {
        self.signals.read_signals(agent_id, unread_only, None, None)
    }

    // === Live delivery ===

    /// Poll for pending deliveries for an agent.
    pub fn poll_delivery(&self, agent_id: &str) -> Vec<PendingDelivery> {
        self.live_delivery.poll(agent_id)
    }

    /// Acknowledge delivery of a signal.
    pub fn ack_delivery(&self, signal_id: &str) -> Result<()> {
        self.live_delivery.ack(signal_id)
    }

    // === Action operations ===

    /// Create a new action.
    pub fn create_action(&self, action: &Action) -> Result<()> {
        self.actions.create_action(action)
    }

    /// Two-phase claim an action.
    pub fn claim_action(&self, action_id: &str, agent_id: &str) -> Result<ClaimResult> {
        self.actions.claim_action(action_id, agent_id)
    }

    /// Update action status.
    pub fn update_action_status(&self, action_id: &str, status: ActionStatus) -> Result<()> {
        self.actions.update_action_status(action_id, status)
    }

    // === File reservations ===

    /// Reserve a file for exclusive/shared access.
    pub fn reserve_file(
        &self,
        path_pattern: &str,
        agent_id: &str,
        mode: ReservationMode,
        reason: Option<&str>,
        ttl_minutes: i64,
    ) -> Result<FileReservation> {
        self.reservations
            .acquire(path_pattern, agent_id, mode, reason, ttl_minutes)
    }

    /// Check for file conflicts.
    pub fn check_file_conflict(
        &self,
        path_pattern: &str,
        agent_id: &str,
        mode: ReservationMode,
    ) -> Result<ReservationConflict> {
        self.reservations
            .check_conflict(path_pattern, agent_id, mode)
    }

    /// Release a file reservation.
    pub fn release_file(&self, reservation_id: &str) -> Result<()> {
        self.reservations.release(reservation_id)
    }

    // === Saturation ===

    /// Check for coordination saturation signals.
    pub fn check_saturation(
        &self,
        events: &[mempalace_core::coordination::saturation::CoordinationEvent],
        now_ms: u64,
    ) -> SaturationReport {
        self.saturation.analyze(events, now_ms)
    }

    // === Artifacts ===

    /// Store a large payload as an artifact.
    pub fn store_artifact(
        &self,
        category: &str,
        entity_id: &str,
        content: &str,
        retention: Retention,
    ) -> Result<String> {
        self.artifacts
            .store(category, entity_id, content, retention)
    }

    /// Read an artifact.
    pub fn read_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<mempalace_core::coordination::artifacts::Artifact>> {
        self.artifacts.read(artifact_id)
    }
}
