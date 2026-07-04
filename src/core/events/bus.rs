//! Event bus — distributes [`EngineEvent`]s to zero or more subscribers.
//!
//! # Current implementation
//! Backed by a `tokio::sync::broadcast` channel.  The bus is cheaply
//! cloneable (`Arc`-backed) so it can be passed to the engine, executor,
//! and individual skills without lifetime concerns.
//!
//! # Backpressure
//! `broadcast` drops events if a slow receiver falls behind the channel
//! capacity.  For production use consider replacing with a bounded
//! `mpsc` channel per subscriber, or an external message broker.
//!
//! # Future extensions
//! - Typed subscriptions: `bus.subscribe_to::<WorkflowCompletedEvent>()`
//! - Persistent listeners registered by name
//! - Async middleware / filters
//! - Dead-letter queue for dropped events

use std::sync::Arc;

use tokio::sync::broadcast;

use super::event_types::EngineEvent;

// ── EventBus ──────────────────────────────────────────────────────────────────

/// A broadcast event bus that distributes [`EngineEvent`]s to all active
/// subscribers.
///
/// Clone freely — all clones share the same underlying channel.
#[derive(Clone, Debug)]
pub struct EventBus {
    sender: Arc<broadcast::Sender<EngineEvent>>,
}

impl EventBus {
    /// Create a new bus with the given channel capacity.
    ///
    /// `capacity` is the maximum number of events buffered before the oldest
    /// is dropped for lagging receivers.  128–256 is a sensible default for
    /// interactive workflows; increase for high-throughput scenarios.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Subscribe to all events.
    ///
    /// Returns a [`broadcast::Receiver`] that yields every event emitted
    /// after the subscription point.  Events emitted before subscription
    /// are not delivered.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.sender.subscribe()
    }

    /// Emit an event to all current subscribers.
    ///
    /// If there are no subscribers, the event is silently dropped — this is
    /// intentional.  The bus is fire-and-forget; callers must not rely on
    /// delivery guarantees.
    ///
    /// Returns `true` if at least one receiver got the event.
    pub fn emit(&self, event: EngineEvent) -> bool {
        self.sender.send(event).is_ok()
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

// ── No-op bus ─────────────────────────────────────────────────────────────────

/// A zero-overhead event bus that discards all events.
///
/// Use in unit tests or when observability is not needed.  Avoids allocating
/// a channel entirely.
#[derive(Clone, Debug, Default)]
pub struct NoopEventBus;

impl NoopEventBus {
    /// Discard the event.
    #[inline]
    pub fn emit(&self, _event: EngineEvent) -> bool {
        false
    }
}
