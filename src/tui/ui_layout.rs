use ftui_style::{Color, Style};
use crate::tui::dim_color;

#[allow(unused_imports)] // re-exported for downstream `ui` consumers
pub(crate) use jcode_tui_render::chrome::{
    align_if_unset, centered_content_block_width, clear_area, draw_right_rail_chrome,
    left_aligned_content_inset, left_pad_lines_to_block_width,
};

pub(super) fn right_rail_border_style(focused: bool, focus_color: Color) -> Style {
    jcode_tui_render::chrome::right_rail_border_style(
        focused,
        focus_color,
        dim_color(),
    )
}
