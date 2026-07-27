//! Weather configuration and route endpoints.
//!
//! Per ADR-0005, these methods are YAGNI candidates: kept on the trait surface
//! for parity with the upstream Intervals.icu API and for mock implementations
//! in `test_support.rs`. They satisfy ADR-0005 inclusion criterion #2 — at
//! least one integration test in `tests/http_client_contract.rs` exercises a
//! real `ReqwestIntervalsClient` code path against these endpoints, so the
//! implementations cannot be deleted.
//!
//! `#[doc(hidden)]` on the trait declarations keeps them out of rendered
//! `cargo doc` output until a real production caller appears.

use crate::Result;
use crate::http_client::ReqwestIntervalsClient;
use crate::utils::QueryBuilder;

impl ReqwestIntervalsClient {
    /// Get the athlete's weather configuration.
    pub(crate) async fn fetch_weather_config(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "weather-config"]);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Update the athlete's weather configuration.
    pub(crate) async fn fetch_update_weather_config(
        &self,
        config: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "weather-config"]);
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(config))
            .await
    }

    /// List the athlete's routes.
    pub(crate) async fn fetch_list_routes(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "routes"]);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Fetch a single route by ID.
    pub(crate) async fn fetch_route(
        &self,
        route_id: i64,
        include_path: bool,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "routes", &route_id.to_string()]);
        let pairs = QueryBuilder::new()
            .add("includePath", include_path)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Update a route.
    pub(crate) async fn fetch_update_route(
        &self,
        route_id: i64,
        route: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "routes", &route_id.to_string()]);
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(route))
            .await
    }

    /// Compare similarity between two routes.
    pub(crate) async fn fetch_route_similarity(
        &self,
        route_id: i64,
        other_id: i64,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "routes",
            &route_id.to_string(),
            "similarity",
            &other_id.to_string(),
        ]);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }
}
