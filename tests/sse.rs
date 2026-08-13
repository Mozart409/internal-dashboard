//! The live-update contract.
//!
//! Every surface publishes into one broadcast channel, so a change made through
//! any of them has to reach every connected browser. These tests drive the real
//! router over real sockets and read `/events` incrementally.
//!
//! Every read is bounded by a timeout, so a missing event fails the test
//! instead of hanging the suite forever.

mod common;

use std::pin::Pin;
use std::time::Duration;

use futures::{Stream, StreamExt as _};
use sqlx::PgPool;
use tokio::time::timeout;

/// Budget for a single frame. Generous enough for a loaded machine, short
/// enough that a lost event fails fast.
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// One parsed SSE frame.
struct Frame {
    /// The `event:` field — `created`, `updated` or `deleted`.
    name: String,
    /// The `data:` field, with multiple `data:` lines joined by newlines the
    /// way the SSE spec prescribes.
    data: String,
    /// How many `data:` lines the frame spanned. The handler strips newlines
    /// out of the rendered markup precisely so that this stays 1.
    data_lines: usize,
}

type ByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<Vec<u8>>> + Send>>;

/// Incremental reader over a chunked `text/event-stream` body.
struct Frames {
    stream: ByteStream,
    buf: Vec<u8>,
}

impl Frames {
    /// Next frame, or a panic naming what the caller was waiting for.
    async fn next_frame(&mut self, expecting: &str) -> Frame {
        let read = async {
            loop {
                if let Some(frame) = self.take_buffered_frame() {
                    return frame;
                }
                let Some(chunk) = self.stream.next().await else {
                    panic!("SSE stream closed while waiting for {expecting}");
                };
                self.buf
                    .extend_from_slice(&chunk.expect("SSE body chunk should read cleanly"));
            }
        };

        timeout(FRAME_TIMEOUT, read).await.unwrap_or_else(|_| {
            panic!("no SSE frame arrived within {FRAME_TIMEOUT:?} while waiting for {expecting}")
        })
    }

    /// Pull one complete frame out of the buffer, discarding keep-alive
    /// comments (`:\n\n`), which carry neither an event name nor data.
    fn take_buffered_frame(&mut self) -> Option<Frame> {
        loop {
            let end = self.buf.windows(2).position(|w| w == b"\n\n")?;
            let block =
                String::from_utf8(self.buf[..end].to_vec()).expect("SSE frames should be UTF-8");
            self.buf.drain(..end + 2);

            if let Some(frame) = parse_frame(&block) {
                return Some(frame);
            }
        }
    }
}

/// Parse one `\n\n`-delimited block. Returns `None` for comment-only blocks.
fn parse_frame(block: &str) -> Option<Frame> {
    let mut name = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in block.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix("event:") {
            name = Some(rest.trim_start().to_owned());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }

    if name.is_none() && data_lines.is_empty() {
        return None;
    }

    Some(Frame {
        name: name.unwrap_or_default(),
        data: data_lines.join("\n"),
        data_lines: data_lines.len(),
    })
}

/// Open `/events` and return a reader positioned before the first frame.
///
/// The handler subscribes to the broadcast channel before it can return a
/// response, so by the time this function returns the subscription is live and
/// the caller cannot miss an event it triggers next.
async fn subscribe(app: &common::TestApp) -> Frames {
    let res = app
        .client
        .get(app.url("/events"))
        .send()
        .await
        .expect("GET /events");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "/events should accept subscribers"
    );

    Frames {
        stream: Box::pin(res.bytes_stream().map(|chunk| chunk.map(|b| b.to_vec()))),
        buf: Vec::new(),
    }
}

/// PUT a partial update and assert the API accepted it.
async fn put_link(app: &common::TestApp, id: uuid::Uuid, body: &serde_json::Value) {
    let res = app
        .client
        .put(app.url(&format!("/api/v1/links/{id}")))
        .json(body)
        .send()
        .await
        .expect("PUT /api/v1/links/{id}");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "setup: updating an existing link should return 200"
    );
}

#[sqlx::test]
async fn events_endpoint_serves_an_event_stream(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = app
        .client
        .get(app.url("/events"))
        .send()
        .await
        .expect("GET /events");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "GET /events should return 200"
    );

    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .expect("/events must send a content-type")
        .to_str()
        .expect("content-type should be ASCII");

    assert!(
        content_type.starts_with("text/event-stream"),
        "/events must be an SSE endpoint, got content-type {content_type}"
    );
}

