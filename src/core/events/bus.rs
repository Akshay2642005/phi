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

#[derive(Clone, Debug)]
pub struct EventBus {
    sender: Arc<broadcast::Sender<EngineEvent>>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.sender.subscribe()
    }

    pub fn emit(&self, event: EngineEvent) -> bool {
        self.sender.send(event).is_ok()
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoopEventBus;

impl NoopEventBus {
    #[inline]
    pub fn emit(&self, _event: EngineEvent) -> bool {
        false
    }
}
