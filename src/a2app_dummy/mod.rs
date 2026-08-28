//! Dummy a2app widgets for builds without the `a2app` feature.
//!
//! DSL references to these widget names must always resolve at runtime,
//! so non-a2app builds register invisible stubs under the same names.

use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.MiniAppsScreen = View { visible: false }
    mod.widgets.MiniAppHostPane = View { visible: false }
    mod.widgets.MiniAppPermissionPrompt = View { visible: false }
    mod.widgets.MiniAppTimelineCard = View { visible: false }
    mod.widgets.MiniAppTabScreen = View { visible: false }
    mod.widgets.MiniAppDock = View {
        width: Fill, height: Fill
        flow: Down
        body := View {
            width: Fill, height: Fill
            flow: Down
            mid := View {
                width: Fill, height: Fill
                flow: Right
                center := View { width: Fill, height: Fill, flow: Down }
            }
        }
    }
}
