use crate::config::TemperatureStrategyConfig;
use crate::types::CandidateStrategy;

/// Generate N candidate strategies with temperature diversity.
///
/// Strategy: spread temperatures evenly across [min, max] range.
/// For example, with N=4, min=0.2, max=1.0:
///   temperatures = [0.2, 0.47, 0.73, 1.0]
///
/// When explicit temperature values are configured, those are used
/// directly (in order, cycling if fewer values than candidates).
///
/// Modeled after codebuff's strategy diversity approach, adapted
/// for temperature-only diversity (model diversity is a future option).
pub fn generate_strategies(
    count: usize,
    config: &TemperatureStrategyConfig,
) -> Vec<CandidateStrategy> {
    let temperatures = resolve_temperatures(count, config);

    temperatures
        .into_iter()
        .map(|temp| {
            let label = if temp < 0.01 {
                "precise".to_string()
            } else if temp < 0.4 {
                format!("low-temp-{:.1}", temp)
            } else if temp < 0.7 {
                format!("medium-temp-{:.1}", temp)
            } else {
                format!("high-temp-{:.1}", temp)
            };

            CandidateStrategy {
                label,
                temperature: temp,
                model: None,
            }
        })
        .collect()
}

/// Resolve the temperature values for N candidates.
fn resolve_temperatures(count: usize, config: &TemperatureStrategyConfig) -> Vec<f64> {
    if !config.values.is_empty() {
        // Use configured values (cycling if fewer values than count).
        config.values.iter().cycle().take(count).copied().collect()
    } else {
        // Auto-generate evenly-spaced temperatures across the range.
        let min = config.min.max(0.0);
        let max = config.max.min(2.0).max(min);
        let range = max - min;

        if count <= 1 || range < 0.01 {
            return vec![(min + max) / 2.0];
        }

        (0..count)
            .map(|i| {
                let fraction = i as f64 / (count - 1) as f64;
                ((min + fraction * range) * 100.0).round() / 100.0
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generates_correct_count() {
        let config = TemperatureStrategyConfig::default();
        let strategies = generate_strategies(4, &config);

        assert_eq!(strategies.len(), 4);
    }

    #[test]
    fn test_temperatures_spread_evenly() {
        let config = TemperatureStrategyConfig::default(); // min=0.2, max=1.0
        let strategies = generate_strategies(4, &config);

        assert!((strategies[0].temperature - 0.2).abs() < 0.01);
        assert!((strategies[3].temperature - 1.0).abs() < 0.01);
        // Middle values should exist
        assert!(strategies[1].temperature > 0.2);
        assert!(strategies[2].temperature < 1.0);
    }

    #[test]
    fn test_labels_match_temperature_range() {
        let config = TemperatureStrategyConfig {
            values: vec![0.0, 0.3, 0.5, 0.9],
            ..Default::default()
        };
        let strategies = generate_strategies(4, &config);

        assert_eq!(strategies[0].label, "precise");
        assert!(strategies[1].label.starts_with("low-temp"));
        assert!(strategies[2].label.starts_with("medium-temp"));
        assert!(strategies[3].label.starts_with("high-temp"));
    }

    #[test]
    fn test_cycles_values_when_fewer_values_than_count() {
        let config = TemperatureStrategyConfig {
            values: vec![0.2, 0.8],
            ..Default::default()
        };
        let strategies = generate_strategies(4, &config);

        assert_eq!(strategies.len(), 4);
        assert!((strategies[0].temperature - 0.2).abs() < 0.01);
        assert!((strategies[1].temperature - 0.8).abs() < 0.01);
        assert!((strategies[2].temperature - 0.2).abs() < 0.01); // cycles back
        assert!((strategies[3].temperature - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_single_candidate_uses_midpoint() {
        let config = TemperatureStrategyConfig::default();
        let strategies = generate_strategies(1, &config);

        assert_eq!(strategies.len(), 1);
        assert!((strategies[0].temperature - 0.6).abs() < 0.01); // (0.2 + 1.0) / 2
    }

    #[test]
    fn test_large_count_remains_bounded() {
        let config = TemperatureStrategyConfig::default();
        let strategies = generate_strategies(10, &config);

        assert_eq!(strategies.len(), 10);
        for s in &strategies {
            assert!(s.temperature >= 0.2);
            assert!(s.temperature <= 1.0);
        }
    }
}
