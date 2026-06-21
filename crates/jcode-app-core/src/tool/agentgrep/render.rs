use super::*;

pub(super) fn render_smart_output(result: &SmartResult, _args: &SmartArgs) -> String {
    let mut lines = vec![
        format!("ffs trace: {}", result.query.subject),
        format!("relation: {}", result.query.relation.as_str()),
        format!(
            "files: {}, regions: {}",
            result.summary.total_files, result.summary.total_regions
        ),
        String::new(),
    ];

    for file in &result.files {
        lines.push(format!(
            "📄 {}  [{}] (score: {})",
            file.path, file.role, file.score
        ));
        for reason in &file.why {
            lines.push(format!("   why: {reason}"));
        }
        for region in &file.regions {
            lines.push(format!(
                "   └─ {} @ L{}-L{}",
                region.label, region.start_line, region.end_line
            ));
            for line in region.body.lines().take(5) {
                lines.push(format!("       {line}"));
            }
            if region.body.lines().count() > 5 {
                lines.push("       ...".to_string());
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}
