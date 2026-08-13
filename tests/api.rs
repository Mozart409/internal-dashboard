//! End-to-end coverage of the REST API under `/api/v1`, and of the
//! `OpenAPI` document that the docs UIs are generated from.
//!
//! Every request goes through `common::spawn`, which serves the real
//! `internal_dashboard::build_router`. Nothing here rebuilds routing: the
//! `/api/v1` nesting is exactly what several past bugs broke, so it has to be
//! the production wiring under test.

mod common;

use common::TestApp;
use internal_dashboard::models::Link;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// request helpers
// ---------------------------------------------------------------------------

fn parse(body: &str) -> Value {
    serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("expected a JSON response body, got {body:?} ({e})"))
}

async fn get_json(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let (status, body) = app.get(path).await;
    (status, parse(&body))
}

/// POST a raw JSON body to the links collection.
async fn post_link(app: &TestApp, payload: &Value) -> (StatusCode, Value) {
    let res = app
        .client
        .post(app.url("/api/v1/links"))
        .json(payload)
        .send()
        .await
        .expect("POST /api/v1/links");
    let status = res.status();
    let body = res.text().await.expect("read body");
    (status, parse(&body))
}

async fn put_link(app: &TestApp, id: Uuid, payload: &Value) -> (StatusCode, Value) {
    let res = app
        .client
        .put(app.url(&format!("/api/v1/links/{id}")))
        .json(payload)
        .send()
        .await
        .expect("PUT /api/v1/links/{id}");
    let status = res.status();
    let body = res.text().await.expect("read body");
    (status, parse(&body))
}

/// DELETE returns no body on success, so the raw text is handed back as-is.
async fn delete_link(app: &TestApp, id: Uuid) -> (StatusCode, String) {
    let res = app
        .client
        .delete(app.url(&format!("/api/v1/links/{id}")))
        .send()
        .await
        .expect("DELETE /api/v1/links/{id}");
    let status = res.status();
    let body = res.text().await.expect("read body");
    (status, body)
}

