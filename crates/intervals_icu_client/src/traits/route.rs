//! Route service trait for athlete route operations.

use crate::Result;

/// Service for athlete route operations.
#[async_trait::async_trait]
pub trait RouteService: Send + Sync + 'static {
    /// List routes for the athlete.
    async fn list_routes(&self) -> Result<serde_json::Value>;

    /// Get a single route, optionally including the path coordinates.
    async fn get_route(&self, route_id: i64, include_path: bool) -> Result<serde_json::Value>;

    /// Update a single route.
    async fn update_route(
        &self,
        route_id: i64,
        route: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Compare two routes for similarity.
    async fn get_route_similarity(&self, route_id: i64, other_id: i64)
    -> Result<serde_json::Value>;
}
