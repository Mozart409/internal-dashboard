//! The MCP surface, driven exactly the way a real MCP client drives it:
//! `initialize`, `notifications/initialized`, then `tools/list` and
//! `tools/call` over rmcp's streamable-HTTP transport at `/mcp`.
//!
//! Wire shape, discovered against the running server:
//!
//! * every POST answers `200` with `content-type: text/event-stream`;
//! * rmcp emits a priming frame (`data:` empty, plus `id:` and `retry:`)
//!   *before* the JSON-RPC payload, and leaves the stream open afterwards, so
//!   readers must skip empty `data:` lines and must never read to end-of-body;
//! * a tool failure is reported in-band as `result.isError == true` with the
//!   message in `result.content[0].text` — not as a JSON-RPC `error`.
//!
//! Dropping a response before its payload arrives cancels the tool call
//! server-side, so `envelope` always reads the payload out before returning.
//! Every read is bounded by a timeout so a missing reply fails the test rather
//! than hanging the suite.

mod common;

use std::pin::Pin;
use std::time::Duration;

use futures::{Stream, StreamExt as _};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::time::timeout;

/// Budget for one JSON-RPC reply or one SSE frame.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

type ByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<Vec<u8>>> + Send>>;

/// POST one JSON-RPC message to `/mcp`, with the headers the transport requires.
async fn post(app: &common::TestApp, session: Option<&str>, body: &Value) -> reqwest::Response {
    let mut req = app
        .client
        .post(app.url("/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(body);

    if let Some(session) = session {
        req = req.header("mcp-session-id", session);
    }

    req.send().await.expect("POST /mcp")
}

/// Read the JSON-RPC envelope out of a streaming `/mcp` response.
async fn envelope(res: reqwest::Response) -> Value {
    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "an MCP request should be answered with 200"
    );

    let mut stream: ByteStream =
        Box::pin(res.bytes_stream().map(|chunk| chunk.map(|b| b.to_vec())));
    let mut buf: Vec<u8> = Vec::new();

    let read = async {
        loop {
            while let Some(end) = buf.windows(2).position(|w| w == b"\n\n") {
                let block =
                    String::from_utf8(buf[..end].to_vec()).expect("MCP frames should be UTF-8");
                buf.drain(..end + 2);

                for line in block.split('\n') {
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    // Skip rmcp's priming frame, whose `data:` field is empty.
                    if let Some(rest) = line.strip_prefix("data:")
                        && !rest.trim().is_empty()
                    {
                        return serde_json::from_str::<Value>(rest.trim())
                            .expect("the data: line should carry a JSON-RPC message");
                    }
                }
            }

            let Some(chunk) = stream.next().await else {
                panic!("the /mcp stream closed before a JSON-RPC payload arrived");
            };
            buf.extend_from_slice(&chunk.expect("MCP body chunk should read cleanly"));
        }
    };

    timeout(REPLY_TIMEOUT, read)
        .await
        .unwrap_or_else(|_| panic!("no JSON-RPC payload within {REPLY_TIMEOUT:?}"))
}

/// The `result` of a `tools/call`.
struct ToolResult {
    is_error: bool,
    text: String,
}

impl ToolResult {
    /// Assert the call succeeded and return its text content.
    fn expect_ok(self, what: &str) -> String {
        assert!(
            !self.is_error,
            "{what} should have succeeded, but the tool reported an error: {}",
            self.text
        );
        self.text
    }

    /// Assert the call was rejected and return the message, so a caller can
    /// check that the reason is the one it expects.
    fn expect_error(self, what: &str) -> String {
        assert!(
            self.is_error,
            "{what} must surface as a tool error rather than a silent success, got: {}",
            self.text
        );
        self.text
    }
}

/// An initialized MCP session.
struct Session<'a> {
    app: &'a common::TestApp,
    id: String,
    next_rpc_id: i64,
}

