//! Tests for the server-rendered HTMX UI.
//!
//! Everything here drives the real router over real HTTP via the shared
//! harness — the UI bugs these tests guard against lived in route declaration,
//! attribute spelling and template nesting, so nothing may be reconstructed
//! locally.
//!
//! Four of these tests guard regressions that actually shipped:
//!
//! 1. `sse-swap` vs the htmx 1.x `hx-sse` spelling,
//! 2. a duplicated nested `#link-list` container,
//! 3. axum 0.7 `:id` path syntax under axum 0.8,
//! 4. htmx loaded from the `unpkg.com` CDN instead of the vendored copy.

mod common;

use internal_dashboard::models::Link;
use sqlx::PgPool;
use uuid::Uuid;

/// Number of non-overlapping occurrences of `needle` in `haystack`.
fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Percent-encode one form field, leaving only the unreserved characters.
fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    out
}

/// Attach an `application/x-www-form-urlencoded` body — the encoding a browser
/// uses for these routes, which take `Form` rather than `Json`.
///
/// Encoded by hand because this project builds `reqwest` without the feature
/// that provides `RequestBuilder::form`.
fn with_form(builder: reqwest::RequestBuilder, fields: &[(&str, &str)]) -> reqwest::RequestBuilder {
    let body = fields
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");

    builder
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
}

// --- regression guards -------------------------------------------------------

/// Guards the htmx 2 attribute regression: the list container was once bound
/// with `hx-sse="swap:created,updated,deleted"`, which is htmx 1.x syntax.
/// htmx 2 ignores it silently, so SSE looked wired up but never updated
/// anything.
#[sqlx::test]
async fn index_binds_sse_with_the_htmx_2_attribute(pool: PgPool) {
    let app = common::spawn(pool).await;
    app.create_link("https://a.dev", "A", &["rust"]).await;

    let (status, body) = app.get("/").await;

    assert!(status.is_success(), "GET / returned {status}");
    assert!(
        body.contains("sse-swap="),
        "regression guard: the list must be bound with the htmx 2 `sse-swap` \
         attribute, otherwise live updates never fire"
    );
    assert!(
        !body.contains("hx-sse"),
        "regression guard: `hx-sse` is htmx 1.x syntax and is silently ignored \
         by htmx 2 — it must not appear in the rendered page"
    );
    assert!(
        body.contains("sse-connect="),
        "the page must open an SSE connection for `sse-swap` to have a source"
    );
}

/// Guards the duplicate-container regression: the list was once rendered
/// inside another element carrying the same id, so after the first swap the
/// binding pointed at a stale node and updates stopped.
#[sqlx::test]
async fn index_renders_exactly_one_link_list_container(pool: PgPool) {
    let app = common::spawn(pool).await;
    app.create_link("https://a.dev", "A", &["rust"]).await;
    app.create_link("https://b.dev", "B", &[]).await;

    let (status, body) = app.get("/").await;

    assert!(status.is_success(), "GET / returned {status}");
    assert_eq!(
        count(&body, "id=\"link-list\""),
        1,
        "regression guard: `#link-list` must be unique — a nested duplicate \
         loses the swap binding after the first update"
    );
}

/// Guards the axum 0.8 path-syntax regression: the route was once declared
/// `"/links/:id/edit"` (axum 0.7 syntax), which panics at startup under axum
/// 0.8. Any test that spawns the app catches the panic, but this one names the
/// route.
#[sqlx::test]
async fn edit_page_route_uses_axum_0_8_path_syntax(pool: PgPool) {
    let app = common::spawn(pool).await;
    let link = app.create_link("https://a.dev", "A", &["rust"]).await;

    let (status, body) = app.get(&format!("/links/{}/edit", link.id)).await;

    assert!(
        status.is_success(),
        "regression guard: GET /links/{{id}}/edit must resolve under axum 0.8 \
         path syntax, got {status}"
    );
    assert!(
        body.contains("https://a.dev"),
        "the edit form should be prefilled with the link's url"
    );
    assert!(
        body.contains("value=\"rust\""),
        "the edit form should be prefilled with the link's tags"
    );
}

/// Guards the offline-vendoring regression: htmx used to be pulled from
/// `unpkg.com`, which broke the dashboard on an air-gapped host. Both scripts
/// are now embedded in the binary and served from `/static`.
#[sqlx::test]
async fn index_loads_htmx_from_vendored_assets_only(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = app.get("/").await;

    assert!(status.is_success(), "GET / returned {status}");
    assert!(
        body.contains("/static/htmx.min.js"),
        "regression guard: htmx must be loaded from the vendored `/static` copy"
    );
    assert!(
        body.contains("/static/sse.js"),
        "regression guard: the SSE extension must be loaded from the vendored \
         `/static` copy"
    );
    assert_eq!(
        count(&body, "unpkg.com"),
        0,
        "regression guard: the page must not reference the CDN — the dashboard \
         has to work with no network access"
    );
}

// --- page rendering ----------------------------------------------------------

