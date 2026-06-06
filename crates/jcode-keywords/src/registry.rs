//! Keyword registry — all supported keywords, aliases, priorities, and workflow mappings.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

/// Workflow kinds that can be triggered by keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter, EnumString, Serialize, Deserialize)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowKind {
    /// ParallelExecution — spawn sub-agents, coordinate, aggregate
    Ultrawork,
    /// GoalTracking — durable goal + token budget across turns
    Ultragoal,
    /// QACycling — implement → test → fix → repeat
    Ultraqa,
    /// ConsensusPlanning — plan → adversarial review → revise → approve
    Ralplan,
    /// RequirementsGathering — ask questions → score ambiguity → threshold
    DeepInterview,
    /// TestDrivenDev — write test → fail → implement → pass
    Tdd,
    /// CodeReview — spawn reviewer → analyze → report
    CodeReview,
    /// SecurityReview — OWASP scan → secrets → report
    SecurityReview,
    /// ExtendedThinking — deep reasoning, single-turn
    Ultrathink,
    /// CodebaseSearch — multi-strategy search → context map
    Deepsearch,
    /// DeepAnalysis — structured analysis → report
    Analyze,
    /// DocLookup — local + web docs → summary
    Wiki,
    /// SlopCleanup — detect + fix AI low-quality code
    AiSlopCleaner,
    /// CancelAll — stop all modes + cancel tasks
    Cancel,
}

/// A single keyword entry in the registry.
#[derive(Debug, Clone)]
pub struct KeywordEntry {
    /// The canonical keyword trigger (e.g. "$ultrawork")
    pub keyword: &'static str,
    /// Alternative triggers (natural language aliases)
    pub aliases: &'static [&'static str],
    /// Priority: 11 (highest) .. 5 (lowest)
    pub priority: u8,
    /// The workflow this keyword activates
    pub workflow: WorkflowKind,
    /// Human-readable description
    pub description: &'static str,
}

/// Build the full keyword registry, sorted by priority (highest first).
pub fn build_registry() -> Vec<KeywordEntry> {
    let mut entries = vec![
        // Priority 11 — highest
        KeywordEntry {
            keyword: "$ralplan",
            aliases: &["ralplan", "consensus plan"],
            priority: 11,
            workflow: WorkflowKind::Ralplan,
            description: "Consensus planning — plan → adversarial review → revise → approve",
        },
        // Priority 10
        KeywordEntry {
            keyword: "$ultrawork",
            aliases: &["ulw", "uw", "parallel", "dont stop", "must complete"],
            priority: 10,
            workflow: WorkflowKind::Ultrawork,
            description: "Parallel execution — spawn sub-agents, coordinate, aggregate",
        },
        KeywordEntry {
            keyword: "$ultragoal",
            aliases: &["ultragoal"],
            priority: 10,
            workflow: WorkflowKind::Ultragoal,
            description: "Goal tracking — durable goal + token budget across turns",
        },
        // Priority 9
        KeywordEntry {
            keyword: "canceljcode",
            aliases: &["stopjcode"],
            priority: 9,
            workflow: WorkflowKind::Cancel,
            description: "Cancel all active modes and stop running tasks",
        },
        // Priority 8
        KeywordEntry {
            keyword: "$ultraqa",
            aliases: &["ultraqa", "qa cycle"],
            priority: 8,
            workflow: WorkflowKind::Ultraqa,
            description: "QA cycling — implement → test → fix → repeat",
        },
        KeywordEntry {
            keyword: "$deep-interview",
            aliases: &["ouroboros", "interview me", "gather requirements"],
            priority: 8,
            workflow: WorkflowKind::DeepInterview,
            description: "Requirements gathering — ask questions → score ambiguity → threshold",
        },
        // Priority 7
        KeywordEntry {
            keyword: "$ultrathink",
            aliases: &["think hard", "think deeply"],
            priority: 7,
            workflow: WorkflowKind::Ultrathink,
            description: "Extended thinking — deep reasoning, single-turn",
        },
        KeywordEntry {
            keyword: "$deepsearch",
            aliases: &["search the codebase", "find in codebase"],
            priority: 7,
            workflow: WorkflowKind::Deepsearch,
            description: "Codebase search — multi-strategy search → context map",
        },
        KeywordEntry {
            keyword: "$tdd",
            aliases: &["test first", "red green"],
            priority: 7,
            workflow: WorkflowKind::Tdd,
            description: "Test-driven development — write test → fail → implement → pass",
        },
        // Priority 6
        KeywordEntry {
            keyword: "$code-review",
            aliases: &["code review", "review code"],
            priority: 6,
            workflow: WorkflowKind::CodeReview,
            description: "Code review — spawn reviewer → analyze → report",
        },
        KeywordEntry {
            keyword: "$security-review",
            aliases: &["security review", "audit security"],
            priority: 6,
            workflow: WorkflowKind::SecurityReview,
            description: "Security review — OWASP scan → secrets → report",
        },
        KeywordEntry {
            keyword: "$analyze",
            aliases: &["deep-analyze", "deep analysis"],
            priority: 6,
            workflow: WorkflowKind::Analyze,
            description: "Deep analysis — structured analysis → report",
        },
        // Priority 5
        KeywordEntry {
            keyword: "$wiki",
            aliases: &["wiki this", "look up docs"],
            priority: 5,
            workflow: WorkflowKind::Wiki,
            description: "Doc lookup — local + web docs → summary",
        },
        KeywordEntry {
            keyword: "ai-slop-cleaner",
            aliases: &["clean ai slop", "fix ai code"],
            priority: 5,
            workflow: WorkflowKind::AiSlopCleaner,
            description: "AI slop cleanup — detect + fix AI low-quality code",
        },
    ];

    // Sort by priority (highest first)
    entries.sort_by(|a, b| b.priority.cmp(&a.priority));
    entries
}

