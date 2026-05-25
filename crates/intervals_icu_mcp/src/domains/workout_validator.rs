//! Workout Builder syntax validation for Intervals.icu.
//!
//! Parses step durations from structured workout descriptions, validates
//! syntax patterns, and detects common LLM mistakes before they reach the API.
//!
//! # What this catches
//!
//! - `min`/`sec` instead of `m`/`s`/`h` for step durations
//! - Missing `%` on ramp boundaries
//! - Mismatch between step duration sum and `new_duration` field
//!
//! Wire this into `modify_training` (especially `dry_run` mode) to give
//! agents early feedback.

/// A single validation warning.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutWarning {
    pub category: &'static str,
    pub message: String,
}

/// Parsed step duration in seconds plus source line.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedStep {
    pub line: usize,
    pub text: String,
    pub duration_seconds: u32,
}

/// Result of validating a workout description.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutValidation {
    /// Sum of all step durations (seconds).
    pub total_step_seconds: u32,
    /// Number of steps found.
    pub step_count: usize,
    /// Warnings collected during validation.
    pub warnings: Vec<WorkoutWarning>,
}

// ---------------------------------------------------------------------------
// Duration parsing
// ---------------------------------------------------------------------------

/// Parse a single duration from a step line.
///
/// Supports formats matched against a step line:
/// - `30s` → 30
/// - `10m` → 600
/// - `1h` → 3600
/// - `1h30m` → 5400
/// - `1h30m59s` → 5459
/// - `5m30s` → 330
/// - `1m30` → 90  (short form, seconds without trailing `s`)
///
/// Returns `None` if no duration pattern is found.
fn parse_step_duration(line: &str) -> Option<u32> {
    let line = line.trim();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Find the first digit
    while i < len && !bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i >= len {
        return None;
    }

    let mut total: u32 = 0;
    let mut found = false;

    while i < len {
        // Skip non-digit
        while i < len && !bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i >= len {
            break;
        }

        // Collect digits
        let num_start = i;
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let num: u32 = line[num_start..i].parse().ok()?;

        // Check unit suffix
        if i >= len {
            // Number at end with no unit — only valid as seconds in short form
            // (e.g. "1m30"). We don't add it here because the 'm' handler
            // already peeled off the minutes portion.
            break;
        }

        match bytes[i] {
            b'h' => {
                total += num * 3600;
                found = true;
                i += 1;
            }
            b'm' => {
                total += num * 60;
                found = true;
                i += 1;

                // Check for short-form seconds: "1m30" (digit after 'm')
                let sec_start = i;
                let mut sec_end = sec_start;
                while sec_end < len && bytes[sec_end].is_ascii_digit() {
                    sec_end += 1;
                }
                if sec_end > sec_start {
                    let sec: u32 = line[sec_start..sec_end].parse().ok()?;
                    total += sec; // already in seconds
                    i = sec_end;
                }
            }
            b's' => {
                total += num;
                found = true;
                i += 1;
            }
            b'\'' => {
                // ' for minutes
                total += num * 60;
                found = true;
                i += 1;

                // Check for " after: 5'30"
                if i < len && bytes[i] == b'"' {
                    // Need the seconds number between ' and ", which we
                    // already consumed as part of skipping non-digit.
                    // This case is handled below.
                }
            }
            b'"' => {
                total += num;
                found = true;
                i += 1;
            }
            _ => {
                // Unknown suffix — skip
                i += 1;
            }
        }
    }

    if found { Some(total) } else { None }
}

// ---------------------------------------------------------------------------
// Step extraction
// ---------------------------------------------------------------------------

/// Extract all step lines from a workout description and parse their durations.
///
/// A step is any line starting with `- ` (dash-space). Section headers
/// (Warmup, Main Set, Cooldown) are ignored.
pub fn parse_workout_steps(description: &str) -> Vec<ParsedStep> {
    let mut steps = Vec::new();

    for (idx, raw_line) in description.lines().enumerate() {
        let trimmed = raw_line.trim();
        if !trimmed.starts_with("- ") {
            continue;
        }

        if let Some(secs) = parse_step_duration(trimmed) {
            steps.push(ParsedStep {
                line: idx + 1,
                text: trimmed.to_string(),
                duration_seconds: secs,
            });
        }
    }

    steps
}

/// Sum all step durations in a workout description (seconds).
pub fn sum_step_durations(description: &str) -> u32 {
    parse_workout_steps(description)
        .iter()
        .map(|s| s.duration_seconds)
        .sum()
}

/// A section of a workout, with optional repeat count.
#[derive(Debug, Clone)]
struct WorkoutSection {
    repeat_count: u32,
    steps: Vec<ParsedStep>,
}

