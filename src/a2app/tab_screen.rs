//! A mini-app broken out into its own desktop dock tab. The tab owns its own
//! isolate instance for the (app, room) pair; "Return to room" moves it back
//! into that room's dock.

use makepad_widgets::*;
use matrix_sdk::ruma::OwnedRoomId;

use a2app_core::manifest::{instance_tag, MiniAppId};
use crate::a2app::dock::{DockCmd, MiniAppDockAction};
use crate::a2app::host_set::{MiniAppHostAreaWidgetExt, SplashHostSet};
use crate::a2app::runtime::with_a2app;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.MiniAppTabScreen = set_type_default() do #(MiniAppTabScreen::register_widget(vm)) {
        ..mod.widgets.RoundedView
        width: Fill, height: Fill
        flow: Down
        padding: Inset{top: 6, right: 8, bottom: 8, left: 8}
        show_bg: true
        draw_bg +: { color: (COLOR_PRIMARY) }

        header := View {
            width: Fill, height: Fit
            flow: Right
            spacing: 6
            align: Align{y: 0.5}
            margin: Inset{bottom: 6}

            tab_glyph := Label {
                width: Fit, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: TITLE_TEXT {font_size: 13},
                    color: #000
                }
            }
            titles := View {
                width: Fill, height: Fit
                flow: Down
                tab_title := Label {
                    width: Fill, height: Fit
                    padding: 0, margin: 0
                    draw_text +: {
                        text_style: theme.font_bold {font_size: 12},
                        color: (COLOR_TEXT)
                    }
                }
                tab_room := Label {
                    width: Fill, height: Fit
                    padding: 0, margin: 0
                    draw_text +: {
                        text_style: REGULAR_TEXT {font_size: 9},
                        color: (MESSAGE_TEXT_COLOR)
                    }
                }
            }

            return_button := RobrixNeutralIconButton {
                padding: Inset{top: 5, bottom: 5, left: 10, right: 10},
                icon_walk: Walk{width: 0, height: 0, margin: 0}
                text: "Return to room"
            }
        }

        host_area := mod.widgets.MiniAppHostArea {}

        AppHost := mod.widgets.MiniAppHost { visible: false }
    }
}

/// Asks MainDesktopUI to open (or focus) a mini-app tab.
#[derive(Clone, Debug, Default)]
pub enum A2AppTabRequest {
    Open { app_id: MiniAppId, room_id: OwnedRoomId, room_name: String },
    #[default]
    None,
}

/// Notifications from a tab screen back to MainDesktopUI.
#[derive(Clone, Debug, Default)]
pub enum MiniAppTabScreenAction {
    /// The instance left the tab (returned to its room or was stopped);
    /// the tab itself should be closed.
    Vacated { app_id: MiniAppId, room_id: OwnedRoomId },
    #[default]
    None,
}

#[derive(Script, Widget)]
pub struct MiniAppTabScreen {
    #[deref] view: View,
    #[rust] host_set: SplashHostSet,
    #[rust] app_id: Option<MiniAppId>,
    #[rust] room_id: Option<OwnedRoomId>,
}

impl ScriptHook for MiniAppTabScreen {
    fn on_before_apply(
        &mut self,
        _vm: &mut ScriptVm,
        apply: &Apply,
        _scope: &mut Scope,
        _value: ScriptValue,
    ) {
        if apply.is_reload() {
            self.host_set.clear_templates();
        }
    }

    fn on_after_apply(
        &mut self,
        vm: &mut ScriptVm,
        apply: &Apply,
        _scope: &mut Scope,
        value: ScriptValue,
    ) {
        self.host_set.capture_templates(vm, apply, value);
        vm.cx_mut().widget_tree_mark_dirty(self.widget_uid());
    }
}

