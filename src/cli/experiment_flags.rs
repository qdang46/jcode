use anyhow::{Context, Result};
use jcode_experiment_flags::{EXPERIMENT_FLAGS, Experiments, Stage};

pub fn run_experiment_list_command(json: bool) -> Result<()> {
    let config = crate::config::config();
    let experiments = Experiments::from_config(&config.experiments.entries);

    if json {
        let states = experiments.all_flag_states();
        println!("{}", serde_json::to_string_pretty(&states)?);
    } else {
        let header = format!(
            "{:25} {:25} {:8} {:8}  {}",
            "Key", "Flag", "Default", "Current", "Stage"
        );
        println!("{}", header);
        println!("{}", "-".repeat(90));
        for spec in EXPERIMENT_FLAGS {
            let enabled = experiments.check(spec.id);
            let default_str = if spec.default_enabled { "on" } else { "off" };
            let current_str = if enabled { "ON" } else { "OFF" };
            let stage_label = match spec.stage {
                Stage::UnderDevelopment => "UnderDevelopment",
                Stage::Experimental { .. } => "Experimental",
                Stage::Stable => "Stable",
                Stage::Deprecated { .. } => "Deprecated",
                Stage::Removed => "Removed",
            };
            println!(
                "{:25} {:25} {:8} {:8}  {}",
                spec.key,
                format!("{:?}", spec.id),
                default_str,
                current_str,
                stage_label,
            );
        }
    }
    Ok(())
}

pub fn run_experiment_enable_command(key: &str) -> Result<()> {
    if Experiments::resolve_key(key).is_none() {
        anyhow::bail!(
            "Unknown experiment flag '{key}'. Use 'jcode experiment list' to see valid flags."
        );
    }
    let mut config = crate::config::Config::load();
    config.experiments.entries.insert(key.to_string(), true);
    config.save().context("Failed to save config")?;
    crate::config::invalidate_config_cache();
    eprintln!("[jcode] Experiment '{key}' enabled.");
    Ok(())
}

pub fn run_experiment_disable_command(key: &str) -> Result<()> {
    if Experiments::resolve_key(key).is_none() {
        anyhow::bail!(
            "Unknown experiment flag '{key}'. Use 'jcode experiment list' to see valid flags."
        );
    }
    let mut config = crate::config::Config::load();
    config.experiments.entries.insert(key.to_string(), false);
    config.save().context("Failed to save config")?;
    crate::config::invalidate_config_cache();
    eprintln!("[jcode] Experiment '{key}' disabled.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_experiment_flags::Experiments;

    #[test]
    fn test_run_experiment_list_json_roundtrip() {
        // Build expected JSON output.
        let config = crate::config::Config::default();
        let experiments = Experiments::from_config(&config.experiments.entries);
        let states = experiments.all_flag_states();
        let json_str = serde_json::to_string_pretty(&states).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
        // We should have exactly EXPERIMENT_FLAGS.len() entries.
        assert_eq!(parsed.len(), jcode_experiment_flags::EXPERIMENT_FLAGS.len());
        // Each entry should have "flag", "key", "enabled", "default_enabled" fields.
        for (i, entry) in parsed.iter().enumerate() {
            assert!(entry.get("flag").is_some(), "missing 'flag' at index {i}");
            assert!(entry.get("key").is_some(), "missing 'key' at index {i}");
            assert!(
                entry.get("enabled").is_some(),
                "missing 'enabled' at index {i}"
            );
        }
    }

    #[test]
    fn test_run_experiment_enable_disable_roundtrip() {
        // Use a temp JCODE_HOME to isolate from user config.
        let tmp = tempfile::tempdir().unwrap();
        // JCODE_HOME points directly to the jcode data directory.
        // SAFETY: test-only env mutation, single-threaded test harness.
        unsafe {
            std::env::set_var("JCODE_HOME", tmp.path().to_str().unwrap());
        }
        // Initially hooks_v2 should be disabled by default.
        let config = crate::config::Config::load();
        assert!(
            !config
                .experiments
                .entries
                .get("hooks_v2")
                .copied()
                .unwrap_or(false)
        );
        // Enable and verify.
        run_experiment_enable_command("hooks_v2").unwrap();
        crate::config::invalidate_config_cache();
        let config2 = crate::config::Config::load();
        assert_eq!(config2.experiments.entries.get("hooks_v2"), Some(&true));
        // Disable and verify.
        run_experiment_disable_command("hooks_v2").unwrap();
        crate::config::invalidate_config_cache();
        let config3 = crate::config::Config::load();
        assert_eq!(config3.experiments.entries.get("hooks_v2"), Some(&false));
        // SAFETY: test-only env mutation, single-threaded test harness.
        unsafe {
            std::env::remove_var("JCODE_HOME");
        }
    }
}
