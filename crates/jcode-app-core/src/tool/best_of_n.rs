use super::*;

pub(crate) struct BestOfNTool;

impl BestOfNTool {
    pub fn new(provider: Arc<dyn jcode_provider_core::Provider>, registry: Registry) -> Self {
        let _ = (provider, registry);
        Self
    }
}

#[async_trait::async_trait]
impl Tool for BestOfNTool {
    fn name(&self) -> &str { "best_of_n" }
    fn description(&self) -> &str { "Best-of-N parallel editing" }
    fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
    async fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
        Err(anyhow::anyhow!("best_of_n tool not yet implemented"))
    }
}
