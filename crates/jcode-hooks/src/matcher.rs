//! Hook matcher logic - determines which hooks apply to which tools/events

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HookMatcher {
    Exact(String),
    Multi(Vec<String>),
    Regex(String),
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
        HookMatcher::Regex(pattern) => {
            // Global regex cache: compile once per unique pattern string
            static REGEX_CACHE: LazyLock<Mutex<HashMap<String, &'static Regex>>> =
                LazyLock::new(|| Mutex::new(HashMap::new()));
            let re = {
                let mut cache = REGEX_CACHE.lock().expect("regex cache poisoned");
                let re = cache.entry(pattern.to_string()).or_insert_with(|| {
                    Box::leak(Box::new(
                        Regex::new(pattern).unwrap_or_else(|e| {
                            eprintln!(
                                "[jcode-hooks] invalid regex pattern {:?}: {} — using never-match placeholder",
                                pattern, e
                            );
                            Regex::new(r"[^\s\S]").expect("never-match placeholder is valid")
                        }),
                    ))
                });
                *re
            };
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
        let matcher = HookMatcher::Regex("^Bash(git.*)".to_string());

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
        // Invalid regex falls back to a never-match placeholder with a warning.
        let matcher = HookMatcher::Regex("never-match".to_string());
        let ctx = MatcherContext::new("anything");
        assert!(!matches(&matcher, &ctx));
    }
}
