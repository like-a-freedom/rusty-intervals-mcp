use crate::content::ContentBlock;

pub(crate) fn comparison_header_block(later_label: &str, earlier_label: &str) -> ContentBlock {
    ContentBlock::markdown(format!(
        "# Comparison: {} vs {}",
        later_label, earlier_label
    ))
}

pub(crate) fn requested_compare_metrics_blocks(
    rows: Vec<Vec<String>>,
    later_label: &str,
    earlier_label: &str,
) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Requested Metrics".to_string()),
        ContentBlock::table(
            vec![
                "Metric".into(),
                later_label.into(),
                earlier_label.into(),
                "Status".into(),
            ],
            rows,
        ),
    ]
}
