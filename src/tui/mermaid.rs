pub use jcode_tui_mermaid::*;

pub fn install_jcode_mermaid_hooks() {
    jcode_tui_mermaid::set_log_hooks(Some(crate::logging::info));
    jcode_tui_mermaid::set_render_completed_hook(Some(|| {
        crate::bus::Bus::global().publish(crate::bus::BusEvent::MermaidRenderCompleted);
    }));
    jcode_tui_mermaid::set_memory_snapshot_hook(None);
}
