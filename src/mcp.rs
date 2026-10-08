//! MCP (Model Context Protocol) server over streamable HTTP.
//! Exposes link management tools for the internal dashboard.

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db;
use crate::events::AppState;
use crate::models::{LinkSort, NewLink, UpdateLink};

/// Parameters for searching links
#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema, Serialize)]
pub struct SearchParams {
    /// Full-text search query across title, URL, and description
    pub query: String,
    /// Maximum number of results to return (default 20, max 500)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Result order: newest (default), title (A–Z) or clicks (most clicked)
    #[serde(default)]
    pub sort: Option<LinkSort>,
}

/// Parameters for listing links
#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema, Serialize)]
pub struct ListParams {
    /// Optional tag to filter results
    pub tag: Option<String>,
    /// Result order: newest (default), title (A–Z) or clicks (most clicked)
    #[serde(default)]
    pub sort: Option<LinkSort>,
}

/// Parameters for adding a new link
#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema, Serialize)]
pub struct AddLinkParams {
    /// Target URL (must start with http:// or https://)
    pub url: String,
    /// Human-readable title
    pub title: String,
    /// Optional longer description
    pub description: Option<String>,
    /// Optional comma-separated tags or array of tags
    #[serde(default, deserialize_with = "crate::models::de_opt_tags")]
    pub tags: Option<Vec<String>>,
}

/// Parameters for updating an existing link. Omitted fields are left unchanged.
#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema, Serialize)]
pub struct UpdateLinkParams {
    /// UUID of the link to update
    pub id: String,
    /// New URL (must start with http:// or https://)
    pub url: Option<String>,
    /// New title
    pub title: Option<String>,
    /// New description
    pub description: Option<String>,
    /// Replacement tags, as an array or a comma-separated string.
    /// This replaces the existing tags rather than appending to them.
    #[serde(default, deserialize_with = "crate::models::de_opt_tags")]
    pub tags: Option<Vec<String>>,
}

/// Parameters for deleting a link
#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema, Serialize)]
pub struct DeleteParams {
    /// UUID of the link to delete
    pub id: String,
}

/// The MCP server implementation holding application state.
#[derive(Clone)]
pub struct LinksServer {
    state: AppState,
}

impl LinksServer {
    /// Wrap the shared application state for the MCP tools.
    #[must_use]
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tool_router]
impl LinksServer {
    /// List all links, optionally filtered by tag.
    /// Newest links are returned first unless `sort` says otherwise.
    #[tool(description = "List all links, optionally filtered by tag")]
    async fn list_links(&self, params: Parameters<ListParams>) -> Result<String, String> {
        let tag = params.0.tag.as_deref();
        let sort = params.0.sort.unwrap_or_default();
        match db::list_links(&self.state.pool, tag, sort).await {
            Ok(links) => Ok(serde_json::to_string(&links).unwrap_or_default()),
            Err(e) => Err(format!("Failed to list links: {e}")),
        }
    }

    /// Search for links by full-text query across title, URL, and description.
    /// Results are newest first unless `sort` says otherwise.
    #[tool(description = "Search for links by query (searches title, URL, and description)")]
    async fn search_links(&self, params: Parameters<SearchParams>) -> Result<String, String> {
        let limit = params.0.limit.unwrap_or(20).clamp(1, 500);
        let sort = params.0.sort.unwrap_or_default();
        match db::search_links(&self.state.pool, &params.0.query, limit, sort).await {
            Ok(links) => Ok(serde_json::to_string(&links).unwrap_or_default()),
            Err(e) => Err(format!("Failed to search links: {e}")),
        }
    }

    /// Add a new link to the dashboard.
    /// Validates the URL (must be http:// or https://) and required fields.
    /// Publishes a `LinkEvent` so open browser tabs update live.
    #[tool(description = "Add a new link to the dashboard")]
    async fn add_link(&self, params: Parameters<AddLinkParams>) -> Result<String, String> {
        let mut new_link = NewLink {
            url: params.0.url,
            title: params.0.title,
            description: params.0.description,
            tags: params.0.tags.unwrap_or_default(),
        };

        // Validate the link
        if let Err(e) = new_link.validate() {
            return Err(format!("Invalid link: {e}"));
        }

        // Create in database
        match db::create_link(&self.state.pool, &new_link).await {
            Ok(link) => {
                // Publish event so browser tabs update live
                self.state
                    .publish(crate::events::LinkEvent::Created(link.clone()));
                Ok(serde_json::to_string(&link).unwrap_or_default())
            }
            Err(e) => Err(format!("Failed to create link: {e}")),
        }
    }

