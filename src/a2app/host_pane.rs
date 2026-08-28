//! The generic mini-app host: a large centered modal (event-source-modal
//! style) that runs one mini-app at a time, keeping backgrounded apps'
//! isolates alive (iOS-style) until they're force-stopped.

use makepad_widgets::*;

use a2app_core::manifest::{MiniAppId, MiniAppManifest};
use crate::a2app::host_set::{MiniAppHostAreaWidgetExt, SplashHostSet};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.MiniAppHostPane = set_type_default() do #(MiniAppHostPane::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill { max: 1000 }
        height: Fill
        margin: 40,
        flow: Down
        padding: Inset{top: 15, right: 20, bottom: 20, left: 20}

        show_bg: true
        draw_bg +: {
            color: (COLOR_PRIMARY)
            border_radius: 6.0
            border_size: 0.0
        }

        header := View {
            width: Fill, height: Fit
            flow: Right
            spacing: 10
            align: Align{y: 0.5}
            margin: Inset{bottom: 10}

            app_glyph := Label {
                width: Fit, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: TITLE_TEXT {font_size: 18},
                    color: #000
                }
            }
            app_title := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: TITLE_TEXT {font_size: 16},
                    color: #000
                }
            }

            close_button := RobrixIconButton {
                width: Fit, height: Fit,
                padding: 12,
                spacing: 0
                align: Align{x: 0.5, y: 0.5}
                icon_walk: Walk{width: 18, height: 18, margin: 0}
                draw_icon.svg: (ICON_CLOSE)
                draw_icon.color: #666
                draw_bg +: {
                    border_size: 0
                    color: #0000
                    color_hover: #00000015
                    color_down: #00000025
                }
            }
        }

        // The active host draws in here.
        host_area := mod.widgets.MiniAppHostArea {}

        // A TEMPLATE, not real content: the pane's View would happily draw it,
        // so it stays invisible; instantiated copies are un-hidden.
        AppHost := mod.widgets.MiniAppHost { visible: false }
    }
}

/// Actions this pane emits for the runtime to apply.
#[derive(Clone, Debug, Default)]
pub enum MiniAppHostPaneAction {
    /// The user closed the pane; the app keeps running in the background.
    CloseClicked,
    #[default]
    None,
}

#[derive(Script, Widget)]
pub struct MiniAppHostPane {
    #[deref] view: View,
    #[rust] host_set: SplashHostSet,
    /// The app currently shown in the pane.
    #[rust] active: Option<MiniAppId>,
}

impl ScriptHook for MiniAppHostPane {
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

impl Widget for MiniAppHostPane {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        // Deliver queued on_app_resize calls first, before any early return.
        if self.host_set.has_pending_resize() {
            self.host_set.flush_resize_notifications(cx);
        }

        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            if self.view.button(cx, ids!(close_button)).clicked(actions) {
                cx.action(MiniAppHostPaneAction::CloseClicked);
            }
        }

        // Backgrounded apps still receive network responses so in-flight
        // requests complete; everything else goes to the active host only.
        if let Event::NetworkResponses(_) = event {
            self.host_set.handle_network_responses(cx, event, scope);
        } else if let Some(active) = self.active.clone() {
            self.host_set.handle_event_for(cx, event, scope, &active);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)?;

        if let Some(active) = self.active.clone() {
            let size = self.view.mini_app_host_area(cx, ids!(host_area)).last_size();
            self.host_set.note_host_size(&active, size);
        }
        DrawStep::done()
    }
}

impl MiniAppHostPaneRef {
    /// Opens (or brings back) the given app, creating its isolate on first open.
    pub fn open_app(&self, cx: &mut Cx, manifest: &MiniAppManifest, grants: Vec<String>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let uid = inner.widget_uid();
        if inner.host_set.ensure_host(cx, uid, manifest, &grants, &manifest.id).is_none() {
            return;
        }
        inner.active = Some(manifest.id.clone());
        let host = inner.host_set.host_of(&manifest.id);
        inner.view.mini_app_host_area(cx, ids!(host_area)).set_host(host);
        inner.view.label(cx, ids!(app_glyph)).set_text(cx, &manifest.icon);
        inner.view.label(cx, ids!(app_title)).set_text(cx, &manifest.name);
        inner.view.redraw(cx);
    }

    /// Tears down the app's host and isolate entirely.
    pub fn force_stop(&self, cx: &mut Cx, app_id: &str) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let uid = inner.widget_uid();
        inner.host_set.teardown(cx, uid, app_id);
        if inner.active.as_deref() == Some(app_id) {
            inner.active = None;
            inner.view.mini_app_host_area(cx, ids!(host_area)).set_host(None);
        }
        inner.view.redraw(cx);
    }

    /// Restarts a RUNNING app with fresh grants (needed for `network`
    /// changes, whose runtime is baked in at VM alloc). No-op if not running.
    pub fn restart_if_running(&self, cx: &mut Cx, manifest: &MiniAppManifest, grants: Vec<String>) {
        let (was_running, was_active) = {
            let Some(inner) = self.borrow() else { return };
            (
                inner.host_set.is_running(&manifest.id),
                inner.active.as_deref() == Some(manifest.id.as_str()),
            )
        };
        if !was_running {
            return;
        }
        self.force_stop(cx, &manifest.id);
        if was_active {
            // open_app re-points the host area at the fresh host.
            self.open_app(cx, manifest, grants);
        } else {
            let Some(mut inner) = self.borrow_mut() else { return };
            let uid = inner.widget_uid();
            inner.host_set.ensure_host(cx, uid, manifest, &grants, &manifest.id);
        }
    }

    /// See [`SplashHostSet::update_app_caps`].
    pub fn update_app_caps(&self, cx: &mut Cx, app_id: &str, grants: Vec<String>) {
        let Some(inner) = self.borrow() else { return };
        inner.host_set.update_app_caps(cx, app_id, grants);
    }

    /// See [`SplashHostSet::deliver_ipc`].
    pub fn deliver_ipc(&self, cx: &mut Cx, from_heap: usize, from: &str, to: &str, data_json: &str) -> bool {
        let Some(inner) = self.borrow() else { return false };
        inner.host_set.deliver_ipc(cx, from_heap, from, to, data_json)
    }

    pub fn is_running(&self, app_id: &str) -> bool {
        self.borrow().is_some_and(|inner| inner.host_set.is_running(app_id))
    }

    pub fn active_app(&self) -> Option<MiniAppId> {
        self.borrow().and_then(|inner| inner.active.clone())
    }

    pub fn running_ids(&self) -> Vec<MiniAppId> {
        self.borrow()
            .map(|inner| inner.host_set.running_ids())
            .unwrap_or_default()
    }
}
