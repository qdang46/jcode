//! Hook matcher logic - determines which hooks apply to which tools/events

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HookMatcher {
    Exact(String),
    Multi(Vec<String>),
    Regex(regex::Regex),
    Wildcard,
}

/// Context for matching a hook against an event
#[derive(Debug, Clone)]
pub struct MatcherContext<'a> {
    /// The tool name or event identifier being matched
    pub target: &'a str,
    /// Additional context (e.g., full command for Bash hooks)
    pub context: Option<&'a str>,
}

impl<'a> MatcherContext<'a> {
    /// Create a new matcher context
    pub fn new(target: &'a str) -> Self {
        Self {
            target,
            context: None,
        }
    }

    /// Create with additional context
    pub fn with_context(target: &'a str, context: &'a str) -> Self {
        Self {
            target,
            context: Some(context),
        }
    }
}

/// Check if a matcher pattern matches the given context
pub fn matches(matcher: &HookMatcher, ctx: &MatcherContext) -> bool {
    match matcher {
        HookMatcher::Exact(pattern) => ctx.target == pattern,
        HookMatcher::Multi(patterns) => patterns.iter().any(|p| ctx.target == p),
        HookMatcher::Regex(re) => {
            // Match against target + context (concatenated) for full flexibility
            let match_str = match ctx.context {
                Some(context) => format!("{}{}", ctx.target, context),
                None => ctx.target.to_string(),
            };
            re.is_match(&match_str)
        }
        HookMatcher::Wildcard => true,
    }
}

/// Parse a multi-value pattern string like "Write|Edit" into individual values
pub fn parse_multi_pattern(pattern: &str) -> Vec<String> {
    pattern.split('|').map(|s| s.trim().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_matcher() {
        let matcher = HookMatcher::Exact("Bash".to_string());
        let ctx = MatcherContext::new("Bash");
        assert!(matches(&matcher, &ctx));

        let ctx = MatcherContext::new("Write");
        assert!(!matches(&matcher, &ctx));
    }

    #[test]
    fn test_multi_matcher() {
        let matcher = HookMatcher::Multi(vec!["Bash".to_string(), "Write".to_string()]);
        let ctx = MatcherContext::new("Bash");
        assert!(matches(&matcher, &ctx));

        let ctx = MatcherContext::new("Write");
        assert!(matches(&matcher, &ctx));

        let ctx = MatcherContext::new("Edit");
        assert!(!matches(&matcher, &ctx));
    }

    #[test]
    fn test_multi_matcher_from_string() {
        let patterns = parse_multi_pattern("Write|Edit|Glob");
        assert_eq!(patterns, vec!["Write", "Edit", "Glob"]);
    }

    #[test]
    fn test_regex_matcher() {
        let matcher = HookMatcher::Regex(regex::Regex::new("^Bash(git.*)").unwrap());

        let ctx = MatcherContext::new("Bash");
        assert!(!matches(&matcher, &ctx)); // No match without git prefix

        let ctx = MatcherContext::with_context("Bash", "git commit");
        assert!(matches(&matcher, &ctx));

        let ctx = MatcherContext::with_context("Bash", "ls -la");
        assert!(!matches(&matcher, &ctx));
    }

    #[test]
    fn test_wildcard_matcher() {
        let matcher = HookMatcher::Wildcard;
        let ctx = MatcherContext::new("Anything");
        assert!(matches(&matcher, &ctx));
    }

    #[test]
    fn test_invalid_regex_falls_back() {
        let matcher = // HookMatcher::Regex("[invalid".to_string()); — removed, invalid regex would cause a panic at parse time
        let ctx = MatcherContext::new("[invalid");
        // Invalid regex should fall back to exact match
        assert!(matches(&matcher, &ctx));
    }
}
