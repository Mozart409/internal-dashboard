//! Server-rendered HTMX UI with Maud templates and SSE real-time updates.

use axum::{
    Form, Router,
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::events::{AppState, LinkEvent};
use crate::models::{Link, NewLink, UpdateLink};

/// Vendored htmx assets, embedded in the binary so the dashboard works with no
/// network access and without needing `static/` next to the executable.
/// Refresh them with `just vendor-assets`.
const HTMX_JS: &str = include_str!("../static/htmx.min.js");
const HTMX_SSE_JS: &str = include_str!("../static/sse.js");

/// Serve an embedded script. Immutable caching is safe because the contents
/// only change when the binary is rebuilt.
fn javascript(body: &'static str) -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
}

// axum requires handlers to be async even when they do no awaiting.
#[allow(clippy::unused_async)]
async fn htmx_js() -> impl IntoResponse {
    javascript(HTMX_JS)
}

#[allow(clippy::unused_async)]
async fn htmx_sse_js() -> impl IntoResponse {
    javascript(HTMX_SSE_JS)
}

/// Stylesheet for the whole dashboard, inlined into every page.
const STYLES: &str = r#"
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

:root {
  --bg-primary: #ffffff;
  --bg-secondary: #f8f9fa;
  --text-primary: #1a1a1a;
  --text-secondary: #555555;
  --border: #e0e0e0;
  --accent: #2563eb;
  --accent-hover: #1d4ed8;
  --tag-bg: #e0e7ff;
  --tag-text: #3730a3;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg-primary: #1a1a1a;
    --bg-secondary: #2d2d2d;
    --text-primary: #ffffff;
    --text-secondary: #b0b0b0;
    --border: #404040;
    --accent: #3b82f6;
    --accent-hover: #60a5fa;
    --tag-bg: #312e81;
    --tag-text: #c7d2fe;
  }
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  background-color: var(--bg-primary);
  color: var(--text-primary);
  transition: background-color 0.2s, color 0.2s;
  padding: 1rem;
  line-height: 1.5;
}

.container {
  max-width: 900px;
  margin: 0 auto;
}

h1 {
  font-size: 2rem;
  margin-bottom: 2rem;
}

h2 {
  font-size: 1.5rem;
  margin-bottom: 1.5rem;
  color: var(--text-secondary);
}

form {
  background-color: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  padding: 1.5rem;
  margin-bottom: 2rem;
}

.form-group {
  margin-bottom: 1.5rem;
}

.form-group:last-child {
  margin-bottom: 0;
}

label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
  color: var(--text-secondary);
}

input, textarea {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid var(--border);
  border-radius: 0.375rem;
  background-color: var(--bg-primary);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 1rem;
}

textarea {
  resize: vertical;
  min-height: 100px;
}

input:focus, textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}

button {
  background-color: var(--accent);
  color: white;
  padding: 0.75rem 1.5rem;
  border: none;
  border-radius: 0.375rem;
  font-size: 1rem;
  cursor: pointer;
  font-weight: 500;
  transition: background-color 0.2s;
}

button:hover {
  background-color: var(--accent-hover);
}

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-secondary {
  background-color: var(--text-secondary);
}

.btn-secondary:hover {
  background-color: var(--text-primary);
}

.btn-danger {
  background-color: #dc2626;
}

.btn-danger:hover {
  background-color: #b91c1c;
}

#link-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.link-row {
  background-color: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  padding: 1.5rem;
  transition: box-shadow 0.2s;
}

.link-row:hover {
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
}

@media (prefers-color-scheme: dark) {
  .link-row:hover {
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }
}

.link-title {
  font-size: 1.25rem;
  font-weight: 600;
  margin-bottom: 0.5rem;
}

.link-title a {
  color: var(--accent);
  text-decoration: none;
}

.link-title a:hover {
  text-decoration: underline;
}

.link-url {
  font-size: 0.875rem;
  color: var(--text-secondary);
  word-break: break-all;
  margin-bottom: 0.75rem;
}

.link-description {
  color: var(--text-secondary);
  margin-bottom: 0.75rem;
}

.link-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 1rem;
  margin-bottom: 1rem;
}

.link-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.tag {
  display: inline-block;
  background-color: var(--tag-bg);
  color: var(--tag-text);
  padding: 0.25rem 0.75rem;
  border-radius: 1rem;
  font-size: 0.875rem;
  font-weight: 500;
}

.link-date {
  font-size: 0.875rem;
  color: var(--text-secondary);
}

.link-actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 1rem;
}

