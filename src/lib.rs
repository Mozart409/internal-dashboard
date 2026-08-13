//! Internal link dashboard.
//!
//! Three surfaces — the HTMX UI, the REST API and the MCP server — sit on the
//! single data-access layer in [`db`]. The library target exists so integration
//! tests can build the real router via [`build_router`] rather than
//! reconstructing the wiring themselves.

pub mod api;
pub mod db;
pub mod error;
pub mod events;
pub mod mcp;
pub mod models;
pub mod sse;
pub mod ui;

use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable as _};
use utoipa_swagger_ui::SwaggerUi;

use crate::events::AppState;

/// Compose every surface into one router.
///
/// This is the single definition of the app's routing, shared by the binary and
/// the integration tests — so the tests exercise the real wiring, including the
/// `/api/v1` nesting and the axum path syntax.
pub fn build_router(state: AppState) -> axum::Router {
    // Handlers register themselves into the spec, so the docs cannot drift
    // from the routes.
    let (api_router, openapi) = OpenApiRouter::with_openapi(api::ApiDoc::openapi())
        .nest("/api/v1", api::router())
        .split_for_parts();

    axum::Router::new()
        .merge(ui::router())
        .merge(sse::router())
        .merge(api_router)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi.clone()))
        .merge(Scalar::with_url("/scalar", openapi))
        .nest_service("/mcp", mcp::service(state.clone()))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
