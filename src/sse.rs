use std::convert::Infallible;

use axum::{
    Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use futures::stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::events::AppState;

/// SSE endpoint that pushes link updates to connected clients.
///
/// On each broadcast event, re-queries the full link list and renders it as HTML,
/// emitting an SSE event with the event name (created/updated/deleted) and rendered markup.
/// Broadcast receive errors (lagged subscribers) are gracefully skipped.
///
/// # Errors
///
/// Never produces an error to the client; all failures are silently skipped.
pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl futures::stream::Stream<Item = Result<Event, Infallible>>> {
    let pool = state.pool.clone();
    let rx = state.events.subscribe();

    // `filter_map` wants a future yielding Option, not an Option of a future,
    // so the async block is unconditional and the skips happen inside it via `?`.
    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        let pool = pool.clone();
        async move {
            // A lagged receiver just misses a frame; the next event resyncs the
            // whole list anyway, so dropping it is safe.
            let event = result.ok()?;
            let links = crate::db::list_links(&pool, None).await.ok()?;

            // Strip newlines to satisfy the SSE line protocol: data frames must
            // not contain literal newlines without per-line framing.
            let html = crate::ui::render_link_list(&links)
                .into_string()
                .replace(['\n', '\r'], "");

            Some(Ok::<Event, Infallible>(
                Event::default().event(event.name()).data(html),
            ))
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Router exposing the SSE endpoint at `GET /events`.
pub fn router() -> Router<AppState> {
    Router::new().route("/events", get(events))
}