    /// Update an existing link. Omitted fields keep their current value.
    /// Publishes a `LinkEvent` so open browser tabs update live.
    #[tool(
        description = "Update an existing link by UUID. Only the fields you supply are changed; supplying tags replaces the existing tags"
    )]
    async fn update_link(&self, params: Parameters<UpdateLinkParams>) -> Result<String, String> {
        let id =
            Uuid::parse_str(&params.0.id).map_err(|_| format!("Invalid UUID: {}", params.0.id))?;

        let mut update = UpdateLink {
            url: params.0.url,
            title: params.0.title,
            description: params.0.description,
            tags: params.0.tags,
        };

        // The same validation the API and UI apply.
        if let Err(e) = update.validate() {
            return Err(format!("Invalid link: {e}"));
        }

        match db::update_link(&self.state.pool, id, &update).await {
            Ok(Some(link)) => {
                self.state
                    .publish(crate::events::LinkEvent::Updated(link.clone()));
                Ok(serde_json::to_string(&link).unwrap_or_default())
            }
            Ok(None) => Err(format!("Link not found: {id}")),
            Err(e) => Err(format!("Failed to update link: {e}")),
        }
    }

    /// Delete a link by ID.
    /// Publishes a `LinkEvent` so open browser tabs update live.
    #[tool(description = "Delete a link by UUID")]
    async fn delete_link(&self, params: Parameters<DeleteParams>) -> Result<String, String> {
        // Parse the UUID from the string
        let id =
            Uuid::parse_str(&params.0.id).map_err(|_| format!("Invalid UUID: {}", params.0.id))?;

        // Delete from database
        match db::delete_link(&self.state.pool, id).await {
            Ok(deleted) => {
                if deleted {
                    // Publish event so browser tabs update live
                    self.state.publish(crate::events::LinkEvent::Deleted(id));
                    Ok(serde_json::json!({ "deleted": true }).to_string())
                } else {
                    Err(format!("Link not found: {id}"))
                }
            }
            Err(e) => Err(format!("Failed to delete link: {e}")),
        }
    }
}

#[tool_handler(
    name = "internal-dashboard",
    version = "0.1.0",
    instructions = "MCP server for the internal link dashboard. Use these tools to manage links programmatically."
)]
impl ServerHandler for LinksServer {}

/// Factory function to create the streamable HTTP service.
/// Used by main.rs to nest the MCP server into the axum router at `/mcp`.
///
/// Reads `MCP_ALLOWED_HOSTS` (comma-separated) to widen rmcp's default
/// DNS-rebinding protection, which otherwise only accepts a loopback `Host`
/// header -- so anything reached through a reverse proxy on a real hostname
/// (Caddy, an MCP aggregator like axon-gateway) gets a 403 unless the
/// fronting name is listed here.
#[must_use]
pub fn service(state: AppState) -> StreamableHttpService<LinksServer, LocalSessionManager> {
    let handler = LinksServer::new(state);
    let session_manager = Arc::new(LocalSessionManager::default());
    let mut config = StreamableHttpServerConfig::default();
    if let Some(hosts) = parse_allowed_hosts(std::env::var("MCP_ALLOWED_HOSTS").ok().as_deref()) {
        config = config.with_allowed_hosts(hosts);
    }

    StreamableHttpService::new(move || Ok(handler.clone()), session_manager, config)
}

/// Parse a comma-separated allow-list, treating "set but empty" as unset.
///
/// Returning `Some(vec![])` would be actively dangerous: `service` passes it to
/// rmcp's `with_allowed_hosts`, and an empty allow-list rejects *every* inbound
/// `Host` header -- a typo'd or empty `MCP_ALLOWED_HOSTS` would silently produce
/// a server that accepts no connections at all. Collapsing that to `None` falls
/// back to rmcp's loopback-only default instead, the safe reading of "unset".
fn parse_allowed_hosts(raw: Option<&str>) -> Option<Vec<String>> {
    let hosts: Vec<String> = raw?
        .split(',')
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();

    if hosts.is_empty() { None } else { Some(hosts) }
}

#[cfg(test)]
mod tests {
    use super::parse_allowed_hosts;

    #[test]
    fn unset_is_none() {
        assert_eq!(parse_allowed_hosts(None), None);
    }

    #[test]
    fn splits_trims_and_drops_empty_items() {
        assert_eq!(
            parse_allowed_hosts(Some(" a.example.com , localhost ,, ")),
            Some(vec!["a.example.com".to_string(), "localhost".to_string()])
        );
    }

    #[test]
    fn blank_collapses_to_none() {
        for blank in [" , ", "", ",", "   ", ",,,"] {
            assert_eq!(
                parse_allowed_hosts(Some(blank)),
                None,
                "{blank:?} should collapse to None"
            );
        }
    }
}
