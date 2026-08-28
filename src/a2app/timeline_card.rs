//! The timeline card shown for `rs.robius.a2app` events: a mini-app shared
//! into a room. Fixed height, so late changes never shift the viewport.

use makepad_widgets::*;
use matrix_sdk::ruma::OwnedRoomId;

use a2app_core::bundle;
use crate::a2app::runtime::{with_a2app, A2AppOp};
use crate::shared::popup_list::{enqueue_popup_notification, PopupKind};

/// The custom Matrix event type carrying a shared mini-app bundle.
pub const A2APP_EVENT_TYPE: &str = "rs.robius.a2app";

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.MiniAppTimelineCard = set_type_default() do #(MiniAppTimelineCard::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill,
        // Fixed height: a timeline item's height must never change after
        // its first draw.
        height: 84,
        flow: Right
        spacing: 10
        align: Align{y: 0.5}
        padding: Inset{top: 10, bottom: 10, left: 12, right: 12}
        margin: Inset{top: 4, bottom: 4, left: 10, right: 10}

        show_bg: true
        draw_bg +: {
            color: #F6F8F9
            border_color: (COLOR_DIVIDER_DARK)
            border_size: 1.0
            border_radius: 4.0
        }

        card_glyph := Label {
            width: 40, height: Fit
            padding: 0, margin: 0
            draw_text +: {
                text_style: TITLE_TEXT {font_size: 24},
                color: #000
            }
        }
        View {
            width: Fill, height: Fit
            flow: Down
            spacing: 3
            card_name := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: theme.font_bold {font_size: 12},
                    color: (COLOR_TEXT)
                }
            }
            card_detail := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                max_lines: 2,
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 9.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
            }
        }
        card_install_button := RobrixIconButton {
            padding: 8,
            draw_icon +: { svg: (ICON_IMPORT) }
            icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
            text: "Install"
        }
        card_run_button := RobrixPositiveIconButton {
            padding: 8,
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            text: "Run"
        }
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppTimelineCard {
    #[deref] view: View,
    /// The raw bundle text from the event, kept for Install.
    #[rust] bundle_text: String,
    /// The already-installed app this bundle matches, if any.
    #[rust] installed_id: Option<String>,
    #[rust] room_id: Option<OwnedRoomId>,
}

impl Widget for MiniAppTimelineCard {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            if self.view.button(cx, ids!(card_install_button)).clicked(actions) {
                if self.bundle_text.is_empty() {
                    enqueue_popup_notification("This shared mini-app has no bundle content.", PopupKind::Error, Some(4.0));
                } else {
                    cx.action(A2AppOp::ImportText(self.bundle_text.clone()));
                }
            }
            if self.view.button(cx, ids!(card_run_button)).clicked(actions)
                && let Some(app_id) = self.installed_id.clone()
            {
                cx.action(A2AppOp::OpenApp {
                    app_id,
                    room_id: self.room_id.clone(),
                    in_room_pane: true,
                });
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppTimelineCardRef {
    /// Populates the card from a shared bundle's raw text.
    /// Re-set on every draw, since timeline items get recycled.
    pub fn populate(&self, cx: &mut Cx, bundle_text: &str, sender_name: &str, room_id: Option<OwnedRoomId>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.bundle_text = bundle_text.to_string();
        inner.room_id = room_id;
        match bundle::parse(bundle_text) {
            Ok(manifest) => {
                // Same source already installed (under any id): offer Run
                // instead of a duplicate Install.
                inner.installed_id = with_a2app(|state| {
                    state.registry.iter()
                        .find(|a| a.source == manifest.source)
                        .map(|a| a.id.clone())
                }).flatten();
                let wants = if manifest.permissions.is_empty() {
                    String::from("no permissions")
                } else {
                    manifest.permissions.join(", ")
                };
                inner.view.label(cx, ids!(card_glyph)).set_text(cx, &manifest.icon);
                inner.view.label(cx, ids!(card_name))
                    .set_text(cx, &format!("{} — Splash mini-app", manifest.name));
                inner.view.label(cx, ids!(card_detail))
                    .set_text(cx, &format!("Shared by {sender_name} · wants: {wants}. Installing is not granting; you approve each permission."));
            }
            Err(e) => {
                inner.installed_id = None;
                inner.view.label(cx, ids!(card_glyph)).set_text(cx, "❓");
                inner.view.label(cx, ids!(card_name)).set_text(cx, "Shared mini-app (unreadable)");
                inner.view.label(cx, ids!(card_detail)).set_text(cx, &e);
            }
        }
        let installed = inner.installed_id.is_some();
        inner.view.button(cx, ids!(card_install_button)).set_visible(cx, !installed);
        inner.view.button(cx, ids!(card_run_button)).set_visible(cx, installed);
    }
}
