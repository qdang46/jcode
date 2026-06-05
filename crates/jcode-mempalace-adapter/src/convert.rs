// =====================================================================
// convert — bidirectional type conversions between jcode and mempalace
// =====================================================================
//
// This module defines **local mirror types** that match mempalace's
// `Drawer` / `DrawerKind` / `MemoryScope` shapes exactly, so the
// conversion layer has zero dependency on mempalace-core (avoiding
// the rusqlite 0.32 vs 0.33 link conflict).  When the full backend
// integration lands, these mirrors will be replaced with the real
// types behind a feature flag.

use chrono::{DateTime, Utc};
use jcode_memory_types::{MemoryCategory, MemoryEntry, Reinforcement, TrustLevel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---- Local mirror types (match mempalace's public surface) -----------

/// Mirror of mempalace's `DrawerKind` enum.
///
/// Matches `crates/core/src/palace.rs` exactly (including the new
/// `Entity`, `Correction`, `Custom(String)` variants from issue #28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrawerKind {
    Fact,
    Event,
    Discovery,
    Preference,
    Advice,
    Raw,
    Entity,
    Correction,
    Custom(String),
}

impl DrawerKind {
    /// Category-specific confidence-decay half-life in days.
    /// Matches mempalace's `DrawerKind::half_life_days()` (issue #28).
    pub fn half_life_days(&self) -> f64 {
        match self {
            DrawerKind::Correction => 365.0,
            DrawerKind::Preference => 90.0,
            DrawerKind::Entity => 60.0,
            DrawerKind::Fact => 30.0,
            DrawerKind::Custom(_) => 45.0,
            DrawerKind::Event | DrawerKind::Discovery | DrawerKind::Advice | DrawerKind::Raw => {
                30.0
            }
        }
    }
}

/// Mirror of mempalace's `MemoryScope` enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryScope {
    Local,
    Global,
    Auto,
    All,
    Wing(String),
    Room { wing: String, room: String },
}

/// Mirror of mempalace's `DrawerId` (newtype around String).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DrawerId(pub String);

/// Mirror of mempalace's `Reinforcement` struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpReinforcement {
    pub session_id: String,
    pub message_index: usize,
    pub timestamp: DateTime<Utc>,
}

/// Mirror of mempalace's `Drawer` struct.
///
/// Fields match `crates/core/src/palace.rs` Drawer struct exactly,
/// including the new typed fields from issues #25-#27.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drawer {
    pub id: Option<DrawerId>,
    pub content: String,
    pub kind: DrawerKind,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active: bool,
    pub trust: Option<String>,
    pub access_count: u64,
    pub superseded_by: Option<DrawerId>,
    pub reinforcements: Vec<MpReinforcement>,
    pub confidence: f64,
    pub consolidation_strength: u32,
    pub derived_from: Vec<DrawerId>,
}

impl Drawer {
    pub fn new(content: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            content: content.into(),
            kind: DrawerKind::Raw,
            tags: vec![],
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
            active: true,
            trust: None,
            access_count: 0,
            superseded_by: None,
            reinforcements: vec![],
            confidence: 1.0,
            consolidation_strength: 1,
            derived_from: vec![],
        }
    }

    pub fn kind(mut self, kind: DrawerKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn confidence(mut self, c: f64) -> Self {
        self.confidence = c;
        self
    }

    pub fn consolidation_strength(mut self, s: u32) -> Self {
        self.consolidation_strength = s;
        self
    }

    pub fn set_metadata(&mut self, key: String, value: serde_json::Value) {
        self.metadata.insert(key, value);
    }
}

// ---- Conversion functions --------------------------------------------

/// Convert jcode's `MemoryCategory` to the mirror `DrawerKind`.
pub fn category_to_kind(cat: &MemoryCategory) -> DrawerKind {
    match cat {
        MemoryCategory::Fact => DrawerKind::Fact,
        MemoryCategory::Preference => DrawerKind::Preference,
        MemoryCategory::Entity => DrawerKind::Entity,
        MemoryCategory::Correction => DrawerKind::Correction,
        MemoryCategory::Custom(s) => DrawerKind::Custom(s.clone()),
    }
}

/// Convert mirror `DrawerKind` back to jcode's `MemoryCategory`.
///
/// Non-jcode kinds (`Event`, `Discovery`, `Advice`, `Raw`) map to
/// `MemoryCategory::Fact` as a safe default.
pub fn kind_to_category(kind: &DrawerKind) -> MemoryCategory {
    match kind {
        DrawerKind::Fact => MemoryCategory::Fact,
        DrawerKind::Preference => MemoryCategory::Preference,
        DrawerKind::Entity => MemoryCategory::Entity,
        DrawerKind::Correction => MemoryCategory::Correction,
        DrawerKind::Custom(s) => MemoryCategory::Custom(s.clone()),
        DrawerKind::Event | DrawerKind::Discovery | DrawerKind::Advice | DrawerKind::Raw => {
            MemoryCategory::Fact
        }
    }
}