#[sqlx::test]
async fn index_renders_the_add_link_form(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = app.get("/").await;

    assert_eq!(status, reqwest::StatusCode::OK, "GET / should return 200");
    assert!(
        body.contains("hx-post=\"/links\""),
        "the add-link form should post to /links"
    );
    for field in ["name=\"url\"", "name=\"title\"", "name=\"tags\""] {
        assert!(
            body.contains(field),
            "the add-link form should contain an input with {field}"
        );
    }
}

#[sqlx::test]
async fn index_renders_each_links_title_and_url(pool: PgPool) {
    let app = common::spawn(pool).await;
    app.create_link("https://rust-lang.org", "Rust", &["lang"])
        .await;
    app.create_link("https://docs.rs", "Docs.rs", &[]).await;

    let (status, body) = app.get("/").await;

    assert!(status.is_success(), "GET / returned {status}");
    for expected in [
        "Rust",
        "https://rust-lang.org",
        "Docs.rs",
        "https://docs.rs",
    ] {
        assert!(
            body.contains(expected),
            "the dashboard should render {expected}"
        );
    }
    assert_eq!(
        count(&body, "class=\"link-row\""),
        2,
        "the dashboard should render one row per link"
    );
}

#[sqlx::test]
async fn index_renders_an_empty_state_when_there_are_no_links(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = app.get("/").await;

    assert!(status.is_success(), "GET / returned {status}");
    assert!(
        body.contains("class=\"empty-state\""),
        "an empty database should render the empty state"
    );
    assert_eq!(
        count(&body, "class=\"link-row\""),
        0,
        "an empty database should render no link rows"
    );
    assert_eq!(
        count(&body, "id=\"link-list\""),
        1,
        "the empty state must still render exactly one swap target"
    );
}

// --- vendored static assets --------------------------------------------------

#[sqlx::test]
async fn static_htmx_is_served_as_javascript(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = app
        .client
        .get(app.url("/static/htmx.min.js"))
        .send()
        .await
        .expect("GET /static/htmx.min.js");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "the vendored htmx bundle should be served"
    );
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .expect("content-type header")
        .to_string();
    assert!(
        content_type.starts_with("application/javascript"),
        "htmx should be served as javascript, got {content_type}"
    );

    let body = res.text().await.expect("read body");
    assert!(!body.is_empty(), "the htmx bundle should not be empty");
    assert!(
        body.contains("htmx"),
        "the served bundle should actually be htmx, not an error page"
    );
    assert!(
        !body.to_lowercase().contains("<!doctype html"),
        "the served bundle must be javascript, not an HTML error page"
    );
}

#[sqlx::test]
async fn static_sse_extension_is_served_as_javascript(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = app
        .client
        .get(app.url("/static/sse.js"))
        .send()
        .await
        .expect("GET /static/sse.js");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::OK,
        "the vendored SSE extension should be served"
    );
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .expect("content-type header")
        .to_string();
    assert!(
        content_type.starts_with("application/javascript"),
        "the SSE extension should be served as javascript, got {content_type}"
    );

    let body = res.text().await.expect("read body");
    assert!(!body.is_empty(), "the SSE extension should not be empty");
    assert!(
        body.contains("sse"),
        "the served file should actually be the SSE extension, not an error page"
    );
}

// --- form submission ---------------------------------------------------------

#[sqlx::test]
async fn post_links_creates_a_link_and_returns_a_fragment(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = with_form(
        app.client.post(app.url("/links")),
        &[
            ("url", "https://rust-lang.org"),
            ("title", "Rust"),
            ("description", "the language"),
            ("tags", "lang"),
        ],
    )
    .send()
    .await
    .expect("POST /links");

    let status = res.status();
    let body = res.text().await.expect("read body");

    assert!(status.is_success(), "POST /links returned {status}: {body}");
    assert!(
        body.contains("Rust") && body.contains("https://rust-lang.org"),
        "the returned fragment should contain the new link"
    );
    assert_eq!(
        count(&body, "class=\"link-row\""),
        1,
        "the fragment should render the one link that now exists"
    );
}

/// The swap replaces `#link-list` via `outerHTML`, so a fragment without the
/// binding would leave the page with no SSE target — live updates would work
/// until the first form submit and then silently stop.
#[sqlx::test]
async fn post_links_fragment_carries_the_sse_swap_binding(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = with_form(
        app.client.post(app.url("/links")),
        &[("url", "https://a.dev"), ("title", "A")],
    )
    .send()
    .await
    .expect("POST /links");

    let body = res.text().await.expect("read body");

    assert!(
        body.contains("sse-swap="),
        "the fragment must re-emit `sse-swap` — outerHTML replacement would \
         otherwise drop the binding and stop live updates after one submit"
    );
    assert_eq!(
        count(&body, "id=\"link-list\""),
        1,
        "the fragment must carry exactly one `#link-list` swap target"
    );
    assert!(
        body.contains("hx-swap=\"outerHTML\""),
        "the fragment must keep the outerHTML swap mode it is replaced with"
    );
}