impl Widget for MiniAppTabScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if self.host_set.has_pending_resize() {
            self.host_set.flush_resize_notifications(cx);
        }

        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            for action in actions {
                match action.downcast_ref::<DockCmd>() {
                    Some(DockCmd::QuitEverywhere(app_id)) => {
                        if self.app_id.as_ref() == Some(app_id) {
                            self.vacate(cx, false);
                        }
                        continue;
                    }
                    Some(DockCmd::UpdateCaps { app_id, grants }) => {
                        if self.host_set.is_running(app_id) {
                            self.host_set.update_app_caps(cx, app_id, grants.clone());
                        }
                        continue;
                    }
                    Some(DockCmd::Restart { app_id, grants }) => {
                        if self.host_set.is_running(app_id) {
                            let Some(room_id) = self.room_id.clone() else { continue };
                            let uid = self.widget_uid();
                            self.host_set.teardown(cx, uid, app_id);
                            let manifest = with_a2app(|state| state.registry.get(app_id).cloned()).flatten();
                            if let Some(manifest) = manifest {
                                let tag = instance_tag(app_id, Some(room_id.as_str()));
                                self.host_set.ensure_host(cx, uid, &manifest, grants, &tag);
                                let host = self.host_set.host_of(app_id);
                                self.view.mini_app_host_area(cx, ids!(host_area)).set_host(host);
                            }
                            self.view.redraw(cx);
                        }
                        continue;
                    }
                    _ => {}
                }
            }

            if self.view.button(cx, ids!(return_button)).pressed(actions) {
                self.vacate(cx, true);
            }
        }

        if let Event::NetworkResponses(_) = event {
            self.host_set.handle_network_responses(cx, event, scope);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)?;

        if let Some(app_id) = self.app_id.clone() {
            let size = self.view.mini_app_host_area(cx, ids!(host_area)).last_size();
            self.host_set.note_host_size(&app_id, size);
        }
        DrawStep::done()
    }
}

impl MiniAppTabScreen {
    /// Stops this tab's instance. With `back_to_room`, the room's dock is
    /// asked to open a fresh instance in its place.
    fn vacate(&mut self, cx: &mut Cx, back_to_room: bool) {
        let (Some(app_id), Some(room_id)) = (self.app_id.take(), self.room_id.take()) else { return };
        let uid = self.widget_uid();
        self.host_set.teardown(cx, uid, &app_id);
        self.view.mini_app_host_area(cx, ids!(host_area)).set_host(None);
        cx.action(MiniAppDockAction::Quit {
            app_id: app_id.clone(),
            room_id: room_id.clone(),
        });
        cx.action(MiniAppTabScreenAction::Vacated {
            app_id: app_id.clone(),
            room_id: room_id.clone(),
        });
        if back_to_room {
            cx.action(DockCmd::Open { app_id, room_id });
        }
        self.view.redraw(cx);
    }
}

impl MiniAppTabScreenRef {
    /// Starts (or restarts) the tab's own instance of the app.
    pub fn open(&self, cx: &mut Cx, app_id: MiniAppId, room_id: OwnedRoomId, room_name: &str) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let manifest = with_a2app(|state| state.registry.get(&app_id).cloned()).flatten();
        let Some(manifest) = manifest else { return };
        let grants = a2app_core::permissions::snapshot_grants_for(&app_id);
        let uid = inner.widget_uid();
        let tag = instance_tag(&app_id, Some(room_id.as_str()));
        if inner.host_set.ensure_host(cx, uid, &manifest, &grants, &tag).is_none() {
            return;
        }
        let host = inner.host_set.host_of(&app_id);
        inner.view.mini_app_host_area(cx, ids!(host_area)).set_host(host);
        inner.view.label(cx, ids!(tab_glyph)).set_text(cx, &manifest.icon);
        inner.view.label(cx, ids!(tab_title)).set_text(cx, &manifest.name);
        inner.view.label(cx, ids!(tab_room)).set_text(cx, room_name);
        cx.action(MiniAppDockAction::Opened {
            app_id: app_id.clone(),
            room_id: room_id.clone(),
        });
        inner.app_id = Some(app_id);
        inner.room_id = Some(room_id);
        inner.view.redraw(cx);
    }

    /// Stops the tab's instance without a return; used when the tab is closed.
    pub fn quit(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.vacate(cx, false);
        }
    }

    pub fn instance(&self) -> Option<(MiniAppId, OwnedRoomId)> {
        self.borrow().and_then(|inner| {
            Some((inner.app_id.clone()?, inner.room_id.clone()?))
        })
    }
}
