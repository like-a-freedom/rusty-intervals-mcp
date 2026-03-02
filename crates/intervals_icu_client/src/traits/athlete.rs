//! Athlete service trait for profile-related operations.

use crate::{AthleteProfile, Result};

/// Service for athlete profile operations.
#[async_trait::async_trait]
pub trait AthleteService: Send + Sync + 'static {
    /// Get the athlete's profile information.
    async fn get_athlete_profile(&self) -> Result<AthleteProfile>;
}