impl<'a> Session<'a> {
    /// Run the full handshake: `initialize`, then `notifications/initialized`.
    async fn open(app: &'a common::TestApp) -> Session<'a> {
        let (id, _) = initialize(app).await;

        let res = post(
            app,
            Some(&id),
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await;
        assert_eq!(
            res.status(),
            reqwest::StatusCode::ACCEPTED,
            "the initialized notification should be accepted with 202"
        );

        Session {
            app,
            id,
            next_rpc_id: 100,
        }
    }

    /// Send one request and return its `result`, asserting the reply is a
    /// well-formed JSON-RPC answer to that exact request.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_rpc_id;
        self.next_rpc_id += 1;

        let res = post(
            self.app,
            Some(&self.id),
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .await;
        let envelope = envelope(res).await;

        assert_eq!(
            envelope["id"], id,
            "the reply must answer the request it was sent for, got: {envelope}"
        );
        assert!(
            envelope.get("error").is_none(),
            "{method} failed at the protocol level: {envelope}"
        );

        envelope["result"].clone()
    }

    /// Invoke a tool and unpack rmcp's in-band success/failure reporting.
    async fn call_tool(&mut self, name: &str, arguments: Value) -> ToolResult {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;

        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} should answer with text content, got: {result}"))
            .to_owned();

        ToolResult {
            is_error: result["isError"].as_bool().unwrap_or(false),
            text,
        }
    }
}

/// `initialize` on a fresh connection, returning the session id and the
/// `result` member of the reply.
async fn initialize(app: &common::TestApp) -> (String, Value) {
    let res = post(
        app,
        None,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "internal-dashboard-tests", "version": "1" },
            },
        }),
    )
    .await;

    let session = res
        .headers()
        .get("mcp-session-id")
        .expect("initialize must return an mcp-session-id header")
        .to_str()
        .expect("the session id should be ASCII")
        .to_owned();

    let envelope = envelope(res).await;
    assert!(
        envelope.get("error").is_none(),
        "initialize failed at the protocol level: {envelope}"
    );

    (session, envelope["result"].clone())
}

/// Every link currently in the database, read back through the REST surface.
async fn links_in_db(app: &common::TestApp) -> Vec<Value> {
    let (status, body) = app.get("/api/v1/links").await;
    assert!(status.is_success(), "GET /api/v1/links returned {status}");
    serde_json::from_str(&body).expect("the links endpoint should return a JSON array")
}

/// Open `/events`. The handler subscribes to the broadcast channel before it
/// can respond, so once this returns no later event can be missed.
async fn subscribe(app: &common::TestApp) -> ByteStream {
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

    Box::pin(res.bytes_stream().map(|chunk| chunk.map(|b| b.to_vec())))
}

/// Next SSE frame that carries an event name, as `(name, data)`.
async fn next_event(stream: &mut ByteStream, expecting: &str) -> (String, String) {
    let mut buf: Vec<u8> = Vec::new();

    let read = async {
        loop {
            while let Some(end) = buf.windows(2).position(|w| w == b"\n\n") {
                let block =
                    String::from_utf8(buf[..end].to_vec()).expect("SSE frames should be UTF-8");
                buf.drain(..end + 2);

                let mut name = None;
                let mut data = String::new();
                for line in block.split('\n') {
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    if let Some(rest) = line.strip_prefix("event:") {
                        name = Some(rest.trim_start().to_owned());
                    } else if let Some(rest) = line.strip_prefix("data:") {
                        data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
                    }
                }

                // Keep-alive comments carry no event name; skip them.
                if let Some(name) = name {
                    return (name, data);
                }
            }

            let Some(chunk) = stream.next().await else {
                panic!("SSE stream closed while waiting for {expecting}");
            };
            buf.extend_from_slice(&chunk.expect("SSE body chunk should read cleanly"));
        }
    };

    timeout(REPLY_TIMEOUT, read).await.unwrap_or_else(|_| {
        panic!("no SSE frame within {REPLY_TIMEOUT:?} while waiting for {expecting}")
    })
}

#[sqlx::test]
async fn initialize_identifies_the_server_and_opens_a_session(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (session, result) = initialize(&app).await;

    assert!(
        !session.is_empty(),
        "the mcp-session-id header must carry a usable session id"
    );
    assert_eq!(
        result["serverInfo"]["name"], "internal-dashboard",
        "initialize must identify the server, got: {result}"
    );
}

