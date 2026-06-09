use super::*;
pub(crate) struct ProposeWriteTool;
impl ProposeWriteTool {
    pub fn new() -> Self { Self }
}
#[async_trait::async_trait]
impl Tool for ProposeWriteTool {
    fn name(&self) -> &str { "propose_write" }
    fn description(&self) -> &str { "Propose a write for best-of-N" }
    fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
    async fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
        Err(anyhow::anyhow!("propose_write tool not yet implemented"))
    }
}