/// The architecture guarantee: a write on a completely separate connection
/// reaches a browser that is already subscribed.
#[sqlx::test]
async fn a_create_on_another_connection_reaches_an_open_subscriber(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut frames = subscribe(&app).await;

    let link = app
        .create_link(
            "https://example.com/cross",
            "Cross client delivery",
            &["rust"],
        )
        .await;

    let frame = frames.next_frame("the created frame").await;

    assert_eq!(
        frame.name, "created",
        "POST /api/v1/links must broadcast an event named `created`"
    );
    assert!(
        frame.data.contains(&link.title),
        "the frame must carry the new link's title, got: {}",
        frame.data
    );
}

#[sqlx::test]
async fn an_update_broadcasts_an_updated_frame(pool: PgPool) {
    let app = common::spawn(pool).await;
    let link = app
        .create_link("https://example.com/u", "Before rename", &["rust"])
        .await;

    let mut frames = subscribe(&app).await;
    put_link(
        &app,
        link.id,
        &serde_json::json!({ "title": "After rename" }),
    )
    .await;

    let frame = frames.next_frame("the updated frame").await;

    assert_eq!(
        frame.name, "updated",
        "PUT /api/v1/links/{{id}} must broadcast an event named `updated`"
    );
    assert!(
        frame.data.contains("After rename"),
        "the updated frame must re-render the list with the new title, got: {}",
        frame.data
    );
}

#[sqlx::test]
async fn a_delete_broadcasts_a_deleted_frame(pool: PgPool) {
    let app = common::spawn(pool).await;
    let link = app
        .create_link("https://example.com/d", "Doomed link", &["rust"])
        .await;

    let mut frames = subscribe(&app).await;
    let res = app
        .client
        .delete(app.url(&format!("/api/v1/links/{}", link.id)))
        .send()
        .await
        .expect("DELETE /api/v1/links/{id}");
    assert_eq!(
        res.status(),
        reqwest::StatusCode::NO_CONTENT,
        "setup: deleting an existing link should return 204"
    );

    let frame = frames.next_frame("the deleted frame").await;

    assert_eq!(
        frame.name, "deleted",
        "DELETE /api/v1/links/{{id}} must broadcast an event named `deleted`"
    );
    assert!(
        !frame.data.contains("Doomed link"),
        "the deleted frame must re-render the list without the removed link, got: {}",
        frame.data
    );
}

/// Each payload replaces `#link-list` via `outerHTML`, so it has to carry the
/// `sse-swap` binding again. Without it live updates would work exactly once
/// and then silently stop.
#[sqlx::test]
async fn every_payload_re_emits_the_sse_swap_binding(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut frames = subscribe(&app).await;

    app.create_link("https://example.com/swap", "Rebind me", &[])
        .await;

    let frame = frames.next_frame("the created frame").await;

    assert!(
        frame.data.contains("sse-swap"),
        "the swapped-in markup must re-emit sse-swap or live updates stop after one swap, got: {}",
        frame.data
    );
    assert!(
        frame.data.contains("hx-swap=\"outerHTML\""),
        "the swapped-in markup must re-emit its outerHTML swap mode, got: {}",
        frame.data
    );
}

/// SSE line protocol: a raw newline inside the rendered HTML would split the
/// frame and truncate the payload, so the handler strips them. A title that
/// itself contains a newline is the sharpest probe of that.
#[sqlx::test]
async fn payload_arrives_as_one_intact_data_line(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut frames = subscribe(&app).await;

    app.create_link("https://example.com/nl", "Multi\nline title", &["a\nb"])
        .await;

    let frame = frames.next_frame("the created frame").await;

    assert_eq!(
        frame.data_lines, 1,
        "newlines in the markup must be stripped, or the frame splits across data: lines; got {} lines: {}",
        frame.data_lines, frame.data
    );
    assert!(
        frame.data.contains("<div id=\"link-list\""),
        "the frame must start with the list container, got: {}",
        frame.data
    );
    assert!(
        frame.data.contains("</div>"),
        "the frame must contain a closing tag, proving it was not truncated mid-HTML, got: {}",
        frame.data
    );
    assert!(
        frame.data.contains("Multi") && frame.data.contains("line title"),
        "both halves of a newline-containing title must survive in one frame, got: {}",
        frame.data
    );
}

#[sqlx::test]
async fn two_concurrent_subscribers_receive_the_same_event(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut first = subscribe(&app).await;
    let mut second = subscribe(&app).await;

    app.create_link("https://example.com/fanout", "Fan out", &["rust"])
        .await;

    let a = first
        .next_frame("the first subscriber's created frame")
        .await;
    let b = second
        .next_frame("the second subscriber's created frame")
        .await;

    assert_eq!(a.name, "created", "the first subscriber must be notified");
    assert_eq!(b.name, "created", "the second subscriber must be notified");
    assert_eq!(
        a.data, b.data,
        "every subscriber must receive the identical re-rendered list"
    );
}