#[sqlx::test]
async fn post_links_normalises_comma_separated_tags(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = with_form(
        app.client.post(app.url("/links")),
        &[
            ("url", "https://a.dev"),
            ("title", "A"),
            ("tags", "Rust, docs ,rust"),
        ],
    )
    .send()
    .await
    .expect("POST /links");

    assert!(
        res.status().is_success(),
        "POST /links with comma-separated tags returned {}",
        res.status()
    );

    let links: Vec<Link> = app
        .client
        .get(app.url("/api/v1/links"))
        .send()
        .await
        .expect("GET /api/v1/links")
        .json()
        .await
        .expect("decode links");

    assert_eq!(links.len(), 1, "exactly one link should have been created");
    assert_eq!(
        links[0].tags,
        vec!["docs".to_string(), "rust".to_string()],
        "form tags must be split on commas, trimmed, lowercased, sorted and \
         deduped"
    );
}

#[sqlx::test]
async fn put_links_updates_and_returns_a_fragment(pool: PgPool) {
    let app = common::spawn(pool).await;
    let link = app
        .create_link("https://a.dev", "Old title", &["rust"])
        .await;

    let res = with_form(
        app.client.put(app.url(&format!("/links/{}", link.id))),
        &[
            ("url", "https://b.dev"),
            ("title", "New title"),
            ("tags", "Docs, rust"),
        ],
    )
    .send()
    .await
    .expect("PUT /links/{id}");

    let status = res.status();
    let body = res.text().await.expect("read body");

    assert!(status.is_success(), "PUT /links/{{id}} returned {status}");
    assert!(
        body.contains("New title") && body.contains("https://b.dev"),
        "the returned fragment should show the updated link"
    );
    assert!(
        !body.contains("Old title"),
        "the returned fragment should not still show the old title"
    );

    let links: Vec<Link> = app
        .client
        .get(app.url("/api/v1/links"))
        .send()
        .await
        .expect("GET /api/v1/links")
        .json()
        .await
        .expect("decode links");

    assert_eq!(links.len(), 1, "updating must not create a second link");
    assert_eq!(
        links[0].title, "New title",
        "the update should be persisted"
    );
    assert_eq!(
        links[0].tags,
        vec!["docs".to_string(), "rust".to_string()],
        "updated form tags must be normalised the same way as on create"
    );
}

#[sqlx::test]
async fn delete_links_removes_the_link(pool: PgPool) {
    let app = common::spawn(pool).await;
    let doomed = app.create_link("https://a.dev", "Doomed", &[]).await;
    app.create_link("https://b.dev", "Survivor", &[]).await;

    let res = app
        .client
        .delete(app.url(&format!("/links/{}", doomed.id)))
        .send()
        .await
        .expect("DELETE /links/{id}");

    let status = res.status();
    let body = res.text().await.expect("read body");

    assert!(
        status.is_success(),
        "DELETE /links/{{id}} returned {status}"
    );
    assert!(
        !body.contains("Doomed"),
        "the returned fragment should no longer contain the deleted link"
    );
    assert!(
        body.contains("Survivor"),
        "the returned fragment should still contain the remaining link"
    );

    let links: Vec<Link> = app
        .client
        .get(app.url("/api/v1/links"))
        .send()
        .await
        .expect("GET /api/v1/links")
        .json()
        .await
        .expect("decode links");

    assert_eq!(
        links.len(),
        1,
        "the deleted link should be gone from the db"
    );
    assert_eq!(links[0].title, "Survivor", "the wrong link was deleted");
}

// --- error handling ----------------------------------------------------------

/// A missing link must not surface as a 500. It currently answers 404 with the
/// shared JSON error body (`{"error":"not found"}`) rather than an HTML page.
#[sqlx::test]
async fn edit_page_for_a_missing_link_does_not_500(pool: PgPool) {
    let app = common::spawn(pool).await;

    let missing = Uuid::new_v4();
    let (status, body) = app.get(&format!("/links/{missing}/edit")).await;

    assert_ne!(
        status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "a missing link is a client error, not a server fault"
    );
    assert_eq!(
        status,
        reqwest::StatusCode::NOT_FOUND,
        "GET /links/{{missing}}/edit should return 404, got {status}: {body}"
    );
    assert!(
        body.contains("not found"),
        "the 404 response should say what went wrong, got: {body}"
    );
}

#[sqlx::test]
async fn edit_page_rejects_a_malformed_id_without_500(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, _body) = app.get("/links/not-a-uuid/edit").await;

    assert_ne!(
        status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "an unparseable id is a client error, not a server fault"
    );
    assert!(
        status.is_client_error(),
        "an unparseable id should be a 4xx, got {status}"
    );
}

#[sqlx::test]
async fn post_links_rejects_a_non_http_url(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = with_form(
        app.client.post(app.url("/links")),
        &[("url", "ftp://a.dev"), ("title", "A")],
    )
    .send()
    .await
    .expect("POST /links");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::BAD_REQUEST,
        "the UI create route must apply the same validation as the API"
    );

    let links: Vec<Link> = app
        .client
        .get(app.url("/api/v1/links"))
        .send()
        .await
        .expect("GET /api/v1/links")
        .json()
        .await
        .expect("decode links");
    assert!(
        links.is_empty(),
        "a rejected submission must persist nothing"
    );
}
