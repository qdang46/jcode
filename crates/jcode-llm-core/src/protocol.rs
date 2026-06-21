use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::schema::{LlmRequest, Usage};

/// Output of a single protocol step.
///
/// The protocol implementation drives a state-machine-style iteration: it may
/// emit zero or more protocol-level events (`Events`), signal that it needs
/// more data from the wire (`NeedMore`), or finish (`Done`) with an optional
/// reason and accumulated usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepOutput<E> {
    /// Zero or more protocol-level events produced during this step.
    Events(Vec<E>),
    /// The protocol needs more data before it can advance.
    NeedMore,
    /// The protocol has finished processing its input.
    Done {
        /// Human-readable reason for finishing (e.g. "stop", "max_tokens", "tool_use").
        reason: Option<String>,
        /// Token usage accumulated so far.
        usage: Option<Usage>,
    },
    /// An irrecoverable error occurred.
    Error {
        /// Human-readable error description.
        message: String,
    },
}

/// A wire-level protocol that can convert between LLM request/response types
/// and the byte-level framing used on the wire.
///
/// `B` is the body type used in HTTP/WebSocket requests/responses.
/// `E` is the event type the protocol emits during streaming.
/// `S` is the opaque state the protocol carries across `step()` calls.
#[async_trait]
pub trait Protocol: Send + Sync + 'static {
    /// The type representing the body of an HTTP request or response for this
    /// protocol (e.g. `serde_json::Value`, a specific protobuf type, etc.).
    type Body: Send + 'static;

    /// Protocol-level events emitted during streaming.  These are lower-level
    /// than `crate::schema::LlmEvent` and represent the protocol's own
    /// vocabulary (e.g. `"message_start"`, `"content_block_delta"`).
    type Event: Send + 'static;

    /// Opaque state that the protocol carries between `step()` invocations.
    /// Created inside `body_from_request` and threaded through `step()` calls.
    type State: Send + 'static;

    /// Build an HTTP request body from a high-level `LlmRequest`.
    ///
    /// Returns the serialized body plus the initial state for the streaming
    /// decoder loop.
    fn body_from_request(&self, request: &LlmRequest) -> Result<(Self::Body, Self::State), String>;

    /// Advance the protocol decoder by one step, consuming some input data
    /// (`chunk`) and/or mutating internal state.
    ///
    /// Returns a `StepOutput` describing what happened:
    /// - `Events(v)` — protocol events that should be translated into
    ///   application-level events.
    /// - `NeedMore` — the decoder needs another chunk of data.
    /// - `Done { .. }` — no more events will be produced.
    /// - `Error { .. }` — something went wrong.
    async fn step(&self, state: &mut Self::State, chunk: Option<&[u8]>) -> StepOutput<Self::Event>;
}
