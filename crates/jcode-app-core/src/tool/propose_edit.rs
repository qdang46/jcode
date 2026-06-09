use super::*;
pub(crate) struct ProposeEditTool;
impl ProposeEditTool {
    pub fn new() -> Self { Self }
}
#[async_trait::async_trait]
impl Tool for ProposeEditTool {
    fn name(&self) -> &str { "propose_edit" }
    fn description(&self) -> &str { "Propose an edit for best-of-N" }
    fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
    async fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
        Err(anyhow::anyhow!("propose_edit tool not yet implemented"))
    }
}
