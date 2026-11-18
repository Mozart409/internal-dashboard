use std::time::Duration;

use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgListener, PgPoolOptions};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Postgres NOTIFY channel the `links_notify` trigger publishes on.
pub const CHANNEL: &str = "link_events";

/// A change to the `links` table, as seen by every replica.
///
/// The `links_notify` trigger (migration 0004) is the only publisher: it
/// NOTIFYs on [`CHANNEL`] for every insert, update and delete, no matter which
/// surface (UI, REST API, MCP) or replica made the change. [`listen`] turns
/// each notification into one of these and broadcasts it to the local SSE
/// clients.
///
/// The ids are carried but not currently read: the SSE handler re-renders the
/// whole list from the database on any event, which keeps every client
/// consistent regardless of how many events it missed. They are kept so a
/// consumer can move to granular per-row updates without changing publishers.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum LinkEvent {
    Created(Uuid),
    Updated(Uuid),
    Deleted(Uuid),
    /// The listener lost its connection, so notifications may have been missed;
    /// clients re-render the list to catch up.
    Resync,
}

/// The JSON the trigger sends: `{"op": "created", "id": "<uuid>"}`.
#[derive(Deserialize)]
struct Payload {
    op: String,
    id: Uuid,
}

impl LinkEvent {
    /// SSE event name; the HTMX `sse-swap` attribute subscribes to these.
    /// A [`LinkEvent::Resync`] goes out as `updated`, since the handler
    /// re-renders everything for any of them.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            LinkEvent::Created(_) => "created",
            LinkEvent::Updated(_) | LinkEvent::Resync => "updated",
            LinkEvent::Deleted(_) => "deleted",
        }
    }

    /// Parse a NOTIFY payload; `None` if it is not one the trigger would send.
    #[must_use]
    pub fn from_payload(payload: &str) -> Option<Self> {
        let Payload { op, id } = serde_json::from_str(payload).ok()?;
        match op.as_str() {
            "created" => Some(LinkEvent::Created(id)),
            "updated" => Some(LinkEvent::Updated(id)),
            "deleted" => Some(LinkEvent::Deleted(id)),
            _ => None,
        }
    }
}

pub type EventSender = broadcast::Sender<LinkEvent>;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    /// Fan-out to this process's SSE clients, fed by [`listen`].
    pub events: EventSender,
    /// Cancelled on SIGTERM/Ctrl-C: readiness turns 503, the SSE and MCP
    /// streams end, and the listener stops.
    pub shutdown: CancellationToken,
}

impl AppState {
    /// Build state with a fresh broadcast channel and shutdown token.
    #[must_use]
    pub fn new(pool: sqlx::PgPool) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            pool,
            events,
            shutdown: CancellationToken::new(),
        }
    }

    /// Publish an event to the local SSE clients. A send error only means
    /// nobody is listening, which is normal when no browser tab is open, so it
    /// is deliberately ignored.
    fn publish(&self, event: LinkEvent) {
        let _ = self.events.send(event);
    }
}

/// Start relaying Postgres notifications on [`CHANNEL`] into `state.events`.
///
/// `LISTEN` has completed by the time this returns, so every commit after that
/// is seen. The returned task runs until `state.shutdown` is cancelled and
/// survives a lost connection: sqlx reconnects and re-listens on its own, and
/// the notifications missed meanwhile are covered by a [`LinkEvent::Resync`].
///
/// The listener holds its own one-connection pool, outside `state.pool`, so it
/// never takes a slot from `DB_MAX_CONNECTIONS`. `options` lets it reach
/// Postgres directly when the app pool goes through a transaction-pooling proxy
/// such as pgbouncer, where `LISTEN` does not work. The connection is closed
/// when the task ends.
///
/// # Errors
///
/// Returns the sqlx error if the listener cannot connect or `LISTEN` fails.
pub async fn listen(
    state: &AppState,
    options: PgConnectOptions,
) -> Result<tokio::task::JoinHandle<()>, sqlx::Error> {
    // The same pool `PgListener::connect` builds: one connection, kept forever.
    let listen_pool = PgPoolOptions::new()
        .max_connections(1)
        .max_lifetime(None)
        .idle_timeout(None)
        .connect_with(options)
        .await?;
    let mut listener = PgListener::connect_with(&listen_pool).await?;
    listener.listen(CHANNEL).await?;

    let state = state.clone();
    Ok(tokio::spawn(async move {
        'run: loop {
            tokio::select! {
                () = state.shutdown.cancelled() => break 'run,
                received = listener.try_recv() => match received {
                    Ok(Some(notification)) => {
                        if let Some(event) = LinkEvent::from_payload(notification.payload()) {
                            state.publish(event);
                        } else {
                            tracing::warn!(
                                payload = notification.payload(),
                                "ignoring an unparsable link event"
                            );
                        }
                    }
                    // The connection dropped and sqlx has already reconnected
                    // and re-listened; whatever was committed in between is lost.
                    Ok(None) => {
                        tracing::warn!("link event listener reconnected, resyncing clients");
                        state.publish(LinkEvent::Resync);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "link event listener failed, retrying");
                        tokio::select! {
                            () = state.shutdown.cancelled() => break 'run,
                            () = tokio::time::sleep(Duration::from_secs(1)) => {}
                        }
                    }
                },
            }
        }
        drop(listener);
        listen_pool.close().await;
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "7d1f5a0e-3c2b-4e8a-9a51-0b6f2f4c9d10";

    #[test]
    fn from_payload_parses_each_op() {
        let id = Uuid::parse_str(ID).unwrap();
        for (op, expected) in [
            ("created", LinkEvent::Created(id)),
            ("updated", LinkEvent::Updated(id)),
            ("deleted", LinkEvent::Deleted(id)),
        ] {
            let payload = format!(r#"{{"op":"{op}","id":"{ID}"}}"#);
            let event = LinkEvent::from_payload(&payload)
                .unwrap_or_else(|| panic!("{payload} should parse"));
            assert_eq!(event.name(), expected.name());
            assert_eq!(format!("{event:?}"), format!("{expected:?}"));
        }
    }

    #[test]
    fn from_payload_rejects_an_unknown_op() {
        let payload = format!(r#"{{"op":"truncated","id":"{ID}"}}"#);
        assert!(LinkEvent::from_payload(&payload).is_none());
    }

    #[test]
    fn from_payload_rejects_garbage() {
        for garbage in [
            "",
            "not json",
            "{}",
            r#"{"op":"created"}"#,
            r#"{"op":"created","id":"x"}"#,
        ] {
            assert!(
                LinkEvent::from_payload(garbage).is_none(),
                "{garbage:?} should not parse"
            );
        }
    }

    #[test]
    fn a_resync_goes_out_as_updated() {
        assert_eq!(LinkEvent::Resync.name(), "updated");
    }
}
