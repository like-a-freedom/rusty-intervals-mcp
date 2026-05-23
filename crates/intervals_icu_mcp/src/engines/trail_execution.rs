use serde::{Deserialize, Serialize};

// =============================================================================
// Trail Execution Constants
// Sources: Vernillo et al. Sports Med 2017, Di Prampero et al. JAP 1986
// =============================================================================

/// KM per meter conversion factor.
const METERS_PER_KM: f64 = 1000.0;

/// Seconds per hour conversion factor.
const SECONDS_PER_HOUR: f64 = 3600.0;

/// Terrain index threshold for steep classification (m/km).
/// >20 m/km = steep terrain per Vernillo classification.
const TERRAIN_STEEP_THRESHOLD: f64 = 20.0;

/// Efficiency drift threshold for terrain-induced fatigue (%).
/// Drift >5% drop = terrain-induced efficiency loss.
const EFFICIENCY_DRIFT_THRESHOLD: f64 = -5.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerrainContext {
    pub terrain_index: Option<f64>,
    pub vam: Option<f64>,
    pub efficiency_drift: Option<f64>,
    pub terrain_induced: bool,
    pub supported: bool,
}

impl TerrainContext {
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            terrain_index: None,
            vam: None,
            efficiency_drift: None,
            terrain_induced: false,
        }
    }
}

/// Compute terrain index = elevation_gain (m) / distance (km).
pub fn compute_terrain_index(elevation_gain_m: f64, distance_m: f64) -> Option<f64> {
    if distance_m <= 0.0 {
        return None;
    }
    Some(elevation_gain_m / (distance_m / METERS_PER_KM))
}

/// Compute VAM = elevation_gain / (moving_time_secs / 3600).
pub fn compute_vam(elevation_gain_m: f64, moving_time_secs: i64) -> Option<f64> {
    if moving_time_secs <= 0 {
        return None;
    }
    Some(elevation_gain_m / (moving_time_secs as f64 / SECONDS_PER_HOUR))
}

/// Compute terrain execution context from activity detail.
/// terrain is context modifier, NOT load — metabolic cost already in TSS.
pub fn compute_terrain_context(
    elevation_gain_m: f64,
    distance_m: f64,
    moving_time_secs: i64,
    efficiency_drift: Option<f64>,
) -> TerrainContext {
    let terrain_index = compute_terrain_index(elevation_gain_m, distance_m);
    let vam = compute_vam(elevation_gain_m, moving_time_secs);

    let terrain_induced = terrain_index
        .map(|ti| ti > TERRAIN_STEEP_THRESHOLD && efficiency_drift.map(|ed| ed < EFFICIENCY_DRIFT_THRESHOLD).unwrap_or(false))
        .unwrap_or(false);

    TerrainContext {
        terrain_index,
        vam,
        efficiency_drift,
        terrain_induced,
        supported: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_index_flat() {
        let ti = compute_terrain_index(50.0, 10000.0).unwrap();
        assert!((ti - 5.0).abs() < 0.01);
    }

    #[test]
    fn terrain_index_mountain() {
        let ti = compute_terrain_index(1200.0, 10000.0).unwrap();
        assert!((ti - 120.0).abs() < 0.01);
    }

    #[test]
    fn terrain_index_zero_distance() {
        assert!(compute_terrain_index(100.0, 0.0).is_none());
    }

    #[test]
    fn vam_computation() {
        let vam = compute_vam(800.0, 3600).unwrap();
        assert!((vam - 800.0).abs() < 0.01);
    }

    #[test]
    fn vam_zero_time() {
        assert!(compute_vam(800.0, 0).is_none());
    }

    #[test]
    fn terrain_context_flat() {
        let ctx = compute_terrain_context(50.0, 10000.0, 3600, None);
        assert!(ctx.supported);
        assert!(!ctx.terrain_induced);
    }

    #[test]
    fn terrain_context_mountain_induced() {
        let ctx = compute_terrain_context(1200.0, 10000.0, 5400, Some(-8.0));
        assert!(ctx.supported);
        assert!(ctx.terrain_induced);
    }
}
