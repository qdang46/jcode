use super::*;

pub(super) fn maybe_write_context_json(
    _params: &AgentGrepInput,
    _ctx: &ToolContext,
) -> Result<Option<PathBuf>> {
    // Context system is not needed with FFS backend
    Ok(None)
}

#[cfg(test)]
pub(super) fn collect_bash_exposure(_session: &Session) -> Vec<ToolExposureObservation> {
    Vec::new()
}

#[cfg(test)]
pub(super) fn collect_trace_exposure(
    _session: &Session,
    _tool: &str,
) -> Vec<ToolExposureObservation> {
    Vec::new()
}

#[cfg(test)]
pub(super) fn tune_known_file(
    _observations: &[ToolExposureObservation],
    _known: &mut AgentGrepKnownFile,
) {
}

#[cfg(test)]
pub(super) fn tune_known_region(
    _observations: &[ToolExposureObservation],
    _known: &mut AgentGrepKnownRegion,
) {
}
