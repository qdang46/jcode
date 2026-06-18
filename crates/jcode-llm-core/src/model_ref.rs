use crate::schema::ModelRef;

impl ModelRef {
    /// Parse a string in the format `"provider/model"` or `"provider/model:variant"`
    /// into a `ModelRef`.
    ///
    /// # Errors
    ///
    /// Returns an error if the string does not contain a `/` separator.
    pub fn parse(s: &str) -> Result<Self, String> {
        let (provider_id, rest) = s
            .split_once('/')
            .ok_or_else(|| format!("invalid model ref '{s}': missing '/' separator"))?;

        let (id, variant) = match rest.split_once(':') {
            Some((model_id, v)) => (model_id.to_string(), Some(v.to_string())),
            None => (rest.to_string(), None),
        };

        Ok(ModelRef {
            provider_id: provider_id.into(),
            id,
            variant,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_ref_parse_basic() {
        let m = ModelRef::parse("anthropic/claude-sonnet-4-20250514").unwrap();
        assert_eq!(m.provider_id.as_str(), "anthropic");
        assert_eq!(m.id, "claude-sonnet-4-20250514");
        assert_eq!(m.variant, None);
    }

    #[test]
    fn test_model_ref_parse_with_variant() {
        let m = ModelRef::parse("openai/gpt-4o:latest").unwrap();
        assert_eq!(m.provider_id.as_str(), "openai");
        assert_eq!(m.id, "gpt-4o");
        assert_eq!(m.variant.as_deref(), Some("latest"));
    }

    #[test]
    fn test_model_ref_parse_no_slash() {
        let err = ModelRef::parse("just-a-model").unwrap_err();
        assert!(err.contains("missing '/' separator"));
    }

    #[test]
    fn test_model_ref_parse_multi_colon() {
        let m = ModelRef::parse("gemini/gemini-2.5-pro:beta:extra").unwrap();
        assert_eq!(m.provider_id.as_str(), "gemini");
        // only the first colon separates variant; the rest stays in the variant
        assert_eq!(m.variant.as_deref(), Some("beta:extra"));
    }
}