#[sqlx::test]
async fn tools_list_exposes_every_link_tool(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut mcp = Session::open(&app).await;

    let result = mcp.request("tools/list", json!({})).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .expect("tools/list must return an array of tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("every tool needs a name"))
        .collect();

    for expected in [
        "list_links",
        "search_links",
        "add_link",
        "update_link",
        "delete_link",
    ] {
        assert!(
            names.contains(&expected),
            "tools/list must advertise {expected}, got: {names:?}"
        );
    }
    assert_eq!(
        names.len(),
        5,
        "the MCP surface should expose exactly the five link tools, got: {names:?}"
    );
}

#[sqlx::test]
async fn add_link_through_mcp_reaches_the_database(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut mcp = Session::open(&app).await;

    let created = mcp
        .call_tool(
            "add_link",
            json!({
                "url": "https://example.com/mcp-add",
                "title": "Added over MCP",
                // Deliberately unsorted, mixed case and duplicated: tags are
                // normalized on the way in.
                "tags": ["Rust", "mcp", "rust"],
            }),
        )
        .await
        .expect_ok("add_link with a valid https url");

    let created: Value = serde_json::from_str(&created).expect("add_link returns the created link");

    // The tool's own answer is not proof; read it back through another surface.
    let links = links_in_db(&app).await;
    assert_eq!(
        links.len(),
        1,
        "add_link must persist exactly one link, got: {links:?}"
    );
    assert_eq!(links[0]["id"], created["id"], "the persisted id must match");
    assert_eq!(links[0]["title"], "Added over MCP");
    assert_eq!(
        links[0]["tags"],
        json!(["mcp", "rust"]),
        "add_link must persist its tags normalized: lowercased, sorted, deduped"
    );
}

#[sqlx::test]
async fn update_link_through_mcp_replaces_tags(pool: PgPool) {
    let app = common::spawn(pool).await;
    let link = app
        .create_link("https://example.com/u", "Retag me", &["old"])
        .await;
    let mut mcp = Session::open(&app).await;

    mcp.call_tool(
        "update_link",
        json!({ "id": link.id.to_string(), "tags": ["fresh", "tags"] }),
    )
    .await
    .expect_ok("update_link on an existing link");

    let links = links_in_db(&app).await;
    assert_eq!(
        links[0]["tags"],
        json!(["fresh", "tags"]),
        "update_link must replace the tags rather than append to them"
    );
    assert_eq!(
        links[0]["title"], "Retag me",
        "update_link must leave omitted fields untouched"
    );
}

#[sqlx::test]
async fn delete_link_through_mcp_removes_the_link(pool: PgPool) {
    let app = common::spawn(pool).await;
    let link = app
        .create_link("https://example.com/d", "Doomed", &[])
        .await;
    let mut mcp = Session::open(&app).await;

    mcp.call_tool("delete_link", json!({ "id": link.id.to_string() }))
        .await
        .expect_ok("delete_link on an existing link");

    let links = links_in_db(&app).await;
    assert!(
        links.is_empty(),
        "delete_link must remove the row from the database, still present: {links:?}"
    );
}

#[sqlx::test]
async fn search_links_through_mcp_finds_a_match(pool: PgPool) {
    let app = common::spawn(pool).await;
    app.create_link("https://example.com/hay", "Needle in a haystack", &["find"])
        .await;
    app.create_link("https://example.com/other", "Something else", &[])
        .await;
    let mut mcp = Session::open(&app).await;

    let found = mcp
        .call_tool("search_links", json!({ "query": "Needle" }))
        .await
        .expect_ok("search_links with a matching query");

    let found: Vec<Value> = serde_json::from_str(&found).expect("search_links returns an array");
    assert_eq!(
        found.len(),
        1,
        "search_links must return only the matching link, got: {found:?}"
    );
    assert_eq!(found[0]["title"], "Needle in a haystack");
}

/// Bad input must come back as a tool error. A silent success here would let a
/// model believe it had written something it had not.
#[sqlx::test]
async fn invalid_arguments_surface_as_tool_errors(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut mcp = Session::open(&app).await;

    let rejected_scheme = mcp
        .call_tool(
            "add_link",
            json!({ "url": "ftp://x", "title": "Wrong scheme" }),
        )
        .await
        .expect_error("add_link with a non-http url");
    assert!(
        rejected_scheme.contains("http://") || rejected_scheme.contains("https://"),
        "the rejection should explain the url scheme rule, got: {rejected_scheme}"
    );

    let rejected_uuid = mcp
        .call_tool("delete_link", json!({ "id": "not-a-uuid" }))
        .await
        .expect_error("delete_link with a malformed uuid");
    assert!(
        rejected_uuid.to_lowercase().contains("uuid"),
        "the rejection should name the malformed uuid, got: {rejected_uuid}"
    );

    let missing = "11111111-1111-1111-1111-111111111111";
    let reported_missing = mcp
        .call_tool("delete_link", json!({ "id": missing }))
        .await
        .expect_error("delete_link with a valid but unknown uuid");
    assert!(
        reported_missing.contains("not found") && reported_missing.contains(missing),
        "a missing link should be reported as not found, got: {reported_missing}"
    );

    assert!(
        links_in_db(&app).await.is_empty(),
        "no rejected call may leave anything behind in the database"
    );
}

/// The whole point of the shared broadcast channel: a write that arrives over
/// MCP still reaches every browser tab watching `/events`.
#[sqlx::test]
async fn a_link_added_over_mcp_reaches_an_open_browser(pool: PgPool) {
    let app = common::spawn(pool).await;
    let mut events = subscribe(&app).await;
    let mut mcp = Session::open(&app).await;

    mcp.call_tool(
        "add_link",
        json!({ "url": "https://example.com/live", "title": "Live from MCP" }),
    )
    .await
    .expect_ok("add_link over MCP");

    let (name, data) = next_event(&mut events, "the created frame from an MCP write").await;

    assert_eq!(
        name, "created",
        "an MCP write must broadcast a `created` event to open browsers"
    );
    assert!(
        data.contains("Live from MCP"),
        "the broadcast payload must contain the link MCP just added, got: {data}"
    );
    assert!(
        data.contains("sse-swap"),
        "the swapped-in markup must re-emit sse-swap so later updates keep arriving, got: {data}"
    );
}
