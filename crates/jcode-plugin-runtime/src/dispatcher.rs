use crate::gate::{ApprovalGate, GateDecision};
use crate::types::HandlerSlot;
use futures::future::join_all;
use jcode_plugin_core::PluginEvent;
use jcode_plugin_core::ToolTier;
use jcode_plugin_core::events::{EventInput, EventOutput, HandlerResult};
use jcode_plugin_core::types::PluginId;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone)]
struct HandlerBitmap(u128);

impl HandlerBitmap {
    fn new() -> Self {
        Self(0)
    }
    fn set(&mut self, event: PluginEvent) {
        self.0 |= 1u128 << (event as u32);
    }
    fn has(&self, event: PluginEvent) -> bool {
        (self.0 & (1u128 << (event as u32))) != 0
    }
    fn _clear(&mut self, event: PluginEvent) {
        self.0 &= !(1u128 << (event as u32));
    }
    fn rebuild(handlers: &[(PluginEvent, PluginId, HandlerSlot)]) -> Self {
        let mut bm = Self(0);
        for (event, _, _) in handlers {
            bm.set(*event);
        }
        bm
    }
}

#[derive(Clone)]
struct RegistrySnapshot {
    bitmap: HandlerBitmap,
    handlers: Vec<(PluginEvent, PluginId, HandlerSlot)>,
}

pub struct RcuDispatcher {
    snapshot: RwLock<Arc<RegistrySnapshot>>,
    pending: Mutex<Vec<(PluginEvent, PluginId, HandlerSlot)>>,
    approval_gate: RwLock<Option<ApprovalGate>>,
}

#[allow(clippy::new_without_default)]
impl RcuDispatcher {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(Arc::new(RegistrySnapshot {
                bitmap: HandlerBitmap::new(),
                handlers: Vec::new(),
            })),
            pending: Mutex::new(Vec::new()),
            approval_gate: RwLock::new(None),
        }
    }

    pub fn register(&self, event: PluginEvent, id: PluginId, slot: HandlerSlot) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push((event, id, slot));
        }
    }

    pub fn commit(&self) {
        // Drain pending first, then build the new snapshot, then publish.
        // This avoids holding the snapshot lock while acquiring the write lock.
        let to_commit: Vec<(PluginEvent, PluginId, HandlerSlot)> = {
            let Ok(mut pending) = self.pending.lock() else {
                return;
            };
            if pending.is_empty() {
                return;
            }
            pending.drain(..).collect()
        };

        // Get the current handlers (read lock — released before write lock)
        let current_handlers = {
            let Ok(current) = self.snapshot.read() else {
                return;
            };
            current.handlers.clone()
        };

        let mut new_handlers = current_handlers;
        new_handlers.extend(to_commit);
        let new_bitmap = HandlerBitmap::rebuild(&new_handlers);

        let Ok(mut snapshot) = self.snapshot.write() else {
            return;
        };
        *snapshot = Arc::new(RegistrySnapshot {
            bitmap: new_bitmap,
            handlers: new_handlers,
        });
    }

    pub fn has_handler(&self, event: PluginEvent) -> bool {
        if let Ok(snapshot) = self.snapshot.read() {
            snapshot.bitmap.has(event)
        } else {
            false
        }
    }

    /// Dispatch an event to all registered handlers.
    ///
    /// Uses `join_all` for concurrent dispatch. Each handler receives
    /// a clone of the input/output and returns its own HandlerResult.
    pub async fn dispatch(
        &self,
        event: PluginEvent,
        input: EventInput,
        output: Option<EventOutput>,
    ) -> Vec<(PluginId, HandlerResult)> {
        // RCU: clone the Arc for zero-contention reads
        let snapshot = if let Ok(s) = self.snapshot.read() {
            s.clone()
        } else {
            return Vec::new();
        };

        // O(1) bitmap check — fast path when no handlers exist
        if !snapshot.bitmap.has(event) {
            return Vec::new();
        }

        let handlers: Vec<_> = snapshot
            .handlers
            .iter()
            .filter(|(e, _, _)| *e == event)
            .map(|(_, id, slot)| (id.clone(), slot.clone()))
            .collect();

        if handlers.is_empty() {
            return Vec::new();
        }

        // Dispatch via join_all — each handler gets a clone of the input
        let futures: Vec<_> = handlers
            .into_iter()
            .map(|(id, slot)| {
                let inp = input.clone();
                let out = output.clone();
                async move {
                    let result = match slot {
                        HandlerSlot::Rust(handler) => handler(inp, out).await,
                    };
                    (id, result)
                }
            })
            .collect();

        join_all(futures).await
    }

    pub fn unregister_plugin(&self, id: &PluginId) {
        // Get current handlers (read lock — released before write lock)
        let current_handlers = {
            let Ok(current) = self.snapshot.read() else {
                return;
            };
            current.handlers.clone()
        };

        let new_handlers: Vec<_> = current_handlers
            .into_iter()
            .filter(|(_, pid, _)| pid != id)
            .collect();
        let new_bitmap = HandlerBitmap::rebuild(&new_handlers);

        let Ok(mut snapshot) = self.snapshot.write() else {
            return;
        };
        *snapshot = Arc::new(RegistrySnapshot {
            bitmap: new_bitmap,
            handlers: new_handlers,
        });
    }

    pub fn handler_count(&self) -> usize {
        if let Ok(snapshot) = self.snapshot.read() {
            snapshot.handlers.len()
        } else {
            0
        }
    }

    pub fn plugin_count(&self) -> usize {
        if let Ok(snapshot) = self.snapshot.read() {
            let mut ids: Vec<&PluginId> = snapshot.handlers.iter().map(|(_, id, _)| id).collect();
            ids.sort_by_key(|id| id.to_string());
            ids.dedup();
            ids.len()
        } else {
            0
        }
    }

    /// Install or replace the [`ApprovalGate`] used to check tool calls.
    pub fn set_approval_gate(&self, gate: ApprovalGate) {
        if let Ok(mut lock) = self.approval_gate.write() {
            *lock = Some(gate);
        }
    }

    /// Remove the approval gate (disables gate checks).
    pub fn clear_approval_gate(&self) {
        if let Ok(mut lock) = self.approval_gate.write() {
            *lock = None;
        }
    }

    /// Check a tool call through the approval gate (if one is installed).
    ///
    /// Returns `None` if no gate is installed (allows the call), or the
    /// [`GateDecision`] if a gate is present.
    pub fn check_tool(
        &self,
        tool_name: &str,
        tier: ToolTier,
        args: &serde_json::Value,
    ) -> Option<GateDecision> {
        let Ok(lock) = self.approval_gate.read() else {
            return None;
        };
        lock.as_ref().map(|gate| gate.check(tool_name, tier, args))
    }
}