.link-actions button {
  padding: 0.5rem 1rem;
  font-size: 0.875rem;
}

.empty-state {
  text-align: center;
  padding: 3rem 1.5rem;
  color: var(--text-secondary);
}

.empty-state p {
  margin-bottom: 0.5rem;
}

.filter-form {
  background-color: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  padding: 1.5rem;
  margin-bottom: 2rem;
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
  align-items: flex-end;
}

.filter-form-group {
  flex: 1;
  min-width: 200px;
}

.filter-form-group label {
  margin-bottom: 0.5rem;
}

.filter-form-group input {
  margin-bottom: 0;
}

.filter-form button {
  margin: 0;
}

.filter-form a {
  padding: 0.75rem 1.5rem;
  background-color: var(--text-secondary);
  color: white;
  text-decoration: none;
  border-radius: 0.375rem;
  font-size: 1rem;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.2s;
  display: inline-block;
}

.filter-form a:hover {
  background-color: var(--text-primary);
}
"#;

/// Full HTML document layout.
///
/// Includes the locally vendored HTMX and SSE extension, plus the dashboard
/// styles with dark-mode support and a centered container.
#[must_use]
pub fn layout(title: &str, body: Markup) -> Markup {
    html! {
        doctype;
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                script src="/static/htmx.min.js" {}
                script src="/static/sse.js" {}
                style { (maud::PreEscaped(STYLES)) }
            }
            body {
                (body)
            }
        }
    }
}

/// Render a list of links in a container with SSE update support.
///
/// # Arguments
/// * `links` - Slice of links to render
///
/// # Returns
/// Markup containing `<div id="link-list">…</div>` with one row per link,
/// or an empty-state message if the slice is empty.
#[must_use]
pub fn render_link_list(links: &[Link]) -> Markup {
    // The sse-swap binding lives on this element and is re-emitted on every
    // render, because each SSE payload replaces it via outerHTML — if the
    // attributes lived on a parent, the binding would be lost after one swap.
    if links.is_empty() {
        return html! {
            div id="link-list" sse-swap="created,updated,deleted" hx-swap="outerHTML" {
                div class="empty-state" {
                    p { "No links yet. Create one to get started!" }
                }
            }
        };
    }

    html! {
        div id="link-list" sse-swap="created,updated,deleted" hx-swap="outerHTML" {
            @for link in links {
                div class="link-row" {
                    div class="link-title" {
                        a href=(link.url) target="_blank" rel="noopener noreferrer" {
                            (link.title)
                        }
                    }
                    div class="link-url" {
                        (link.url)
                    }
                    @if let Some(desc) = &link.description {
                        div class="link-description" {
                            (desc)
                        }
                    }
                    div class="link-meta" {
                        @if !link.tags.is_empty() {
                            div class="link-tags" {
                                @for tag in &link.tags {
                                    span class="tag" { (tag) }
                                }
                            }
                        }
                        div class="link-date" {
                            (format_date(link.created_at))
                        }
                    }
                    div class="link-actions" {
                        a href={"/links/" (link.id) "/edit"} {
                            button type="button" class="btn-secondary" { "Edit" }
                        }
                        button type="button" class="btn-danger" hx-delete={"/links/" (link.id)}
                                hx-target="#link-list" hx-swap="outerHTML"
                                hx-confirm="Are you sure?" { "Delete" }
                    }
                }
            }
        }
    }
}

/// GET / — Display the dashboard with an add-link form and SSE-connected list.
///
/// # Errors
/// Returns [`AppError::Database`] if loading the links fails.
pub async fn index(
    State(state): State<AppState>,
    Query(params): Query<IndexParams>,
) -> Result<impl IntoResponse, AppError> {
    let links = if let Some(q) = &params.q {
        crate::db::search_links(&state.pool, q, 100).await?
    } else {
        crate::db::list_links(&state.pool, params.tag.as_deref()).await?
    };

    let form_html = html! {
        h2 { "Add a Link" }
        form hx-post="/links" hx-target="#link-list" hx-swap="outerHTML" hx-on::after-request="this.reset()" {
            div class="form-group" {
                label for="url" { "URL" }
                input type="url" id="url" name="url" required;
            }
            div class="form-group" {
                label for="title" { "Title" }
                input type="text" id="title" name="title" required;
            }
            div class="form-group" {
                label for="description" { "Description" }
                textarea id="description" name="description" {}
            }
            div class="form-group" {
                label for="tags" { "Tags (comma-separated)" }
                input type="text" id="tags" name="tags";
            }
            button type="submit" { "Add Link" }
        }
    };

    let filter_html = if params.tag.is_some() || params.q.is_some() {
        html! {
            div class="filter-form" {
                div class="filter-form-group" {
                    p { "Active filters:" }
                }
                @if let Some(tag) = &params.tag {
                    span { "Tag: " (tag) }
                }
                @if let Some(q) = &params.q {
                    span { "Search: " (q) }
                }
                a href="/" { "Clear" }
            }
        }
    } else {
        html! {}
    };

    let list_html = html! {
        div hx-ext="sse" sse-connect="/events" {
            (render_link_list(&links))
        }
    };

    let body = html! {
        div class="container" {
            h1 { "Links Dashboard" }
            (filter_html)
            (form_html)
            (list_html)
        }
    };

    Ok(layout("Links Dashboard", body))
}

