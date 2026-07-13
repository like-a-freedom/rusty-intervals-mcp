use std::collections::HashSet;

use chrono::NaiveDate;
use intervals_icu_client::Event;

use super::shared::parse_event_date;

pub(crate) fn dedupe_and_sort_events(mut events: Vec<Event>) -> Vec<Event> {
    let mut seen = HashSet::new();
    events.retain(|event| {
        let dedupe_key = event.id.clone().unwrap_or_else(|| {
            format!(
                "{}:{}:{:?}",
                event.start_date_local, event.name, event.category
            )
        });
        seen.insert(dedupe_key)
    });

    events.sort_by(|a, b| {
        let a_date = parse_event_date(&a.start_date_local).unwrap_or(NaiveDate::MIN);
        let b_date = parse_event_date(&b.start_date_local).unwrap_or(NaiveDate::MIN);
        a_date
            .cmp(&b_date)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| format!("{:?}", a.category).cmp(&format!("{:?}", b.category)))
            .then_with(|| a.id.cmp(&b.id))
    });

    events
}

#[cfg(test)]
mod tests {
    use intervals_icu_client::EventCategory;

    use super::*;

    #[test]
    fn dedupe_and_sort_events_prefers_unique_calendar_entries() {
        let events = vec![
            Event {
                id: Some("event-1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Race day".to_string(),
                category: EventCategory::RaceA,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("event-2".to_string()),
                start_date_local: "2026-03-02".to_string(),
                name: "Recovery".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("event-3".to_string()),
                start_date_local: "2026-03-03".to_string(),
                name: "Workout".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ];
        let deduped = dedupe_and_sort_events(events);
        assert_eq!(deduped.len(), 3);
    }

    #[test]
    fn dedupe_and_sort_events_empty_list() {
        let events: Vec<Event> = vec![];
        let deduped = dedupe_and_sort_events(events);
        assert!(deduped.is_empty());
    }

    #[test]
    fn dedupe_and_sort_events_removes_duplicates_by_id() {
        let events = vec![
            Event {
                id: Some("dup-1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Race".to_string(),
                category: EventCategory::RaceA,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("dup-1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Race".to_string(),
                category: EventCategory::RaceA,
                description: None,
                r#type: None,
            },
        ];
        let deduped = dedupe_and_sort_events(events);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn dedupe_and_sort_events_sorts_by_date() {
        let events = vec![
            Event {
                id: Some("b".to_string()),
                start_date_local: "2026-03-02".to_string(),
                name: "Second".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("a".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "First".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ];
        let deduped = dedupe_and_sort_events(events);
        assert_eq!(deduped[0].id.as_deref(), Some("a"));
        assert_eq!(deduped[1].id.as_deref(), Some("b"));
    }

    #[test]
    fn dedupe_and_sort_events_without_id_uses_fallback_key() {
        let events = vec![
            Event {
                id: None,
                start_date_local: "2026-03-01".to_string(),
                name: "Same".to_string(),
                category: EventCategory::RaceA,
                description: None,
                r#type: None,
            },
            Event {
                id: None,
                start_date_local: "2026-03-01".to_string(),
                name: "Same".to_string(),
                category: EventCategory::RaceA,
                description: None,
                r#type: None,
            },
        ];
        let deduped = dedupe_and_sort_events(events);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn dedupe_and_sort_events_fallback_key_includes_category() {
        let events = vec![
            Event {
                id: None,
                start_date_local: "2026-03-01".to_string(),
                name: "Same Date Name".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
            Event {
                id: None,
                start_date_local: "2026-03-01".to_string(),
                name: "Same Date Name".to_string(),
                category: EventCategory::Note,
                description: None,
                r#type: None,
            },
        ];
        // Different categories = different fallback keys = both kept
        let deduped = dedupe_and_sort_events(events);
        assert_eq!(deduped.len(), 2);
    }
}
