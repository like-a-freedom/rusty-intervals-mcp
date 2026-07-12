//! Rust-native Critical Power regression.
//! Implements the 2-parameter CP model: P(t) = CP + W'/t
//! Uses linear regression on transformed data (P·t = CP·t + W').

// =============================================================================
// CP Regression Constants
// =============================================================================

/// Minimum number of data points required for valid CP regression.
const MIN_CP_DATA_POINTS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct CpFitDiagnostics {
    pub sample_count: usize,
    pub min_duration_secs: f64,
    pub max_duration_secs: f64,
    pub rmse_watts: f64,
    pub max_abs_residual_watts: f64,
    pub cp_standard_error_watts: Option<f64>,
    pub w_prime_standard_error_joules: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpResult {
    pub cp: f64,
    pub w_prime: f64,
    pub r_squared: f64,
    pub valid: bool,
    pub diagnostics: CpFitDiagnostics,
}

/// Fit 2-parameter CP model from (duration_secs, power_watts) data points.
/// Minimum 3 data points required. Returns None for invalid data.
/// `valid` means only that the two-parameter regression is mathematically
/// identifiable, finite, and yields positive CP/W'.
pub fn fit_cp(data: &[(f64, f64)]) -> Option<CpResult> {
    if data.len() < MIN_CP_DATA_POINTS {
        return None;
    }

    let n = data.len() as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_x2 = 0.0;

    for &(t, p) in data {
        let work = p * t;
        sum_x += t;
        sum_y += work;
        sum_xy += t * work;
        sum_x2 += t * t;
    }

    let denominator = n * sum_x2 - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    // Linear regression: y = a + b*x, where x = t, y = P*t
    // a = W' (intercept), b = CP (slope)
    let cp = (n * sum_xy - sum_x * sum_y) / denominator;
    let w_prime = (sum_y - cp * sum_x) / n;
    let y_mean = sum_y / n;

    let ss_res = data
        .iter()
        .map(|&(t, p)| {
            let predicted = cp * t + w_prime;
            let work = p * t;
            (work - predicted) * (work - predicted)
        })
        .sum::<f64>();

    let ss_tot = data
        .iter()
        .map(|&(t, p)| {
            let work = p * t;
            (work - y_mean) * (work - y_mean)
        })
        .sum::<f64>();

    let r_squared = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };

    // Compute diagnostics
    let residuals: Vec<f64> = data
        .iter()
        .map(|&(t, p)| {
            let predicted = cp * t + w_prime;
            let work = p * t;
            work - predicted
        })
        .collect();

    let rmse = (ss_res / n).sqrt();
    let max_abs_residual = residuals.iter().map(|r| r.abs()).fold(0.0f64, f64::max);

    // Parameter standard errors
    let residual_variance = if n > 2.0 { ss_res / (n - 2.0) } else { 0.0 };
    let sxx = sum_x2 - (sum_x * sum_x) / n;

    let cp_se = if sxx > 0.0 {
        Some((residual_variance / sxx).sqrt())
    } else {
        None
    };

    let w_prime_se = if sxx > 0.0 && n > 2.0 {
        let se = (residual_variance * (1.0 / n + (sum_x / n).powi(2) / sxx)).sqrt();
        Some(se)
    } else {
        None
    };

    let min_duration = data.iter().map(|&(t, _)| t).fold(f64::INFINITY, f64::min);
    let max_duration = data.iter().map(|&(t, _)| t).fold(0.0f64, f64::max);

    let diagnostics = CpFitDiagnostics {
        sample_count: data.len(),
        min_duration_secs: min_duration,
        max_duration_secs: max_duration,
        rmse_watts: rmse,
        max_abs_residual_watts: max_abs_residual,
        cp_standard_error_watts: cp_se,
        w_prime_standard_error_joules: w_prime_se,
    };

    // Valid means: mathematically identifiable, finite, and yields positive CP/W'
    let valid = cp > 0.0 && w_prime > 0.0 && cp.is_finite() && w_prime.is_finite();

    Some(CpResult {
        cp,
        w_prime,
        r_squared,
        valid,
        diagnostics,
    })
}