#[derive(Debug, Deserialize)]
pub struct IndexParams {
    tag: Option<String>,
    q: Option<String>,
}

/// GET /links/{id}/edit — Display the edit form for a link.
///
/// # Errors
/// Returns [`AppError::NotFound`] if no link has that id, or
/// [`AppError::Database`] if the lookup fails.
pub async fn edit_page(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let link = crate::db::get_link(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let tags_str = link.tags.join(", ");

    let body = html! {
        div class="container" {
            h1 { "Edit Link" }
            form hx-put={"/links/" (link.id)} hx-target="#link-list" hx-swap="outerHTML" {
                div class="form-group" {
                    label for="url" { "URL" }
                    input type="url" id="url" name="url" value=(link.url) required;
                }
                div class="form-group" {
                    label for="title" { "Title" }
                    input type="text" id="title" name="title" value=(link.title) required;
                }
                div class="form-group" {
                    label for="description" { "Description" }
                    textarea id="description" name="description" {
                        @if let Some(desc) = &link.description {
                            (desc)
                        }
                    }
                }
                div class="form-group" {
                    label for="tags" { "Tags (comma-separated)" }
                    input type="text" id="tags" name="tags" value=(tags_str);
                }
                button type="submit" { "Update Link" }
                a href="/" {
                    button type="button" class="btn-secondary" { "Cancel" }
                }
            }
        }
    };

    Ok(layout(&format!("Edit — {}", link.title), body))
}

/// POST /links — Create a new link.
///
/// # Errors
/// Returns error if validation fails or database operation fails.
pub async fn create_link(
    State(state): State<AppState>,
    Form(mut new_link): Form<NewLink>,
) -> Result<Markup, AppError> {
    new_link.validate()?;

    let link = crate::db::create_link(&state.pool, &new_link).await?;
    state.publish(LinkEvent::Created(link));

    let links = crate::db::list_links(&state.pool, None).await?;
    Ok(render_link_list(&links))
}

/// PUT /links/{id} — Update a link.
///
/// # Errors
/// Returns error if the link is not found or database operation fails.
pub async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(update): Form<UpdateLink>,
) -> Result<Markup, AppError> {
    let link = crate::db::update_link(&state.pool, id, &update)
        .await?
        .ok_or(AppError::NotFound)?;

    state.publish(LinkEvent::Updated(link));

    let links = crate::db::list_links(&state.pool, None).await?;
    Ok(render_link_list(&links))
}

/// DELETE /links/{id} — Delete a link.
///
/// # Errors
/// Returns error if the link is not found or database operation fails.
pub async fn delete_link_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Markup, AppError> {
    let deleted = crate::db::delete_link(&state.pool, id).await?;

    if !deleted {
        return Err(AppError::NotFound);
    }

    state.publish(LinkEvent::Deleted(id));

    let links = crate::db::list_links(&state.pool, None).await?;
    Ok(render_link_list(&links))
}

/// Build the UI router with all HTML routes.
///
/// # Returns
/// An Axum router configured with GET /, GET /links/{id}/edit, POST /links, PUT /links/{id}, and DELETE /links/{id}.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/static/htmx.min.js", axum::routing::get(htmx_js))
        .route("/static/sse.js", axum::routing::get(htmx_sse_js))
        .route("/", axum::routing::get(index))
        .route("/links/{id}/edit", axum::routing::get(edit_page))
        .route("/links", axum::routing::post(create_link))
        .route("/links/{id}", axum::routing::put(update_link))
        .route("/links/{id}", axum::routing::delete(delete_link_handler))
}

/// Format a UTC datetime for display.
fn format_date(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M UTC").to_string()
}
