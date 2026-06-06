//! Preflight static analysis for plugin code.
//!
//! Runs before a plugin's code is evaluated in the QuickJS sandbox.
//! Detects suspicious patterns, undeclared capabilities, and dangerous
//! constructs. Warnings are logged; blocks prevent loading entirely.

use crate::manifest::PluginCapabilities;

/// Result of preflight static analysis.
#[derive(Debug, Clone)]
pub struct PreflightResult {
    /// Whether the plugin passed all checks (no blocks).
    pub passed: bool,
    /// Non-fatal warnings (logged but plugin still loads).
    pub warnings: Vec<String>,
    /// Fatal blocks (prevent loading).
    pub blocks: Vec<String>,
    /// Capabilities declared in the plugin manifest.
    pub declared_capabilities: PluginCapabilities,
    /// Patterns detected during analysis.
    pub detected_patterns: Vec<String>,
    /// Detailed static analysis breakdown.
    pub static_analysis: StaticAnalysis,
}

/// Detailed static analysis of plugin code.
#[derive(Debug, Clone)]
pub struct StaticAnalysis {
    /// Code uses `eval()`.
    pub has_eval: bool,
    /// Code uses dynamic `import()`.
    pub has_dynamic_import: bool,
    /// Code uses `fetch()`.
    pub has_fetch: bool,
    /// Code references `process.*`.
    pub has_process_access: bool,
    /// Detected filesystem access patterns.
    pub has_fs_access: Vec<String>,
    /// Detected network access patterns.
    pub has_network_access: Vec<String>,
    /// Suspicious string literals found.
    pub suspicious_strings: Vec<String>,
}

/// Preflight static analyzer for plugin code.
pub struct PreflightAnalyzer;