/// Extract a repeat count (e.g. `3x`) from a section header line.
fn extract_repeat_count(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i].is_ascii_digit() {
            let num_start = i;
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && bytes[i] == b'x' {
                return line[num_start..i].parse::<u32>().ok();
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Parse a workout description into sections with repeat counts.
///
/// Section headers (non-step lines) may carry a repeat suffix (`Nx`)
/// that multiplies the duration of all steps in that section.
fn parse_workout_sections(description: &str) -> Vec<WorkoutSection> {
    let mut sections: Vec<WorkoutSection> = Vec::new();
    let mut current_repeat: u32 = 1;

    for (idx, raw_line) in description.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("- ") {
            if let Some(secs) = parse_step_duration(trimmed) {
                let step = ParsedStep {
                    line: idx + 1,
                    text: trimmed.to_string(),
                    duration_seconds: secs,
                };
                match sections.last_mut() {
                    Some(section) => section.steps.push(step),
                    None => {
                        sections.push(WorkoutSection {
                            repeat_count: current_repeat,
                            steps: vec![step],
                        });
                    }
                }
            }
        } else {
            current_repeat = extract_repeat_count(trimmed).unwrap_or(1);
            sections.push(WorkoutSection {
                repeat_count: current_repeat,
                steps: Vec::new(),
            });
        }
    }

    sections
}

/// Sum all step durations accounting for section repeat counts.
pub fn sum_step_durations_with_repeats(description: &str) -> u32 {
    parse_workout_sections(description)
        .iter()
        .map(|section| {
            let step_sum: u32 = section.steps.iter().map(|s| s.duration_seconds).sum();
            step_sum * section.repeat_count
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Run all syntax checks on a workout description.
///
/// Returns warnings for common LLM mistakes. This is purely advisory —
/// Intervals.icu may accept text that triggers warnings, but the result
/// might not match expectations.
pub fn validate_workout_syntax(description: &str) -> Vec<WorkoutWarning> {
    let mut warnings: Vec<WorkoutWarning> = Vec::new();

    // Cache steps once
    let steps = parse_workout_steps(description);

    for step in &steps {
        // 1. Check for `min`/`sec`/`minutes`/`seconds` (common LLM mistake)
        // Only flag "min" when preceded by a digit (matches "10min", not "minimum")
        if step.text.contains("min")
            && let Some(pos) = step.text.find("min")
        {
            let before_char = pos
                .checked_sub(1)
                .and_then(|p| step.text[p..].chars().next());
            if before_char.is_some_and(|c| c.is_ascii_digit()) {
                warnings.push(WorkoutWarning {
                    category: "duration_format",
                    message: format!(
                        "Step line {}: use \"m\" instead of \"min\". Found: \"{}\"",
                        step.line,
                        extract_context(&step.text, "min")
                    ),
                });
            }
        }
        if step.text.contains("sec")
            && let Some(pos) = step.text.find("sec")
        {
            let before_char = pos
                .checked_sub(1)
                .and_then(|p| step.text[p..].chars().next());
            if before_char.is_some_and(|c| c.is_ascii_digit()) {
                let before = extract_context(&step.text, "sec");
                warnings.push(WorkoutWarning {
                    category: "duration_format",
                    message: format!(
                        "Step line {}: use \"s\" instead of \"sec\". Found: \"{}\"",
                        step.line, before
                    ),
                });
            }
        }
        if step.text.contains("minutes") {
            warnings.push(WorkoutWarning {
                category: "duration_format",
                message: format!("Step line {}: use \"m\" instead of \"minutes\".", step.line),
            });
        }
        if step.text.contains("seconds") {
            warnings.push(WorkoutWarning {
                category: "duration_format",
                message: format!("Step line {}: use \"s\" instead of \"seconds\".", step.line),
            });
        }

        // 2. Check for missing % on ramp boundaries
        if step.text.contains("ramp") {
            // "ramp 50-75" (missing %) vs "ramp 50%-75%" (correct)
            // Look for patterns like "ramp NNN-NNN" without % before the dash
            if let Some(ramp_part) = step.text.split("ramp").nth(1) {
                let cleaned = ramp_part.trim();
                // If there's a hyphen without % signs, flag it
                // Only warn when NEITHER side has "%" — a single % is valid
                // (e.g. "ramp 60-80% Pace" is official syntax)
                if cleaned.contains('-') {
                    let has_pct_before =
                        cleaned.chars().take_while(|&c| c != '-').any(|c| c == '%');
                    let has_pct_after = cleaned.chars().skip_while(|&c| c != '-').any(|c| c == '%');
                    if !has_pct_before && !has_pct_after {
                        warnings.push(WorkoutWarning {
                            category: "ramp_format",
                            message: format!(
                                "Step line {}: ramp boundaries should use \"%\" (e.g. \"ramp 50%-75%\" not \"ramp 50-75\").",
                                step.line
                            ),
                        });
                    }
                }
            }
        }

        // 4. Check for 400m ambiguity: number >= 100 + 'm' suffix looks like meters
        let step_bytes = step.text.as_bytes();
        let mut scan_i = 0;
        while scan_i < step_bytes.len() {
            if step_bytes[scan_i].is_ascii_digit() {
                let num_start = scan_i;
                while scan_i < step_bytes.len() && step_bytes[scan_i].is_ascii_digit() {
                    scan_i += 1;
                }
                if let Ok(num) = step.text[num_start..scan_i].parse::<u32>()
                    && num >= 100
                    && scan_i < step_bytes.len()
                    && step_bytes[scan_i] == b'm'
                {
                    let after_m = scan_i + 1;
                    if after_m >= step_bytes.len() || !step_bytes[after_m].is_ascii_alphabetic() {
                        warnings.push(WorkoutWarning {
                            category: "duration_ambiguity",
                            message: format!(
                                "Step line {}: \"{}m\" looks like meters. Use \"mtr\" for distance or verify this is meant as minutes.",
                                step.line, num
                            ),
                        });
                    }
                }
            } else {
                scan_i += 1;
            }
        }

        // 5. Check for missing primary target
        if !has_primary_target(&step.text) {
            warnings.push(WorkoutWarning {
                category: "missing_target",
                message: format!(
                    "Step line {}: no primary target found (power %, HR, Pace, RPE, freeride, or zone). Add a target like \"75%\", \"Z2 HR\", or \"Pace\".",
                    step.line
                ),
            });
        }
    }

    // 3. Check for missing `%` on percentage targets (e.g. "95-105" instead of "95-105%")
    // Parse the step line looking for number-hyphen-number patterns without trailing %
    for step in &steps {
        let text = &step.text;
        // Check for patterns like "95-105" where this looks like a % range
        let mut prev_end = 0;
        while let Some(hyphen_pos) = text[prev_end..].find('-') {
            let abs_hyphen = prev_end + hyphen_pos;
            // Look backwards for a number
            let before = &text[..abs_hyphen];
            let before_num_end = before.trim_end().len();
            let before_num_start = before[..before_num_end]
                .rfind(|c: char| !c.is_ascii_digit() && c != '.')
                .map(|p| p + 1)
                .unwrap_or(0);
            let num_before: &str = before[before_num_start..before_num_end].trim();

            // Look forwards for a number
            let after = &text[abs_hyphen + 1..];
            let after_num_len = after
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .unwrap_or(after.len());
            let num_after = &after[..after_num_len];

            // Check if both sides look like numbers
            let looks_like_range = !num_before.is_empty()
                && !num_after.is_empty()
                && num_before.chars().all(|c| c.is_ascii_digit() || c == '.')
                && num_after.chars().all(|c| c.is_ascii_digit() || c == '.');

            if looks_like_range {
                // Check if there's a % after the second number
                let after_range = after[after_num_len..].trim();
                if !after_range.starts_with('%')
                    && !after_range.starts_with("HR")
                    && !after_range.starts_with("Pace")
                    && !after_range.starts_with("rpm")
                    && !after_range.starts_with("w")
                    && !after_range.starts_with("bpm")
                {
                    warnings.push(WorkoutWarning {
                        category: "target_format",
                        message: format!(
                            "Step line {}: range \"{}-{}\" may be missing \"%\" (e.g. \"95-105%\").",
                            step.line, num_before, num_after
                        ),
                    });
                    break;
                }
            }

            prev_end = abs_hyphen + 1;
            if prev_end >= text.len() {
                break;
            }
        }
    }

    warnings
}

/// Run full validation + duration sum in one pass.
///
/// This is the main entry point. It parses steps, sums durations, and
/// runs syntax checks, returning everything in a single struct.
pub fn validate_workout_description(
    description: &str,
    expected_duration_seconds: Option<u32>,
) -> WorkoutValidation {
    let steps = parse_workout_steps(description);
    let total_step_seconds: u32 = steps.iter().map(|s| s.duration_seconds).sum();
    let total_with_repeats = sum_step_durations_with_repeats(description);
    let step_count = steps.len();
    let mut warnings = validate_workout_syntax(description);

    // If both step durations and expected_duration are present, compare them
    // Uses repeat-aware sum (accounts for section repeats like "3x") to avoid false positives
    if let Some(expected) = expected_duration_seconds
        && total_with_repeats > 0
    {
        // Allow a small tolerance (30s) for rounding differences
        let diff = total_with_repeats.abs_diff(expected);
        if diff > 30 {
            let display_sum = if total_with_repeats != total_step_seconds {
                total_with_repeats
            } else {
                total_step_seconds
            };
            warnings.push(WorkoutWarning {
                category: "duration_mismatch",
                message: format!(
                    "Step durations sum to {} ({}:{:02}), but new_duration is {} ({}:{:02}). \
                     Intervals.icu may recalculate event duration from steps, overriding new_duration.",
                    display_sum,
                    display_sum / 60,
                    display_sum % 60,
                    expected,
                    expected / 60,
                    expected % 60,
                ),
            });
        }
    }

    WorkoutValidation {
        total_step_seconds,
        step_count,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a short context window around `needle` for readable warnings.
fn extract_context(text: &str, needle: &str) -> String {
    if let Some(pos) = text.find(needle) {
        let start = pos.saturating_sub(10);
        let end = (pos + needle.len() + 10).min(text.len());
        let snippet = &text[start..end];
        if start > 0 && end < text.len() {
            format!("...{}...", snippet)
        } else {
            snippet.to_string()
        }
    } else {
        text.to_string()
    }
}

/// Check whether a step line contains a primary target indicator.
///
/// Returns `true` if the text mentions power (`%` or watts `w`),
/// heart rate (`HR`/`hr`), pace (`Pace`/`pace`), RPE (`RPE`/`rpe`),
/// freeride, zones (`Z1`-`Z9`, `CZ1`-`CZ9`), or `bpm`.
fn has_primary_target(text: &str) -> bool {
    let lower = text.to_lowercase();

    if lower.contains('%')
        || lower.contains(" hr")
        || lower.contains("hr ")
        || lower.contains("pace")
        || lower.contains(" rpe")
        || lower.contains("rpe ")
        || lower.contains("freeride")
        || lower.contains("bpm")
    {
        return true;
    }

    // Check for zone indicators: Z1-Z9 or CZ1-CZ9
    let bytes = lower.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'z' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
            return true;
        }
        if i + 2 < len && bytes[i] == b'c' && bytes[i + 1] == b'z' && bytes[i + 2].is_ascii_digit()
        {
            return true;
        }
        // Check for power in watts: digit followed by 'w' (e.g. "250w")
        if bytes[i] == b'w' && i > 0 && bytes[i - 1].is_ascii_digit() {
            return true;
        }
        i += 1;
    }

    false
}

/// Format seconds as `MM:SS`.
pub fn format_duration_short(secs: u32) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}

/// Format seconds as `H:MM:SS`.
pub fn format_duration_long(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_step_duration ---

    #[test]
    fn parse_seconds_only() {
        assert_eq!(parse_step_duration("- 30s 75%"), Some(30));
        assert_eq!(parse_step_duration("- 90s"), Some(90));
    }

    #[test]
    fn parse_minutes_only() {
        assert_eq!(parse_step_duration("- 10m 60%"), Some(600));
        assert_eq!(parse_step_duration("- 5m"), Some(300));
    }

    #[test]
    fn parse_hours_only() {
        assert_eq!(parse_step_duration("- 1h Z2"), Some(3600));
        assert_eq!(parse_step_duration("- 2h"), Some(7200));
    }

    #[test]
    fn parse_hours_and_minutes() {
        assert_eq!(parse_step_duration("- 1h30m 75%"), Some(5400));
        assert_eq!(parse_step_duration("- 2h15m"), Some(8100));
    }

    #[test]
    fn parse_full_hms() {
        assert_eq!(parse_step_duration("- 1h30m59s"), Some(5459));
    }

    #[test]
    fn parse_short_form_seconds() {
        assert_eq!(parse_step_duration("- 1m30 120%"), Some(90));
        assert_eq!(parse_step_duration("- 3m45"), Some(225));
    }

    #[test]
    fn parse_mixed_form() {
        assert_eq!(parse_step_duration("- 5m30s Z3"), Some(330));
    }

    #[test]
    fn parse_strides() {
        // "15s" is the duration, the parenthetical is just text
        assert_eq!(
            parse_step_duration("- 15s Z4 Pace (relaxed fast, not sprint)"),
            Some(15)
        );
    }

    #[test]
    fn parse_no_duration_returns_none() {
        assert_eq!(parse_step_duration("- REST"), None);
        assert_eq!(parse_step_duration("Warmup"), None);
        assert_eq!(parse_step_duration(""), None);
        assert_eq!(parse_step_duration("- freeride 20m"), Some(1200)); // "20m" is still parsed
    }

    // --- sum_step_durations ---

    #[test]
    fn sum_simple_workout() {
        let desc = "\
Warmup
- 10m 60%

Main Set 3x
- 5m 95%
- 3m Z1

Cooldown
- 10m 50%";
        assert_eq!(sum_step_durations(desc), 1680); // 600 + 300 + 180 + 600
    }

    #[test]
    fn sum_flat_single_step() {
        let desc = "- 1h45m Z2 HR";
        assert_eq!(sum_step_durations(desc), 6300);
    }

    #[test]
    fn sum_empty_description() {
        assert_eq!(sum_step_durations(""), 0);
        assert_eq!(sum_step_durations("REST\nCooldown"), 0);
    }

    // --- validate_workout_syntax ---

    #[test]
    fn warns_on_min_usage() {
        let desc = "- 10min 60%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("min")),
            "expected duration_format warning for '10min', got: {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_minimum_in_text_prompt() {
        // "minimum" contains "min" but not as a duration unit
        let desc = "- 10m 60% (minimum cadence 70rpm)";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("min")),
            "unexpected 'min' warning for 'minimum': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_minute_word_in_text_prompt() {
        // "minute" as text, not as duration
        let desc = "- 5m 60% (one minute rest between reps)";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("min")),
            "unexpected 'min' warning for 'minute' text: {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_min_usage_combined() {
        // "5min30s" — mixed min + s
        let desc = "- 5min30s 75%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("min")),
            "expected duration_format warning for '5min30s', got: {:?}",
            warnings
        );
    }

    /// Verify "min" in the middle of a word like "admin" does not trigger.
    /// (Unlikely in workout steps, but proves the check is correct.)
    #[test]
    fn no_warn_on_min_as_substring_of_non_duration() {
        let desc = "- 10m Z2 (admin note)";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("min")),
            "unexpected 'min' warning for non-duration 'admin': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_minutes_string() {
        let desc = "Warmup\n- 10minutes 60%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("minutes"))
        );
    }

    #[test]
    fn warns_on_missing_ramp_percent() {
        let desc = "- 10m ramp 50-75";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.iter().any(|w| w.category == "ramp_format"),
            "expected ramp_format warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_ramp_with_second_pct_and_target() {
        // "ramp 60-80% Pace" — only second number has %, this is valid
        let desc = "- 10m ramp 60-80% Pace";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "ramp_format"),
            "unexpected ramp_format warning for 'ramp 60-80% Pace': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_ramp_with_first_pct_only() {
        // "ramp 50%-75" — only first number has %, also valid
        let desc = "- 10m ramp 50%-75";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "ramp_format"),
            "unexpected ramp_format warning for 'ramp 50%-75': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_ramp_with_second_pct_only() {
        // "ramp 60-80%" — only second number has %, valid
        let desc = "- 10m ramp 60-80%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "ramp_format"),
            "unexpected ramp_format warning for 'ramp 60-80%': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_ramp_with_pct_and_rpm() {
        // "ramp 60%-80% 90rpm" — both have %, with cadence
        let desc = "- 15m ramp 60%-90% 85rpm";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "ramp_format"),
            "unexpected ramp_format warning for valid ramp with cadence: {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_correct_ramp() {
        let desc = "- 10m ramp 50%-75%";
        let warnings = validate_workout_syntax(desc);
        assert!(!warnings.iter().any(|w| w.category == "ramp_format"));
    }

    #[test]
    fn warns_on_range_without_percent() {
        // "95-105" without % at the end should trigger a warning
        let desc = "- 5m 95-105";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "target_format" && w.message.contains("95"))
        );
    }

    #[test]
    fn no_warn_on_range_with_percent() {
        let desc = "- 5m 95-105%";
        let warnings = validate_workout_syntax(desc);
        assert!(!warnings.iter().any(|w| w.category == "target_format"));
    }

    #[test]
    fn no_warn_on_clean_workout() {
        let desc = "\
Warmup
- 15m Z1 HR

Main Set 3x
- 6m Z3 HR
- 3m Z1 HR

Cooldown
- 13m Z1 HR";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.is_empty(),
            "expected no warnings on clean workout, got: {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_workout_with_zones_ramp_and_ranges() {
        // Covers all potentially problematic patterns that should NOT warn
        let desc = "\
Warmup
- 15m Z2 HR

Main Set
- 10m ramp 60-80% Pace
- 8m Z3-Z4 HR
- 6m CZ2-CZ3
- 5m 95-105%
- 10m 75% 90rpm

Cooldown
- 10m Z1 HR";
        let warnings = validate_workout_syntax(desc);
        let ramp_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.category == "ramp_format")
            .collect();
        let target_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.category == "target_format")
            .collect();
        assert!(
            ramp_warnings.is_empty(),
            "unexpected ramp_format warnings: {:?}",
            ramp_warnings
        );
        assert!(
            target_warnings.is_empty(),
            "unexpected target_format warnings: {:?}",
            target_warnings
        );
        let ambiguity_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.category == "duration_ambiguity")
            .collect();
        let missing_target_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.category == "missing_target")
            .collect();
        assert!(
            ambiguity_warnings.is_empty(),
            "unexpected duration_ambiguity warnings: {:?}",
            ambiguity_warnings
        );
        assert!(
            missing_target_warnings.is_empty(),
            "unexpected missing_target warnings: {:?}",
            missing_target_warnings
        );
    }

    // --- 400m ambiguity ---

    #[test]
    fn warns_on_400m_ambiguity() {
        let desc = "- 400m Z3";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "duration_ambiguity" && w.message.contains("400m")),
            "expected duration_ambiguity warning for '400m', got: {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_mtr_suffix() {
        // "mtr" is a valid distance unit — 'm' followed by 't' (alphabetic) should not trigger
        let desc = "- 200mtr Z3";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "duration_ambiguity"),
            "unexpected duration_ambiguity warning for '200mtr': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_min_suffix() {
        // "min" — 'm' followed by 'i' (alphabetic) should not trigger
        let desc = "- 100min Z3";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "duration_ambiguity"),
            "unexpected duration_ambiguity warning for '100min': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_small_number_m() {
        // "15m" — 15 < 100, no ambiguity
        let desc = "- 15m Z2";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "duration_ambiguity"),
            "unexpected duration_ambiguity warning for '15m': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_300m_without_target() {
        // 300m alone — ambiguous and also missing target
        let desc = "- 300m";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "duration_ambiguity" && w.message.contains("300m")),
            "expected duration_ambiguity warning for '300m': {:?}",
            warnings
        );
    }

    // --- missing primary target ---

    #[test]
    fn warns_on_missing_target_no_target() {
        let desc = "- 10m";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.iter().any(|w| w.category == "missing_target"),
            "expected missing_target warning for bare '10m': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_percent_target() {
        let desc = "- 10m 75%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '10m 75%': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_hr_target() {
        let desc = "- 10m Z2 HR";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '10m Z2 HR': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_zone_only_target() {
        let desc = "- 10m Z2";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '10m Z2': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_cz_target() {
        let desc = "- 10m CZ2-CZ3";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '10m CZ2-CZ3': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_freeride() {
        let desc = "- freeride 20m";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for 'freeride 20m': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_rpm_only_not_primary_target() {
        // rpm (cadence) is a secondary target — not sufficient as primary
        let desc = "- 10m 90rpm";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.iter().any(|w| w.category == "missing_target"),
            "expected missing_target warning for '10m 90rpm': {:?}",
            warnings
        );
    }

    // --- sum_step_durations_with_repeats ---

    #[test]
    fn sum_with_repeats_accounts_for_section_multipliers() {
        let desc = "\
Warmup
- 10m 50%

Main Set 3x
- 5m 95%
- 3m Z1

Cooldown
- 10m 50%";
        // Raw sum: 600 + 300 + 180 + 600 = 1680
        // With repeats: 600 + 3*(300+180) + 600 = 600 + 1440 + 600 = 2640
        assert_eq!(sum_step_durations(desc), 1680);
        assert_eq!(sum_step_durations_with_repeats(desc), 2640);
    }

    #[test]
    fn sum_with_repeats_no_headers() {
        let desc = "- 10m Z2\n- 5m Z3";
        assert_eq!(sum_step_durations_with_repeats(desc), 900);
    }

    #[test]
    fn sum_with_repeats_empty() {
        assert_eq!(sum_step_durations_with_repeats(""), 0);
        assert_eq!(sum_step_durations_with_repeats("REST"), 0);
    }

    #[test]
    fn sum_with_repeats_repeat_on_own_line() {
        let desc = "\
3x
- 2m 100%
- 1m 50%

1x
- 5m Z1";
        // 3*(120+60) + 1*300 = 540 + 300 = 840
        assert_eq!(sum_step_durations_with_repeats(desc), 840);
    }

    // --- validate_workout_description with repeats ---

    #[test]
    fn no_false_positive_duration_mismatch_with_repeats() {
        // Main Set 3x makes total 44m (2640s), not 28m (1680s)
        let desc = "\
Warmup
- 10m 50%

Main Set 3x
- 5m 95%
- 3m Z1

Cooldown
- 10m 50%";
        let result = validate_workout_description(desc, Some(2640));
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch"),
            "unexpected duration_mismatch warning with repeats accounted: {:?}",
            result.warnings
        );
    }

    // --- validate_workout_description (full) ---

    #[test]
    fn detects_duration_mismatch() {
        let desc = "\
Warmup
- 15m Z1 HR

Main Set
- 15m Z2 HR

Cooldown
- 15m Z1 HR";
        // Steps sum to 45m = 2700s, expected is 1:45 = 6300s
        let result = validate_workout_description(desc, Some(6300));
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch"),
            "expected duration_mismatch warning, got: {:?}",
            result.warnings
        );
        assert_eq!(result.total_step_seconds, 2700);
        assert_eq!(result.step_count, 3);
    }

    #[test]
    fn no_false_positive_duration_match() {
        let desc = "- 1h45m Z2 HR";
        let result = validate_workout_description(desc, Some(6300));
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch")
        );
    }

    #[test]
    fn no_duration_check_when_no_expected() {
        let desc = "- 10m 60%";
        let result = validate_workout_description(desc, None);
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch")
        );
    }

    // --- parse_step_duration edge cases ---

    #[test]
    fn parse_bare_number_no_unit_returns_none() {
        assert_eq!(parse_step_duration("- 120"), None);
        assert_eq!(parse_step_duration("- 90"), None);
    }

    #[test]
    fn parse_quote_notation() {
        assert_eq!(parse_step_duration("- 5'30\" Z2"), Some(330));
        assert_eq!(parse_step_duration("- 3'45\" 75%"), Some(225));
    }

    // --- duration_format edge cases ---

    #[test]
    fn warns_on_seconds_text() {
        let desc = "- 30seconds 75%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| { w.category == "duration_format" && w.message.contains("seconds") }),
            "expected duration_format warning for '30seconds': {:?}",
            warnings
        );
    }

    #[test]
    fn no_false_positive_sec_in_consecutive() {
        let desc = "- 10m Z2 (consecutive efforts)";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings
                .iter()
                .any(|w| w.category == "duration_format" && w.message.contains("sec")),
            "unexpected 'sec' warning for 'consecutive': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_sec_when_preceded_by_digit() {
        let desc = "- 30sec 75%";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| { w.category == "duration_format" && w.message.contains("sec") }),
            "expected duration_format warning for '30sec': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_multiple_duration_format_same_step() {
        let desc = "- 10min30sec 75%";
        let warnings = validate_workout_syntax(desc);
        let min_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.category == "duration_format" && w.message.contains("min"))
            .collect();
        let sec_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.category == "duration_format" && w.message.contains("sec"))
            .collect();
        assert!(
            !min_warnings.is_empty(),
            "expected min warning in '10min30sec': {:?}",
            warnings
        );
        assert!(
            !sec_warnings.is_empty(),
            "expected sec warning in '10min30sec': {:?}",
            warnings
        );
    }

    // --- ramp_format edge cases ---

    #[test]
    fn warns_on_ramp_with_cadence_only_no_pct() {
        // "ramp 50-75 90rpm" — has cadence but no % boundaries
        let desc = "- 10m ramp 50-75 90rpm";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.iter().any(|w| w.category == "ramp_format"),
            "expected ramp_format warning for 'ramp 50-75 90rpm': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_ramp_single_boundary() {
        // "ramp 50-" — hyphen with only one side
        let desc = "- 10m ramp 50-";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.iter().any(|w| w.category == "ramp_format"),
            "expected ramp_format warning for 'ramp 50-': {:?}",
            warnings
        );
    }

    // --- target_format edge cases ---

    #[test]
    fn no_warn_on_range_with_w_suffix() {
        // "250-350w" — w suffix means watts, valid target
        let desc = "- 5m 250-350w";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "target_format"),
            "unexpected target_format warning for '250-350w': {:?}",
            warnings
        );
    }

    // --- duration_ambiguity edge cases ---

    #[test]
    fn warns_on_100m_boundary() {
        // 100m — 100 >= threshold, ambiguous
        let desc = "- 100m Z2";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| { w.category == "duration_ambiguity" && w.message.contains("100m") }),
            "expected duration_ambiguity warning for '100m': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_100mi_miles() {
        // "100mi" — 'm' followed by 'i' (alphabetic), safe
        let desc = "- 100mi";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "duration_ambiguity"),
            "unexpected duration_ambiguity warning for '100mi': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_400s_not_meters() {
        // "400s" — unit is 's', not 'm', no ambiguity
        let desc = "- 400s";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "duration_ambiguity"),
            "unexpected duration_ambiguity warning for '400s': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_500m_with_punctuation() {
        // "500m," — comma after 'm' is not alphabetic, should warn
        let desc = "- 500m, Z2";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings
                .iter()
                .any(|w| w.category == "duration_ambiguity" && w.message.contains("500m")),
            "expected duration_ambiguity warning for '500m,': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_1000mtr_safe() {
        // "1000mtr" — 'm' followed by 't' (alphabetic), safe
        let desc = "- 1000mtr Z2";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "duration_ambiguity"),
            "unexpected duration_ambiguity warning for '1000mtr': {:?}",
            warnings
        );
    }

    // --- missing_target edge cases ---

    #[test]
    fn no_warn_on_bpm_target() {
        let desc = "- 10m 150bpm";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '150bpm': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_watts_target() {
        // "250w" — power in watts, valid primary target
        let desc = "- 10m 250w";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '250w': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_rpe_capitalized() {
        let desc = "- 10m RPE 7";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for 'RPE 7': {:?}",
            warnings
        );
    }

    #[test]
    fn no_warn_on_pace_with_time() {
        let desc = "- 10m 4:00 Pace";
        let warnings = validate_workout_syntax(desc);
        assert!(
            !warnings.iter().any(|w| w.category == "missing_target"),
            "unexpected missing_target warning for '4:00 Pace': {:?}",
            warnings
        );
    }

    #[test]
    fn warns_on_distance_step_no_target() {
        // 400mtr has duration but no power/HR/pace/RPE/freeride/zone
        let desc = "- 400mtr";
        let warnings = validate_workout_syntax(desc);
        assert!(
            warnings.iter().any(|w| w.category == "missing_target"),
            "expected missing_target warning for bare '400mtr': {:?}",
            warnings
        );
    }

    // --- sum_step_durations_with_repeats edge cases ---

    #[test]
    fn sum_with_repeats_zero_repeat() {
        let desc = "\
0x
- 10m Z2";
        assert_eq!(sum_step_durations_with_repeats(desc), 0);
    }

    #[test]
    fn sum_with_repeats_large_repeat() {
        let desc = "\
Warmup
- 1m Z1

Main Set 99x
- 2m 100%

Cooldown
- 1m Z1";
        // 60 + 99*120 + 60 = 60 + 11880 + 60 = 12000
        assert_eq!(sum_step_durations_with_repeats(desc), 12000);
    }

    #[test]
    fn sum_with_repeats_empty_section() {
        let desc = "\
Warmup
- 10m 50%

3x

Cooldown
- 10m 50%";
        // 3x section has no steps, so 600 + 0 + 600 = 1200
        assert_eq!(sum_step_durations_with_repeats(desc), 1200);
    }

    #[test]
    fn sum_with_repeats_consecutive_repeat_headers() {
        let desc = "\
3x
5x
- 2m Z2";
        // Last repeat wins: 5*120 = 600
        assert_eq!(sum_step_durations_with_repeats(desc), 600);
    }

    // --- validate_workout_description edge cases ---

    #[test]
    fn detects_duration_mismatch_just_over_tolerance() {
        // Diff of 31s (one second over 30s tolerance)
        let desc = "- 10m Z2";
        let result = validate_workout_description(desc, Some(631));
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch"),
            "expected duration_mismatch for 31s diff: {:?}",
            result.warnings
        );
    }

    #[test]
    fn no_false_positive_duration_tolerance_boundary() {
        // Diff of exactly 30s — within tolerance
        let desc = "- 10m Z2";
        let result = validate_workout_description(desc, Some(630));
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch"),
            "unexpected duration_mismatch for 30s diff (tolerance): {:?}",
            result.warnings
        );
    }

    #[test]
    fn detects_mismatch_with_repeat_when_expected_too_low() {
        // With repeats the total is 2640s (44m), but expected is 2480s (41m20s)
        // diff = 160s > 30, should warn
        let desc = "\
Warmup
- 10m 50%

Main Set 3x
- 5m 95%

Cooldown
- 10m 50%";
        let result = validate_workout_description(desc, Some(2480));
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.category == "duration_mismatch"),
            "expected duration_mismatch when repeats push total past expected: {:?}",
            result.warnings
        );
    }

    // --- helpers ---

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration_short(90), "1:30");
        assert_eq!(format_duration_short(3600), "60:00");
    }

    #[test]
    fn test_format_duration_long() {
        assert_eq!(format_duration_long(3661), "1:01:01");
        assert_eq!(format_duration_long(6300), "1:45:00");
    }
}