/// Convert jcode's `MemoryScope` to the mirror `MemoryScope`.
pub fn mp_scope_from_jcode(scope: jcode_memory_types::MemoryScope) -> MemoryScope {
    match scope {
        jcode_memory_types::MemoryScope::Project => MemoryScope::Local,
        jcode_memory_types::MemoryScope::Global => MemoryScope::Global,
        jcode_memory_types::MemoryScope::All => MemoryScope::All,
    }
}

/// Convert mirror `MemoryScope` back to jcode's.
pub fn jcode_scope_from_mp(scope: &MemoryScope) -> jcode_memory_types::MemoryScope {
    match scope {
        MemoryScope::Local => jcode_memory_types::MemoryScope::Project,
        MemoryScope::Global => jcode_memory_types::MemoryScope::Global,
        MemoryScope::All => jcode_memory_types::MemoryScope::All,
        _ => jcode_memory_types::MemoryScope::All,
    }
}

/// Convert jcode's `TrustLevel` to a string.
pub fn trust_to_string(t: &TrustLevel) -> String {
    match t {
        TrustLevel::High => "high".to_string(),
        TrustLevel::Medium => "medium".to_string(),
        TrustLevel::Low => "low".to_string(),
    }
}

/// Parse a trust string back into a `TrustLevel`.
pub fn string_to_trust(s: &str) -> TrustLevel {
    match s.to_lowercase().as_str() {
        "high" => TrustLevel::High,
        "low" => TrustLevel::Low,
        _ => TrustLevel::Medium,
    }
}

/// Convert a jcode `MemoryEntry` into the mirror `Drawer`.
pub fn memory_entry_to_drawer(
    entry: &MemoryEntry,
    scope: jcode_memory_types::MemoryScope,
) -> Drawer {
    let kind = category_to_kind(&entry.category);
    let mut drawer = Drawer::new(entry.content.clone())
        .kind(kind)
        .tags(entry.tags.clone())
        .confidence(entry.confidence as f64)
        .consolidation_strength(entry.strength);

    drawer.id = Some(DrawerId(entry.id.clone()));
    drawer.created_at = entry.created_at;
    drawer.updated_at = entry.updated_at;
    drawer.active = entry.active;
    drawer.trust = Some(trust_to_string(&entry.trust));
    drawer.access_count = entry.access_count as u64;
    drawer.superseded_by = entry.superseded_by.as_ref().map(|s| DrawerId(s.clone()));

    drawer.reinforcements = entry
        .reinforcements
        .iter()
        .map(|r| MpReinforcement {
            session_id: r.session_id.clone(),
            message_index: r.message_index,
            timestamp: r.timestamp,
        })
        .collect();

    // Metadata fields that don't have first-class slots
    if let Some(ref source) = entry.source {
        drawer.set_metadata("source".to_string(), serde_json::json!(source));
    }
    if let Some(ref emb) = entry.embedding {
        drawer.set_metadata("jcode_embedding".to_string(), serde_json::json!(emb));
    }
    drawer.set_metadata(
        "jcode_search_text".to_string(),
        serde_json::json!(&entry.search_text),
    );
    drawer.set_metadata(
        "jcode_scope".to_string(),
        serde_json::json!(match scope {
            jcode_memory_types::MemoryScope::Project => "project",
            jcode_memory_types::MemoryScope::Global => "global",
            jcode_memory_types::MemoryScope::All => "all",
        }),
    );

    drawer
}

/// Convert a mirror `Drawer` back into a jcode `MemoryEntry`.
pub fn drawer_to_memory_entry(drawer: &Drawer) -> MemoryEntry {
    let category = kind_to_category(&drawer.kind);
    let trust = drawer
        .trust
        .as_deref()
        .map(string_to_trust)
        .unwrap_or_default();
    let source = drawer
        .metadata
        .get("source")
        .and_then(|v| v.as_str())
        .map(String::from);
    let search_text = drawer
        .metadata
        .get("jcode_search_text")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    let embedding = drawer
        .metadata
        .get("jcode_embedding")
        .and_then(|v| serde_json::from_value::<Vec<f32>>(v.clone()).ok());

    MemoryEntry {
        id: drawer.id.as_ref().map(|d| d.0.clone()).unwrap_or_default(),
        category,
        content: drawer.content.clone(),
        tags: drawer.tags.clone(),
        search_text,
        created_at: drawer.created_at,
        updated_at: drawer.updated_at,
        access_count: drawer.access_count as u32,
        source,
        trust,
        strength: drawer.consolidation_strength,
        active: drawer.active,
        superseded_by: drawer.superseded_by.as_ref().map(|d| d.0.clone()),
        reinforcements: drawer
            .reinforcements
            .iter()
            .map(|r| Reinforcement {
                session_id: r.session_id.clone(),
                message_index: r.message_index,
                timestamp: r.timestamp,
            })
            .collect(),
        embedding,
        confidence: drawer.confidence as f32,
    }
}