/// Validate fitted CP against known API eFTP/W'.
/// Returns % difference for CP and W' separately.
pub fn validate_cp(result: &CpResult, api_ftp: f64, api_wp: f64) -> (f64, f64) {
    let cp_diff = if api_ftp > 0.0 {
        ((result.cp - api_ftp) / api_ftp).abs() * 100.0
    } else {
        0.0
    };
    let wp_diff = if api_wp > 0.0 {
        ((result.w_prime - api_wp) / api_wp).abs() * 100.0
    } else {
        0.0
    };
    (cp_diff, wp_diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp_fit_synthetic_perfect_data() {
        // P(t) = 250 + 20000/t → at t=60: P=583.3, t=300: P=316.7, t=1200: P=266.7, t=3600: P=255.6
        let cp_true = 250.0;
        let wp_true = 20000.0;
        let data = vec![60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0]
            .into_iter()
            .map(|t| (t, cp_true + wp_true / t))
            .collect::<Vec<_>>();

        let result = fit_cp(&data).unwrap();
        assert!((result.cp - cp_true).abs() < 2.0);
        assert!((result.w_prime - wp_true).abs() < 500.0);
        assert!(result.r_squared > 0.99);
        assert!(result.valid);
        assert_eq!(result.diagnostics.sample_count, 7);
        assert!(result.diagnostics.rmse_watts < 1.0);
        assert!(result.diagnostics.max_abs_residual_watts < 1.0);
    }

    #[test]
    fn cp_fit_rejects_few_data_points() {
        let data = vec![(60.0, 300.0), (300.0, 280.0)];
        assert!(fit_cp(&data).is_none());
    }

    #[test]
    fn cp_fit_realistic_data() {
        // Synthetic realistic power-duration data
        let data = vec![
            (5.0, 900.0),
            (30.0, 550.0),
            (60.0, 450.0),
            (300.0, 310.0),
            (1200.0, 275.0),
            (3600.0, 255.0),
        ];
        let result = fit_cp(&data).unwrap();
        assert!(result.cp > 200.0 && result.cp < 300.0);
        assert!(result.w_prime > 10000.0 && result.w_prime < 30000.0);
        assert!(result.r_squared > 0.8);
        assert_eq!(result.diagnostics.sample_count, 6);
        assert!(result.diagnostics.rmse_watts > 0.0);
    }

    #[test]
    fn validate_cp_diff_zero_on_match() {
        let result = CpResult {
            cp: 250.0,
            w_prime: 20000.0,
            r_squared: 0.99,
            valid: true,
            diagnostics: CpFitDiagnostics {
                sample_count: 7,
                min_duration_secs: 60.0,
                max_duration_secs: 3600.0,
                rmse_watts: 1.0,
                max_abs_residual_watts: 2.0,
                cp_standard_error_watts: Some(1.0),
                w_prime_standard_error_joules: Some(100.0),
            },
        };
        let (cp_diff, wp_diff) = validate_cp(&result, 250.0, 20000.0);
        assert!(cp_diff < 0.01);
        assert!(wp_diff < 0.01);
    }

    #[test]
    fn validate_cp_diff_computes_percent() {
        let result = CpResult {
            cp: 255.0,
            w_prime: 21000.0,
            r_squared: 0.99,
            valid: true,
            diagnostics: CpFitDiagnostics {
                sample_count: 7,
                min_duration_secs: 60.0,
                max_duration_secs: 3600.0,
                rmse_watts: 1.0,
                max_abs_residual_watts: 2.0,
                cp_standard_error_watts: Some(1.0),
                w_prime_standard_error_joules: Some(100.0),
            },
        };
        let (cp_diff, wp_diff) = validate_cp(&result, 250.0, 20000.0);
        assert!((cp_diff - 2.0).abs() < 0.01);
        assert!((wp_diff - 5.0).abs() < 0.01);
    }

    #[test]
    fn cp_fit_inconsistent_data_returns_some_result() {
        let data = vec![
            (60.0, 300.0),
            (300.0, 100.0),
            (600.0, 500.0),
            (1200.0, 150.0),
            (3600.0, 600.0),
        ];
        let result = fit_cp(&data);
        assert!(result.is_some());
    }
}
