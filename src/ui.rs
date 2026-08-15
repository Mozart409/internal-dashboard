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
const TAILWIND_CSS: &str = include_str!("../static/tailwind.css");
const FAVICON_SVG: &str = include_str!("../static/favicon.svg");

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

#[allow(clippy::unused_async)]
async fn tailwind_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        TAILWIND_CSS,
    )
}

#[allow(clippy::unused_async)]
async fn favicon() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        FAVICON_SVG,
    )
}

/// Full HTML document layout.
///
/// Includes the locally vendored HTMX and SSE extension, plus the themeable
/// stylesheet generated from `src/input.css`.
#[must_use]
pub fn layout(title: &str, body: Markup) -> Markup {
    html! {
        doctype;
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                link rel="icon" href="/static/favicon.svg" type="image/svg+xml";
                script src="/static/htmx.min.js" {}
                script src="/static/sse.js" {}
                link rel="stylesheet" href="/static/tailwind.css";
                script {
                    (maud::PreEscaped(r"(() => { const saved = localStorage.getItem('atlas-theme'); if (saved) document.documentElement.dataset.theme = saved; window.setAtlasTheme = (value) => { document.documentElement.dataset.theme = value; localStorage.setItem('atlas-theme', value); }; document.addEventListener('DOMContentLoaded', () => { if (saved) document.getElementById('theme').value = saved; }); })();"))
                }
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
                    strong { "The shelf is empty." }
                    p { "Add the first useful thing you find." }
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
                        a class="button button-secondary" href={"/links/" (link.id) "/edit"} { "Edit" }
                        button type="button" class="button-danger" hx-delete={"/links/" (link.id)}
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
#[allow(clippy::too_many_lines)]
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
        div class="section-heading" {
            h2 { "Add to the shelf" }
            p { "A small archive of useful things." }
        }
        form class="form-card" hx-post="/links" hx-target="#link-list" hx-swap="outerHTML" hx-on::after-request="this.reset()" {
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
                label for="tags" { "Tags" }
                input type="text" id="tags" name="tags" placeholder="rust, reading, tools";
            }
            button class="button-primary" type="submit" { "Save link" }
        }
    };

    let filter_html = if params.tag.is_some() || params.q.is_some() {
        html! {
            div class="filter-form" {
                p { "Showing" }
                @if let Some(tag) = &params.tag {
                    span { "Tag: " (tag) }
                }
                @if let Some(q) = &params.q {
                    span { "Search: " (q) }
                }
                a href="/" { "Clear filters" }
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
        div class="shell" {
            aside class="rail" {
                a class="brand" href="/" aria-label="Link Atlas home" {
                    span class="brand-mark" { "L" }
                    span class="brand-name" { "link atlas" }
                }
                p class="rail-note" { "A living index for the things worth returning to." }
                div class="rail-foot" {
                    div class="rail-rule" {}
                    div class="theme-picker" {
                        label for="theme" { "Palette" }
                        select id="theme" onchange="setAtlasTheme(this.value)" {
                            option value="light" { "Paper / light" }
                            option value="dark" { "Charcoal / dark" }
                            option value="kanagawa" { "Kanagawa" }
                            option value="catppuccin" { "Catppuccin" }
                            option value="nord" { "Nord" }
                            option value="gruvbox" { "Gruvbox" }
                            option value="dracula" { "Dracula" }
                            option value="tokyo-night" { "Tokyo Night" }
                            option value="rose-pine" { "Rose Pine" }
                        }
                    }
                }
            }
            main class="main" {
                div class="masthead" {
                    div {
                        p class="eyebrow" { "Personal index / live shelf" }
                        h1 { "Keep the good " em { "stuff." } }
                    }
                    p class="masthead-copy" { "Save the references, tools, and ideas that deserve a second visit." }
                }
                div class="content-grid" {
                    section class="add-panel" { (form_html) }
                    section class="library-panel" {
                        div class="section-heading" {
                            h2 { "Your collection" }
                            p { "Updated live" }
                        }
                        (filter_html)
                        (list_html)
                    }
                }
            }
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
        div class="main edit-shell" {
            p class="eyebrow" { "Personal index / revise entry" }
            h1 { "Edit this " em { "reference." } }
            form class="form-card" hx-put={"/links/" (link.id)} hx-target="#link-list" hx-swap="outerHTML" {
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
                div class="edit-actions" {
                    button class="button-primary" type="submit" { "Save changes" }
                    a class="button button-secondary" href="/" { "Cancel" }
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
        .route("/static/tailwind.css", axum::routing::get(tailwind_css))
        .route("/static/favicon.svg", axum::routing::get(favicon))
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
