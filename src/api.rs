use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::error::AppError;
use crate::events::{AppState, LinkEvent};
use crate::models::{Link, NewLink, UpdateLink};

/// Query parameters for list endpoints.
#[derive(Debug, Deserialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Filter by tag (exact match).
    pub tag: Option<String>,
    /// Search query for substring match.
    pub q: Option<String>,
}

/// Get all links with optional filtering by tag or search query.
///
/// # Errors
///
/// Returns `AppError::Database` if the query fails.
#[utoipa::path(
    get,
    path = "/links",
    tag = "links",
    params(ListQuery),
    responses(
        (status = 200, description = "List of links", body = Vec<Link>),
        (status = 500, description = "Internal server error")
    )
)]
async fn list_links(
    State(state): State<AppState>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<Link>>, AppError> {
    let links = if let Some(q) = params.q {
        crate::db::search_links(&state.pool, &q, 100).await?
    } else {
        crate::db::list_links(&state.pool, params.tag.as_deref()).await?
    };

    Ok(Json(links))
}

/// Get a specific link by ID.
///
/// # Errors
///
/// Returns `AppError::NotFound` if the link does not exist.
/// Returns `AppError::Database` if the query fails.
#[utoipa::path(
    get,
    path = "/links/{id}",
    tag = "links",
    params(
        ("id" = Uuid, Path, description = "Link ID")
    ),
    responses(
        (status = 200, description = "Link found", body = Link),
        (status = 404, description = "Link not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Link>, AppError> {
    let link = crate::db::get_link(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(link))
}

/// Create a new link.
///
/// # Errors
///
/// Returns `AppError::Invalid` if validation fails (e.g., empty url/title, invalid URL scheme).
/// Returns `AppError::Database` if the insert fails.
#[utoipa::path(
    post,
    path = "/links",
    tag = "links",
    request_body = NewLink,
    responses(
        (status = 201, description = "Link created", body = Link),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
async fn create_link(
    State(state): State<AppState>,
    Json(mut new): Json<NewLink>,
) -> Result<(StatusCode, Json<Link>), AppError> {
    new.validate()?;
    let link = crate::db::create_link(&state.pool, &new).await?;

    state.publish(LinkEvent::Created(link.clone()));

    Ok((StatusCode::CREATED, Json(link)))
}

/// Update a link partially.
///
/// # Errors
///
/// Returns `AppError::NotFound` if the link does not exist.
/// Returns `AppError::Database` if the update fails.
#[utoipa::path(
    put,
    path = "/links/{id}",
    tag = "links",
    params(
        ("id" = Uuid, Path, description = "Link ID")
    ),
    request_body = UpdateLink,
    responses(
        (status = 200, description = "Link updated", body = Link),
        (status = 404, description = "Link not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(up): Json<UpdateLink>,
) -> Result<Json<Link>, AppError> {
    let link = crate::db::update_link(&state.pool, id, &up)
        .await?
        .ok_or(AppError::NotFound)?;

    state.publish(LinkEvent::Updated(link.clone()));

    Ok(Json(link))
}

/// Delete a link by ID.
///
/// # Errors
///
/// Returns `AppError::NotFound` if the link does not exist.
/// Returns `AppError::Database` if the delete fails.
#[utoipa::path(
    delete,
    path = "/links/{id}",
    tag = "links",
    params(
        ("id" = Uuid, Path, description = "Link ID")
    ),
    responses(
        (status = 204, description = "Link deleted"),
        (status = 404, description = "Link not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn delete_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let deleted = crate::db::delete_link(&state.pool, id).await?;

    if !deleted {
        return Err(AppError::NotFound);
    }

    state.publish(LinkEvent::Deleted(id));

    Ok(StatusCode::NO_CONTENT)
}

/// `OpenAPI` documentation metadata for the links API.
///
/// Contains title, version, description, tags, and shared component schemas.
/// Paths are auto-collected from handlers by `OpenApiRouter`.
#[derive(OpenApi)]
#[openapi(
    components(
        schemas(Link, NewLink, UpdateLink, ListQuery)
    ),
    tags(
        (name = "links", description = "Link management endpoints")
    )
)]
pub struct ApiDoc;

/// Build the `/links` router with `OpenAPI` documentation.
///
/// Returns an `OpenApiRouter` with all link management endpoints.
/// The router should be nested under `/api/v1` by the caller via:
///
/// ```ignore
/// let api_doc = ApiDoc::openapi();
/// let (router, api) = OpenApiRouter::with_openapi(api_doc)
///     .nest("/api/v1", api::router())
///     .split_for_parts();
/// ```
#[must_use]
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_links, create_link))
        .routes(routes!(get_link, update_link, delete_link))
}