/// Multilingual triggers for search-related keywords.
/// 64 triggers across EN/KO/JA/ZH/VI.
pub fn search_triggers() -> &'static [&'static str] {
    &[
        // English
        "search", "find", "look for", "locate", "grep", "scan for",
        "where is", "search for", "find in codebase", "search the codebase",
        "look up", "hunt for", "dig for",
        // Korean
        "검색", "찾아", "찾기", "검색해", "어디있어", "코드에서 찾아",
        // Japanese
        "検索", "探して", "見つけて", "コードを探す", "どこにある",
        // Chinese
        "搜索", "查找", "找一下", "在代码中查找", "在哪里",
        // Vietnamese
        "tìm kiếm", "tìm", "tìm trong code", "ở đâu", "tìm code",
        // More English variants
        "explore", "investigate", "trace", "lookup", "query",
        "seek out", "fish for", "root out", "comb through",
        // More multilingual
        "ファイル検索", "ファイルを探", "コード検索",
        "파일 검색", "코드 검색",
        "文件搜索", "代码搜索",
        "tìm file", "tìm trong file",
    ]
}

/// Multilingual triggers for analyze-related keywords.
/// 64 triggers across EN/KO/JA/ZH/VI.
pub fn analyze_triggers() -> &'static [&'static str] {
    &[
        // English
        "analyze", "analyse", "deep analysis", "examine", "inspect",
        "investigate", "review deeply", "break down", "dissect", "study",
        "evaluate", "assess", "audit",
        // Korean
        "분석", "심층 분석", "검토", "조사", "평가해",
        "코드 분석", "상세 분석",
        // Japanese
        "分析", "深く分析", "調査", "検証", "評価",
        "コード分析", "詳細分析",
        // Chinese
        "分析", "深度分析", "检查", "审查", "评估",
        "代码分析", "详细分析",
        // Vietnamese
        "phân tích", "phân tích sâu", "kiểm tra", "đánh giá", "xem xét",
        "phân tích code", "phân tích chi tiết",
        // More English variants
        "deep dive", "tear apart", "look into", "probe", "survey",
        "take stock of", "size up", "go through",
        // More multilingual
        "コードを見る", "コードを確認",
        "코드 확인", "코드 리뷰",
        "查看代码", "代码审查",
        "xem code", "kiểm tra code",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_sorted_by_priority() {
        let registry = build_registry();
        for window in registry.windows(2) {
            assert!(
                window[0].priority >= window[1].priority,
                "Registry not sorted: {} ({}) > {} ({})",
                window[0].keyword,
                window[0].priority,
                window[1].keyword,
                window[1].priority,
            );
        }
    }

    #[test]
    fn registry_has_all_workflows() {
        let registry = build_registry();
        let kinds: std::collections::HashSet<WorkflowKind> =
            registry.iter().map(|e| e.workflow).collect();
        // All 14 workflows should be represented
        assert_eq!(kinds.len(), 14);
    }

    #[test]
    fn search_triggers_count() {
        assert!(search_triggers().len() >= 50);
    }

    #[test]
    fn analyze_triggers_count() {
        assert!(analyze_triggers().len() >= 50);
    }
}