/// The `title` of every link in a list response, in response order.
fn titles(list: &Value) -> Vec<&str> {
    list.as_array()
        .expect("a list endpoint must return a JSON array")
        .iter()
        .map(|link| {
            link.get("title")
                .and_then(Value::as_str)
                .expect("every listed link must have a title")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// POST /api/v1/links
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn post_links_creates_a_link_and_returns_201_with_a_generated_id(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = post_link(
        &app,
        &json!({
            "url": "https://example.com/a",
            "title": "Example A",
            "description": "the first example",
            "tags": ["rust"],
        }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating a link must answer 201 Created, got {status} with body {body}"
    );

    let id = body
        .get("id")
        .and_then(Value::as_str)
        .expect("the created link must carry a server-generated id");
    let id = Uuid::parse_str(id).expect("the generated id must be a UUID");
    assert!(!id.is_nil(), "the generated id must not be the nil UUID");

    assert_eq!(body["url"], "https://example.com/a", "url must round-trip");
    assert_eq!(body["title"], "Example A", "title must round-trip");
    assert_eq!(
        body["description"], "the first example",
        "description must round-trip"
    );
    assert!(
        body.get("created_at").is_some_and(Value::is_string),
        "the created link must carry a created_at timestamp"
    );
}

#[sqlx::test]
async fn post_links_normalises_tags_to_lowercase_sorted_and_deduped(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = post_link(
        &app,
        &json!({
            "url": "https://example.com/b",
            "title": "Example B",
            "tags": ["Rust", " docs ", "RUST", "", "axum"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "body was {body}");
    assert_eq!(
        body["tags"],
        json!(["axum", "docs", "rust"]),
        "tags must be trimmed, lowercased, sorted and deduped; got {}",
        body["tags"]
    );
}

#[sqlx::test]
async fn a_created_link_decodes_into_the_production_link_model(pool: PgPool) {
    let app = common::spawn(pool).await;

    // The harness helper asserts 201 and decodes into `Link`, which proves the
    // wire format the API emits matches the model the rest of the app reads.
    let link: Link = app
        .create_link("https://example.com/c", "Example C", &["ops"])
        .await;

    assert_eq!(link.title, "Example C");
    assert_eq!(link.tags, vec!["ops".to_owned()]);
    assert!(
        link.updated_at >= link.created_at,
        "updated_at must not predate created_at"
    );
}

// ---------------------------------------------------------------------------
// GET /api/v1/links  (list, tag filter, search)
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn get_links_returns_an_empty_array_when_there_are_no_links(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = get_json(&app, "/api/v1/links").await;

    assert_eq!(status, StatusCode::OK, "listing links must answer 200");
    assert_eq!(
        body,
        json!([]),
        "an empty collection must serialise as [], not null or an object"
    );
}

#[sqlx::test]
async fn get_links_lists_every_created_link(pool: PgPool) {
    let app = common::spawn(pool).await;

    app.create_link("https://example.com/1", "One", &["rust"])
        .await;
    app.create_link("https://example.com/2", "Two", &["ops"])
        .await;

    let (status, body) = get_json(&app, "/api/v1/links").await;

    assert_eq!(status, StatusCode::OK);
    let mut found = titles(&body);
    found.sort_unstable();
    assert_eq!(
        found,
        ["One", "Two"],
        "the list endpoint must return every stored link"
    );
}

#[sqlx::test]
async fn get_links_filters_by_tag(pool: PgPool) {
    let app = common::spawn(pool).await;

    app.create_link("https://example.com/1", "Rusty", &["rust", "docs"])
        .await;
    app.create_link("https://example.com/2", "Opsy", &["ops"])
        .await;

    let (status, body) = get_json(&app, "/api/v1/links?tag=rust").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        titles(&body),
        ["Rusty"],
        "?tag= must return only links carrying that tag"
    );

    let (_, none) = get_json(&app, "/api/v1/links?tag=nosuchtag").await;
    assert_eq!(
        none,
        json!([]),
        "an unknown tag must yield an empty list, not every link"
    );
}

#[sqlx::test]
async fn get_links_search_matches_a_title(pool: PgPool) {
    let app = common::spawn(pool).await;

    app.create_link("https://example.com/1", "Postgres tuning", &[])
        .await;
    app.create_link("https://example.com/2", "Axum routing", &[])
        .await;

    // Search is a case-insensitive substring match.
    let (status, body) = get_json(&app, "/api/v1/links?q=POSTGRES").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        titles(&body),
        ["Postgres tuning"],
        "?q= must match titles case-insensitively"
    );
}

#[sqlx::test]
async fn get_links_search_matches_a_description(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, _) = post_link(
        &app,
        &json!({
            "url": "https://example.com/handbook",
            "title": "Handbook",
            "description": "everything about deployment runbooks",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "setup: create the handbook");

    app.create_link("https://example.com/other", "Unrelated", &[])
        .await;

    // The search term appears only in the description, nowhere in title or url.
    let (status, body) = get_json(&app, "/api/v1/links?q=runbooks").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        titles(&body),
        ["Handbook"],
        "?q= must search descriptions, not just titles and urls"
    );
}

// ---------------------------------------------------------------------------
// GET /api/v1/links/{id}
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn get_link_by_id_returns_the_link(pool: PgPool) {
    let app = common::spawn(pool).await;

    let created = app
        .create_link("https://example.com/one", "One", &["rust"])
        .await;

    let (status, body) = get_json(&app, &format!("/api/v1/links/{}", created.id)).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "fetching an existing link must be 200"
    );
    assert_eq!(
        body["id"],
        created.id.to_string(),
        "the fetched link must be the one that was created"
    );
    assert_eq!(body["title"], "One");
}

#[sqlx::test]
async fn get_link_by_id_is_404_for_an_unknown_uuid(pool: PgPool) {
    let app = common::spawn(pool).await;

    let missing = Uuid::new_v4();
    let (status, body) = get_json(&app, &format!("/api/v1/links/{missing}")).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a well-formed but unknown id must be 404, not 200 or 500; body was {body}"
    );
    assert!(
        body.get("error").and_then(Value::as_str).is_some(),
        "error responses must carry an `error` field, got {body}"
    );
}

// ---------------------------------------------------------------------------
// PUT /api/v1/links/{id}
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn put_link_updates_the_stored_link(pool: PgPool) {
    let app = common::spawn(pool).await;

    let created = app
        .create_link("https://example.com/old", "Old", &["old"])
        .await;

    let (status, body) = put_link(
        &app,
        created.id,
        &json!({
            "url": "https://example.com/new",
            "title": "New",
            "description": "now described",
            "tags": ["New", "fresh"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "a successful update must be 200");
    assert_eq!(body["id"], created.id.to_string(), "the id must not change");
    assert_eq!(body["url"], "https://example.com/new");
    assert_eq!(body["title"], "New");
    assert_eq!(body["description"], "now described");
    assert_eq!(
        body["tags"],
        json!(["fresh", "new"]),
        "updated tags must be normalised the same way as created ones"
    );

    // The change must be persisted, not merely echoed back.
    let (_, refetched) = get_json(&app, &format!("/api/v1/links/{}", created.id)).await;
    assert_eq!(
        refetched["title"], "New",
        "the update must be visible to a later GET"
    );
}

#[sqlx::test]
async fn put_link_with_one_field_leaves_the_other_fields_intact(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, created) = post_link(
        &app,
        &json!({
            "url": "https://example.com/partial",
            "title": "Before",
            "description": "keep me",
            "tags": ["keep"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "setup: create the link");
    let id = Uuid::parse_str(created["id"].as_str().expect("created id")).expect("uuid");

    let (status, body) = put_link(&app, id, &json!({ "title": "After" })).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "After", "the supplied field must be updated");
    assert_eq!(
        body["url"], "https://example.com/partial",
        "an omitted url must be left unchanged, not cleared"
    );
    assert_eq!(
        body["description"], "keep me",
        "an omitted description must be left unchanged, not cleared"
    );
    assert_eq!(
        body["tags"],
        json!(["keep"]),
        "omitted tags must be left unchanged, not emptied"
    );
}

#[sqlx::test]
async fn put_link_is_404_for_an_unknown_uuid(pool: PgPool) {
    let app = common::spawn(pool).await;

    let missing = Uuid::new_v4();
    let (status, body) = put_link(&app, missing, &json!({ "title": "ghost" })).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "updating an unknown id must be 404 rather than silently creating a row; body was {body}"
    );

    let (_, list) = get_json(&app, "/api/v1/links").await;
    assert_eq!(
        list,
        json!([]),
        "a failed update must not have inserted anything"
    );
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/links/{id}
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn delete_link_returns_204_and_actually_removes_the_row(pool: PgPool) {
    let app = common::spawn(pool).await;

    let created = app
        .create_link("https://example.com/doomed", "Doomed", &[])
        .await;

    let (status, body) = delete_link(&app, created.id).await;

    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "a successful delete must be 204 No Content, got {status}"
    );
    assert!(
        body.is_empty(),
        "204 responses must have an empty body, got {body:?}"
    );

    let (status, _) = app.get(&format!("/api/v1/links/{}", created.id)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the deleted link must be gone: a follow-up GET must be 404"
    );

    let (_, list) = get_json(&app, "/api/v1/links").await;
    assert_eq!(list, json!([]), "the deleted row must leave the collection");
}

#[sqlx::test]
async fn delete_link_is_404_for_an_unknown_uuid(pool: PgPool) {
    let app = common::spawn(pool).await;

    let missing = Uuid::new_v4();
    let (status, _) = delete_link(&app, missing).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "deleting an id that does not exist must be 404, not a silent 204"
    );
}

// ---------------------------------------------------------------------------
// validation
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn post_links_rejects_a_non_http_url_scheme_with_400_and_an_error_field(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = post_link(
        &app,
        &json!({ "url": "ftp://x", "title": "Sneaky", "tags": [] }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a url with a non-http(s) scheme must be rejected with 400; body was {body}"
    );

    let message = body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("a 400 body must carry an `error` field, got {body}"));
    assert!(
        message.contains("http://") && message.contains("https://"),
        "the error must explain which schemes are allowed, got {message:?}"
    );

    let (_, list) = get_json(&app, "/api/v1/links").await;
    assert_eq!(
        list,
        json!([]),
        "a rejected create must not have stored anything"
    );
}

#[sqlx::test]
async fn post_links_rejects_a_blank_title_with_400(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, body) = post_link(
        &app,
        &json!({ "url": "https://example.com", "title": "   ", "tags": [] }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a whitespace-only title must be rejected with 400; body was {body}"
    );

    let message = body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("a 400 body must carry an `error` field, got {body}"));
    assert!(
        message.contains("title"),
        "the error must name the offending field, got {message:?}"
    );
}

// ---------------------------------------------------------------------------
// OpenAPI document — regression cover for the nest-prefix bug
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn openapi_document_nests_the_api_prefix_exactly_once(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, doc) = get_json(&app, "/api-docs/openapi.json").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the generated spec must be served at /api-docs/openapi.json"
    );

    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .expect("the spec must contain a `paths` object");

    let mut keys: Vec<&str> = paths.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["/api/v1/links", "/api/v1/links/{id}"],
        "the spec must document exactly the two real routes; got {keys:?}"
    );

    // The regression itself: a utoipa `path` attribute that repeats the nest
    // prefix produced /api/v1/api/v1/links, so every documented route 404'd.
    assert!(
        !paths.contains_key("/api/v1/api/v1/links"),
        "the /api/v1 prefix is applied twice — every documented route would 404"
    );
    assert!(
        !paths.contains_key("/api/v1/api/v1/links/{id}"),
        "the /api/v1 prefix is applied twice for the item route"
    );
    // The opposite failure: the prefix dropped entirely.
    assert!(
        !paths.contains_key("/links"),
        "the /api/v1 nest prefix is missing from the documented paths"
    );
}

#[sqlx::test]
async fn openapi_document_lists_all_five_link_operations(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (status, doc) = get_json(&app, "/api-docs/openapi.json").await;
    assert_eq!(status, StatusCode::OK);

    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .expect("the spec must contain a `paths` object");

    let mut operations: Vec<&str> = paths
        .values()
        .filter_map(Value::as_object)
        .flat_map(serde_json::Map::values)
        .filter_map(|op| op.get("operationId").and_then(Value::as_str))
        .collect();
    operations.sort_unstable();

    assert_eq!(
        operations,
        [
            "create_link",
            "delete_link",
            "get_link",
            "list_links",
            "update_link"
        ],
        "all five handlers must register themselves in the spec; got {operations:?}"
    );
}

#[sqlx::test]
async fn the_documented_paths_are_the_paths_that_actually_serve(pool: PgPool) {
    let app = common::spawn(pool).await;

    let (_, doc) = get_json(&app, "/api-docs/openapi.json").await;
    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .expect("the spec must contain a `paths` object");

    // Walk the collection path straight out of the spec and call it. If the
    // documented prefix ever drifts from the router again, this 404s.
    for documented in paths.keys().filter(|p| !p.contains('{')) {
        let (status, _) = app.get(documented).await;
        assert!(
            status.is_success(),
            "the spec documents {documented}, but requesting it returned {status}"
        );
    }
}

// ---------------------------------------------------------------------------
// docs UI
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn the_api_docs_ui_is_served(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = app
        .client
        .get(app.url("/scalar"))
        .send()
        .await
        .expect("docs UI request");
    let status = res.status();
    assert!(
        status.is_success(),
        "/scalar must serve the API docs, got {status}"
    );
}

#[sqlx::test]
async fn the_openapi_document_is_served_as_json(pool: PgPool) {
    let app = common::spawn(pool).await;

    let res = app
        .client
        .get(app.url("/api-docs/openapi.json"))
        .send()
        .await
        .expect("openapi request");

    assert_eq!(res.status(), StatusCode::OK);
    // Regression cover for dropping swagger-ui, which used to register this
    // route: the spec must still be served, and served as JSON.
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.starts_with("application/json"),
        "the spec must be served as JSON, got {content_type:?}"
    );
}
