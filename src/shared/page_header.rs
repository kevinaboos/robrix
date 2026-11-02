//! The header atop a root page, like Settings or AddRoom: the page's title with a close button to its right.

use makepad_widgets::*;

/// The space between a page's content and the screen's left and right edges, which its header lines up with.
pub const PAGE_SIDE_PADDING: f64 = 15.0;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // In the mobile view mode, the `HideOnScrollView` slides this out of view while the user scrolls the page down.
    mod.widgets.PageHeader = View {
        width: Fill, height: Fit
        flow: Down
        padding: Inset{top: 5, left: #(PAGE_SIDE_PADDING)}

        title_row := View {
            flow: Right,
            width: Fill, height: Fit
            margin: Inset{top: 5, left: 5, right: #(5.0 + PAGE_SIDE_PADDING)}
            spacing: 10,

            title := TitleLabel {
                padding: 0,
                margin: Inset{ left: 1, top: 11 },
                draw_text +: {
                    text_style: theme.font_regular {font_size: 18},
                }
            }

            // The "X" close button on the top right
            close_button := RobrixNeutralIconButton {
                width: Fit,
                height: Fit,
                spacing: 0,
                margin: 0,
                padding: 15,
                draw_icon.svg: (ICON_CLOSE)
                icon_walk: Walk{width: 14, height: 14}
            }
        }

        // Make sure the dividing line is aligned with the close_button
        LineH { padding: 10, margin: Inset{top: 10, right: #(2.0 + PAGE_SIDE_PADDING)} }
    }
}