impl PreflightAnalyzer {
    /// Analyze plugin code before first execution.
    ///
    /// Checks for:
    /// - Dangerous constructs (eval, Function constructor)
    /// - Undeclared capability usage (fetch without network capability)
    /// - Suspicious patterns (rm -rf, sudo, chmod 777, etc.)
    /// - Access to unavailable globals (process, require)
    pub fn analyze(code: &str, declared: &PluginCapabilities) -> PreflightResult {
        let mut warnings = Vec::new();
        let mut blocks = Vec::new();
        let mut detected = Vec::new();

        // Dangerous constructs
        if code.contains("eval(") {
            warnings.push("Code uses eval()".into());
            detected.push("eval".into());
        }
        if code.contains("new Function(") {
            warnings.push("Uses Function constructor".into());
            detected.push("new Function".into());
        }
        if code.contains("process.") {
            warnings.push("References 'process' (not available in sandbox)".into());
            detected.push("process".into());
        }
        if code.contains("require(") {
            warnings.push("Uses require() — use ES import syntax".into());
            detected.push("require".into());
        }

        // Network capability checks
        let has_fetch = code.contains("fetch(") || code.contains("XMLHttpRequest");
        if has_fetch && declared.network.is_empty() {
            warnings.push("fetch()/XMLHttpRequest used but no network capability declared".into());
        }
        if has_fetch {
            detected.push("fetch".into());
        }

        // Filesystem access pattern detection
        let fs_patterns = [
            "fs.read",
            "fs.write",
            "readFile",
            "writeFile",
            "readText",
            "writeText",
        ];
        let fs_detected: Vec<String> = fs_patterns
            .iter()
            .filter(|p| code.contains(*p))
            .map(|p| p.to_string())
            .collect();
        if !fs_detected.is_empty() {
            detected.extend(fs_detected.clone());
        }

        // Network access pattern detection
        let net_patterns = [
            "fetch(",
            "XMLHttpRequest",
            "WebSocket",
            "http.get",
            "https.get",
        ];
        let net_detected: Vec<String> = net_patterns
            .iter()
            .filter(|p| code.contains(*p))
            .map(|p| p.to_string())
            .collect();

        // Suspicious patterns — these are blockers
        let suspicious = [
            "rm -rf",
            "sudo ",
            "chmod 777",
            "> /dev/sda",
            "rm -rf /",
            "mkfs.",
            "dd if=",
        ];
        let found: Vec<String> = suspicious
            .iter()
            .filter(|s| code.contains(*s))
            .map(|s| s.trim().to_string())
            .collect();
        detected.extend(found.clone());
        if !found.is_empty() {
            blocks.push(format!("Suspicious patterns: {}", found.join(", ")));
        }

        // Check for undeclared shell access
        if (code.contains("exec(") || code.contains("spawn(") || code.contains("child_process"))
            && !declared.shell
        {
            warnings.push(
                "Code appears to use shell/command execution but shell capability not declared"
                    .into(),
            );
            detected.push("shell_exec".into());
        }

        PreflightResult {
            passed: blocks.is_empty(),
            warnings,
            blocks,
            declared_capabilities: declared.clone(),
            detected_patterns: detected,
            static_analysis: StaticAnalysis {
                has_eval: code.contains("eval("),
                has_dynamic_import: code.contains("import("),
                has_fetch: code.contains("fetch("),
                has_process_access: code.contains("process."),
                has_fs_access: fs_detected,
                has_network_access: net_detected,
                suspicious_strings: found,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_caps() -> PluginCapabilities {
        PluginCapabilities::default()
    }

    #[test]
    fn clean_code_passes() {
        let code = r#"pi.on("TurnStart", (e) => { console.log(e); });"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.passed);
        assert!(result.warnings.is_empty());
        assert!(result.blocks.is_empty());
    }

    #[test]
    fn detects_eval() {
        let code = r#"eval("console.log('hi')");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.passed); // eval is a warning, not a block
        assert!(result.static_analysis.has_eval);
        assert!(result.warnings.iter().any(|w| w.contains("eval()")));
    }

    #[test]
    fn detects_suspicious_patterns() {
        let code = r#"const x = "rm -rf /";"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(!result.passed);
        assert!(!result.blocks.is_empty());
        assert!(
            result
                .static_analysis
                .suspicious_strings
                .contains(&"rm -rf".to_string())
        );
    }

    #[test]
    fn detects_sudo() {
        let code = r#"exec("sudo apt install something");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(!result.passed);
        assert!(result.blocks.iter().any(|b| b.contains("sudo")));
    }

    #[test]
    fn detects_fetch_without_network_capability() {
        let code = r#"fetch("https://example.com/api");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("network capability"))
        );
    }

    #[test]
    fn fetch_ok_with_network_capability() {
        let mut caps = default_caps();
        caps.network = vec!["example.com".to_string()];
        let code = r#"fetch("https://example.com/api");"#;
        let result = PreflightAnalyzer::analyze(code, &caps);
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.contains("network capability"))
        );
    }

    #[test]
    fn detects_process_access() {
        let code = r#"const env = process.env;"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.static_analysis.has_process_access);
        assert!(result.warnings.iter().any(|w| w.contains("process")));
    }

    #[test]
    fn detects_require() {
        let code = r#"const fs = require("fs");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.warnings.iter().any(|w| w.contains("require")));
    }

    #[test]
    fn detects_function_constructor() {
        let code = r#"new Function("return 1")();"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Function constructor"))
        );
    }

    #[test]
    fn detects_dynamic_import() {
        let code = r#"import("some-module");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.static_analysis.has_dynamic_import);
    }

    #[test]
    fn detects_multiple_suspicious() {
        let code = r#"exec("rm -rf /"); exec("sudo chmod 777 /");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(!result.passed);
        assert!(result.static_analysis.suspicious_strings.len() >= 2);
    }

    #[test]
    fn detects_shell_without_capability() {
        let code = r#"exec("ls -la");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.warnings.iter().any(|w| w.contains("shell")));
    }

    #[test]
    fn shell_ok_with_capability() {
        let mut caps = default_caps();
        caps.shell = true;
        let code = r#"exec("ls -la");"#;
        let result = PreflightAnalyzer::analyze(code, &caps);
        assert!(!result.warnings.iter().any(|w| w.contains("shell")));
    }

    #[test]
    fn detected_patterns_populated() {
        let code = r#"eval("x"); fetch("url");"#;
        let result = PreflightAnalyzer::analyze(code, &default_caps());
        assert!(result.detected_patterns.contains(&"eval".to_string()));
        assert!(result.detected_patterns.contains(&"fetch".to_string()));
    }
}
