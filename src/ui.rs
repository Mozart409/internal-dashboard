//! Server-rendered HTMX UI with Maud templates and SSE real-time updates.

use axum::{
    Form, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{AppendHeaders, IntoResponse},
};
use chrono::{DateTime, Utc};
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::events::{AppState, LinkEvent};
use crate::models::{Link, LinkSort, NewLink, UpdateLink};

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

/// Cookie remembering the sort the UI toggle last picked. The list fragments
/// returned by mutations and pushed over SSE read it too, so a live update
/// keeps the order the viewer chose.
const SORT_COOKIE: &str = "link-sort";

/// The sort stored in the request's sort cookie, if any (and if valid).
#[must_use]
pub fn sort_cookie(headers: &HeaderMap) -> Option<LinkSort> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(SORT_COOKIE)?.strip_prefix('='))
        .and_then(LinkSort::parse)
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
                        // The href stays the real URL (hover, copy, middle-click
                        // all behave); the beacon counts the click on the side.
                        a href=(link.url) target="_blank" rel="noopener noreferrer"
                            onclick={"navigator.sendBeacon('/links/" (link.id) "/click')"}
                            onauxclick={"if (event.button === 1) navigator.sendBeacon('/links/" (link.id) "/click')"} {
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
    headers: HeaderMap,
    Query(params): Query<IndexParams>,
) -> Result<impl IntoResponse, AppError> {
    // An explicit `?sort=` wins and is remembered; otherwise the cookie decides.
    // Unknown values are ignored rather than rejected.
    let picked = params.sort.as_deref().and_then(LinkSort::parse);
    let sort = picked.or_else(|| sort_cookie(&headers)).unwrap_or_default();
    let set_cookie = picked.map(|sort| {
        (
            header::SET_COOKIE,
            format!(
                "{SORT_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax",
                sort.as_str()
            ),
        )
    });

    let links = if let Some(q) = &params.q {
        crate::db::search_links(&state.pool, q, 100, sort).await?
    } else {
        crate::db::list_links(&state.pool, params.tag.as_deref(), sort).await?
    };

    let form_html = html! {
        div class="section-heading" {
            h2 { "Add to the shelf" }
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

    // A plain GET form, so the browser encodes the tag/search it carries along.
    let sort_html = html! {
        form class="sort-toggle" method="get" action="/" role="group" aria-label="Sort links" {
            @if let Some(tag) = &params.tag {
                input type="hidden" name="tag" value=(tag);
            }
            @if let Some(q) = &params.q {
                input type="hidden" name="q" value=(q);
            }
            span class="sort-label" { "Sort" }
            @for option in LinkSort::ALL {
                button type="submit" name="sort" value=(option.as_str())
                        aria-pressed=(if option == sort { "true" } else { "false" }) {
                    (option.label())
                }
            }
        }
    };

    let body = html! {
        div class="shell" {
            main class="main" {
                div class="masthead" {
                    div {
                        h1 { "Keep the good " em { "stuff." } }
                        p class="masthead-copy" { "Save the references, tools, and ideas that deserve a second visit." }
                    }
                    div class="masthead-controls" {
                        button type="button" class="add-toggle" onclick="document.getElementById('add-form').toggleAttribute('hidden')" {
                            (maud::PreEscaped(r#"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15"/></svg>"#))
                            "Add link"
                        }
                        div class="theme-picker" {
                            select id="theme" onchange="setAtlasTheme(this.value)" {
                                option value="light" { "Light" }
                                option value="dark" { "Dark" }
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
                div class="content-grid" {
                    div id="add-form" class="add-form-panel" hidden {
                        (form_html)
                    }
                    section class="library-panel" {
                        (filter_html)
                        (sort_html)
                        div hx-ext="sse" sse-connect="/events" {
                            (render_link_list(&links))
                        }
                    }
                }
            }
        }
    };

    Ok((
        AppendHeaders(set_cookie),
        layout("Links Dashboard", body),
    ))
}

#[derive(Debug, Deserialize)]
pub struct IndexParams {
    tag: Option<String>,
    q: Option<String>,
    sort: Option<String>,
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
    headers: HeaderMap,
    Form(mut new_link): Form<NewLink>,
) -> Result<Markup, AppError> {
    new_link.validate()?;

    let link = crate::db::create_link(&state.pool, &new_link).await?;
    state.publish(LinkEvent::Created(link));

    let sort = sort_cookie(&headers).unwrap_or_default();
    let links = crate::db::list_links(&state.pool, None, sort).await?;
    Ok(render_link_list(&links))
}

/// PUT /links/{id} — Update a link.
///
/// # Errors
/// Returns error if the link is not found or database operation fails.
pub async fn update_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(update): Form<UpdateLink>,
) -> Result<Markup, AppError> {
    let link = crate::db::update_link(&state.pool, id, &update)
        .await?
        .ok_or(AppError::NotFound)?;

    state.publish(LinkEvent::Updated(link));

    let sort = sort_cookie(&headers).unwrap_or_default();
    let links = crate::db::list_links(&state.pool, None, sort).await?;
    Ok(render_link_list(&links))
}

/// DELETE /links/{id} — Delete a link.
///
/// # Errors
/// Returns error if the link is not found or database operation fails.
pub async fn delete_link_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Markup, AppError> {
    let deleted = crate::db::delete_link(&state.pool, id).await?;

    if !deleted {
        return Err(AppError::NotFound);
    }

    state.publish(LinkEvent::Deleted(id));

    let sort = sort_cookie(&headers).unwrap_or_default();
    let links = crate::db::list_links(&state.pool, None, sort).await?;
    Ok(render_link_list(&links))
}

/// POST /links/{id}/click — Count a click; sent as a beacon by the link anchor.
///
/// Deliberately publishes no event: a click is not an edit, and re-rendering
/// every open list on each click would reorder it under the viewer.
///
/// # Errors
/// Returns [`AppError::NotFound`] if no link has that id, or
/// [`AppError::Database`] if the update fails.
pub async fn record_click(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if crate::db::record_click(&state.pool, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

/// Build the UI router with all HTML routes.
///
/// # Returns
/// An Axum router configured with GET /, GET /links/{id}/edit, POST /links, PUT /links/{id},
/// DELETE /links/{id} and POST /links/{id}/click.
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
        .route("/links/{id}/click", axum::routing::post(record_click))
}

/// Format a UTC datetime for display.
fn format_date(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M UTC").to_string()
}
