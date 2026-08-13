use tokio::sync::broadcast;
use uuid::Uuid;

use crate::models::Link;

/// Broadcast to every connected SSE client whenever a link changes, no matter
/// which surface (UI, REST API or MCP) made the change.
#[derive(Debug, Clone)]
pub enum LinkEvent {
    Created(Link),
    Updated(Link),
    Deleted(Uuid),
}

impl LinkEvent {
    /// SSE event name; the HTMX `sse-swap` attribute subscribes to these.
    pub fn name(&self) -> &'static str {
        match self {
            LinkEvent::Created(_) => "created",
            LinkEvent::Updated(_) => "updated",
            LinkEvent::Deleted(_) => "deleted",
        }
    }
}

pub type EventSender = broadcast::Sender<LinkEvent>;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub events: EventSender,
}

impl AppState {
    pub fn new(pool: sqlx::PgPool) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self { pool, events }
    }

    /// Publish an event. A send error only means nobody is listening, which is
    /// normal when no browser tab is open, so it is deliberately ignored.
    pub fn publish(&self, event: LinkEvent) {
        let _ = self.events.send(event);
    }
}
