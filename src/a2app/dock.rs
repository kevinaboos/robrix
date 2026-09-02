//! The per-RoomScreen mini-app dock: every mini-app instance this room is
//! running, docked to one of the four edges around the timeline, resizable
//! along its one meaningful axis, or minimized to a chip in the top-right.
//!
//! One dock per RoomScreen means one isolate per (app, room): the same app
//! can run in several rooms at once, each instance bound to its own room.

use std::collections::HashMap;

use makepad_widgets::*;
use matrix_sdk::ruma::OwnedRoomId;

use a2app_core::manifest::{instance_tag, MiniAppId};
use crate::a2app::host_set::{MiniAppHostAreaWidgetRefExt, SplashHostSet};
use crate::a2app::runtime::with_a2app;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // The row of minimized-app chips, right-aligned above the room content.
    mod.widgets.MiniAppChipsRow = set_type_default() do #(MiniAppChipsRow::register_widget(vm)) {
        ..mod.widgets.RoundedView
        width: Fill, height: Fill
        show_bg: false
    }

    // One edge of the dock; its panes are owned by the MiniAppDock and drawn
    // here manually, so children() never reports them (the widget tree keeps
    // manually inserted children linked regardless).
    mod.widgets.MiniAppEdge = set_type_default() do #(MiniAppEdge::register_widget(vm)) {
        ..mod.widgets.RoundedView
        width: Fit, height: Fit
        draw_bg +: { color: #0000 }
        draw_handle +: {
            color: (COLOR_SECONDARY_DARKER)
            // A soft capsule with grip dots, the way a drag handle reads on
            // macOS: quiet until you go near it.
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let w = self.rect_size.x
                let h = self.rect_size.y
                let dot = vec4(1.0, 1.0, 1.0, 0.92)
                if h > w {
                    sdf.box(0.0, 0.0, w, h, w * 0.5)
                    sdf.fill(self.color)
                    let gap = w * 0.52
                    let cy = h * 0.5
                    sdf.circle(w * 0.5, cy - gap * 2.0, 1.15)
                    sdf.circle(w * 0.5, cy - gap, 1.15)
                    sdf.circle(w * 0.5, cy, 1.15)
                    sdf.circle(w * 0.5, cy + gap, 1.15)
                    sdf.circle(w * 0.5, cy + gap * 2.0, 1.15)
                    return sdf.fill(dot)
                }
                sdf.box(0.0, 0.0, w, h, h * 0.5)
                sdf.fill(self.color)
                let gap = h * 0.52
                let cx = w * 0.5
                sdf.circle(cx - gap * 2.0, h * 0.5, 1.15)
                sdf.circle(cx - gap, h * 0.5, 1.15)
                sdf.circle(cx, h * 0.5, 1.15)
                sdf.circle(cx + gap, h * 0.5, 1.15)
                sdf.circle(cx + gap * 2.0, h * 0.5, 1.15)
                return sdf.fill(dot)
            }
        }
        draw_grab +: { color: #00000001 }
    }

    // Docked panes are REAL flow children around the center, so they reflow
    // the timeline; each edge sizes itself via its walk() override.
    mod.widgets.MiniAppDock = set_type_default() do #(MiniAppDock::register_widget(vm)) {
        ..mod.widgets.RoundedView
        width: Fill, height: Fill
        flow: Overlay

        body := View {
            width: Fill, height: Fill
            flow: Down
            edge_top := mod.widgets.MiniAppEdge {}
            mid := View {
                width: Fill, height: Fill
                flow: Right
                edge_left := mod.widgets.MiniAppEdge {}
                center := View { width: Fill, height: Fill, flow: Down }
                edge_right := mod.widgets.MiniAppEdge {}
            }
            edge_bottom := mod.widgets.MiniAppEdge {}
        }

        // Floats over the room content: a minimized app must not push the
        // timeline down, so this layer reserves no space of its own.
        chips_row := mod.widgets.MiniAppChipsRow {}

        // The frame around one docked instance: header bar + the app itself.
        // A TEMPLATE: kept invisible so the dock's View never draws it.
        PaneFrame := RoundedView {
            visible: false
            width: Fill, height: Fill
            flow: Down
            margin: 2
            padding: Inset{top: 4, right: 6, bottom: 6, left: 6}
            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 4.0
                border_size: 1.0
                border_color: (COLOR_SECONDARY_DARKER)
            }

            header := View {
                width: Fill, height: Fit
                flow: Right
                spacing: 4
                align: Align{y: 0.5}
                margin: Inset{bottom: 4}

                pane_glyph := Label {
                    width: Fit, height: Fit
                    padding: 0, margin: 0
                    draw_text +: {
                        text_style: TITLE_TEXT {font_size: 12},
                        color: #000
                    }
                }
                titles := View {
                    width: Fill, height: Fit
                    flow: Down
                    pane_title := Label {
                        width: Fill, height: Fit
                        padding: 0, margin: 0
                        draw_text +: {
                            text_style: theme.font_bold {font_size: 11},
                            color: (COLOR_TEXT)
                        }
                    }
                    pane_room := Label {
                        width: Fill, height: Fit
                        padding: 0, margin: 0
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 8.5},
                            color: (MESSAGE_TEXT_COLOR)
                        }
                    }
                }

                header_buttons := View {
                    width: Fit, height: Fit
                    flow: Flow.Right{wrap: true}
                    spacing: 2
                    align: Align{x: 1.0}

                pane_edge_button := RobrixIconButton {
                    width: Fit, height: Fit,
                    padding: 6,
                    spacing: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 13, height: 13, margin: 0}
                    draw_icon.svg: (ICON_PANEL_BOTTOM)
                    draw_icon.color: #666
                    draw_bg +: {
                        border_size: 0
                        color: #0000
                        color_hover: #00000015
                        color_down: #00000025
                    }
                }
                pane_tab_button := RobrixIconButton {
                    width: Fit, height: Fit,
                    padding: 6,
                    spacing: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 13, height: 13, margin: 0}
                    draw_icon.svg: (ICON_EXTERNAL_LINK)
                    draw_icon.color: #666
                    draw_bg +: {
                        border_size: 0
                        color: #0000
                        color_hover: #00000015
                        color_down: #00000025
                    }
                }
                pane_minimize_button := RobrixIconButton {
                    width: Fit, height: Fit,
                    padding: Inset{top: 2, bottom: 6, left: 7, right: 7},
                    spacing: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 0, height: 0, margin: 0}
                    text: "−"
                    draw_text +: {
                        text_style: theme.font_bold {font_size: 13},
                        color: #666
                    }
                    draw_bg +: {
                        border_size: 0
                        color: #0000
                        color_hover: #00000015
                        color_down: #00000025
                    }
                }
                pane_close_button := RobrixIconButton {
                    width: Fit, height: Fit,
                    padding: 6,
                    spacing: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 12, height: 12, margin: 0}
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
            }

            host_area := mod.widgets.MiniAppHostArea {}
        }

        // A minimized instance's chip. Also a template.
        Chip := RobrixNeutralIconButton {
            visible: false
            padding: Inset{top: 5, bottom: 5, left: 10, right: 10},
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            draw_bg +: {
                color: #xF2F4F8E0
                color_hover: #xE6E9EFF2
                color_down: #xD8DCE6F2
                border_color: #x00000022
                border_size: 1.0
            }
        }

        AppHost := mod.widgets.MiniAppHost { visible: false }
    }
}

/// Which edge of the room screen a pane is docked to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaneSide {
    Top,
    Bottom,
    Left,
    #[default]
    Right,
}

impl PaneSide {
    fn next(self) -> Self {
        match self {
            PaneSide::Right => PaneSide::Bottom,
            PaneSide::Bottom => PaneSide::Left,
            PaneSide::Left => PaneSide::Top,
            PaneSide::Top => PaneSide::Right,
        }
    }
    fn is_vertical(self) -> bool {
        matches!(self, PaneSide::Left | PaneSide::Right)
    }
}

/// Commands broadcast by the a2app runtime; each dock applies the ones that
/// concern its room / its running instances. Fire-and-forget by design.
#[derive(Clone, Debug, Default)]
pub enum DockCmd {
    /// Open (or restore) `app_id` in the dock of the RoomScreen showing `room_id`.
    Open { app_id: MiniAppId, room_id: OwnedRoomId },
    /// Quit every docked instance of this app, in every room.
    QuitEverywhere(MiniAppId),
    /// Push a fresh grants list into this app's running instances.
    UpdateCaps { app_id: MiniAppId, grants: Vec<String> },
    /// Restart this app's running instances with fresh grants.
    Restart { app_id: MiniAppId, grants: Vec<String> },
    /// Deliver an IPC message to `to`'s docked instances (skipping the
    /// sender's own heap).
    DeliverIpc { from_heap: usize, from: MiniAppId, to: MiniAppId, data_json: String },
    #[default]
    None,
}

/// Notifications from a dock back to the runtime's instance bookkeeping.
#[derive(Clone, Debug, Default)]
pub enum MiniAppDockAction {
    Opened { app_id: MiniAppId, room_id: OwnedRoomId },
    Quit { app_id: MiniAppId, room_id: OwnedRoomId },
    #[default]
    None,
}

struct Instance {
    pane: WidgetRef,
    /// Width last given to the header's button box, so a reflow only
    /// rewrites the walk (and redraws) when it actually changes.
    header_width: f64,
    side: PaneSide,
    minimized: bool,
    chip: WidgetRef,
}

#[derive(Script, Widget)]
pub struct MiniAppDock {
    #[deref] view: View,
    #[rust] host_set: SplashHostSet,
    #[rust] room_id: Option<OwnedRoomId>,
    #[rust] room_name: String,
    #[rust] instances: HashMap<MiniAppId, Instance>,
    #[rust] sides_assigned: bool,
}

impl ScriptHook for MiniAppDock {
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

impl Widget for MiniAppDock {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.sides_assigned {
            self.sides_assigned = true;
            for side in [PaneSide::Top, PaneSide::Bottom, PaneSide::Left, PaneSide::Right] {
                self.edge(cx, side).set_side(side);
            }
        }
        if self.host_set.has_pending_resize() {
            self.host_set.flush_resize_notifications(cx);
        }

        self.reflow_headers(cx);

        // Chips and panes FIRST. Makepad resolves overlapping hits by
        // dispatch order, not draw order: whoever claims the pointer first
        // wins it. A chip floats over the timeline, so dispatching the
        // timeline first would let it swallow every click on the chip.
        for inst in self.instances.values() {
            inst.chip.handle_event(cx, event, scope);
            if !inst.minimized {
                inst.pane.handle_event(cx, event, scope);
            }
        }
        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            for action in actions {
                match action.downcast_ref::<DockCmd>() {
                    Some(DockCmd::Open { app_id, room_id }) => {
                        if self.room_id.as_ref() == Some(room_id) {
                            self.open_app(cx, app_id.clone());
                        }
                        continue;
                    }
                    Some(DockCmd::QuitEverywhere(app_id)) => {
                        self.quit_app(cx, app_id.clone(), false);
                        continue;
                    }
                    Some(DockCmd::UpdateCaps { app_id, grants }) => {
                        if self.host_set.is_running(app_id) {
                            self.host_set.update_app_caps(cx, app_id, grants.clone());
                        }
                        continue;
                    }
                    Some(DockCmd::Restart { app_id, grants }) => {
                        self.restart_app(cx, app_id, grants);
                        continue;
                    }
                    Some(DockCmd::DeliverIpc { from_heap, from, to, data_json }) => {
                        self.host_set.deliver_ipc(cx, *from_heap, from, to, data_json);
                        continue;
                    }
                    Some(DockCmd::None) | None => {}
                }
            }

            let clicked: Vec<(MiniAppId, PaneButton)> = self.instances.iter()
                .filter_map(|(app_id, inst)| {
                    // Press-based: a manual redraw between down and up can
                    // lose the finger capture, so these manually-drawn frames
                    // never see Clicked reliably. Pressed always arrives.
                    let hit = |id: &[LiveId]| inst.pane.button(cx, id).pressed(actions);
                    if inst.minimized {
                        return inst.chip.as_button().pressed(actions)
                            .then(|| (app_id.clone(), PaneButton::Chip));
                    }
                    let b = if hit(ids!(pane_close_button)) {
                        PaneButton::Close
                    } else if hit(ids!(pane_minimize_button)) {
                        PaneButton::Minimize
                    } else if hit(ids!(pane_edge_button)) {
                        PaneButton::CycleEdge
                    } else if hit(ids!(pane_tab_button)) {
                        PaneButton::BreakOutTab
                    } else {
                        return None;
                    };
                    Some((app_id.clone(), b))
                })
                .collect();
            for (app_id, button) in clicked {
                match button {
                    PaneButton::Close => self.quit_app(cx, app_id, true),
                    PaneButton::Minimize => self.set_minimized(cx, &app_id, true),
                    PaneButton::Chip => self.set_minimized(cx, &app_id, false),
                    PaneButton::CycleEdge => self.cycle_edge(cx, &app_id),
                    PaneButton::BreakOutTab => {
                        // Quit the docked instance; the tab starts its own.
                        let Some(room_id) = self.room_id.clone() else { continue };
                        let room_name = self.room_name.clone();
                        self.quit_app(cx, app_id.clone(), false);
                        cx.action(crate::a2app::tab_screen::A2AppTabRequest::Open {
                            app_id,
                            room_id,
                            room_name,
                        });
                    }
                }
            }
        }

        if let Event::NetworkResponses(_) = event {
            self.host_set.handle_network_responses(cx, event, scope);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)?;

        for (app_id, _) in self.instances.iter().filter(|(_, i)| !i.minimized) {
            let size = self.pane_host_area(cx, app_id)
                .map(|area| area.last_size())
                .unwrap_or_default();
            self.host_set.note_host_size(app_id, size);
        }
        DrawStep::done()
    }
}

const CHIP_WIDTH: f64 = 150.0;

enum PaneButton {
    Close,
    Minimize,
    Chip,
    CycleEdge,
    BreakOutTab,
}

impl MiniAppDock {
    fn edge(&self, cx: &mut Cx, side: PaneSide) -> MiniAppEdgeRef {
        let id = match side {
            PaneSide::Top => ids!(edge_top),
            PaneSide::Bottom => ids!(edge_bottom),
            PaneSide::Left => ids!(edge_left),
            PaneSide::Right => ids!(edge_right),
        };
        self.view.mini_app_edge(cx, id)
    }

    fn pane_host_area(&self, cx: &mut Cx, app_id: &str) -> Option<crate::a2app::host_set::MiniAppHostAreaRef> {
        let inst = self.instances.get(app_id)?;
        Some(inst.pane.mini_app_host_area(cx, ids!(host_area)))
    }

    fn open_app(&mut self, cx: &mut Cx, app_id: MiniAppId) {
        let Some(room_id) = self.room_id.clone() else { return };
        if let Some(inst) = self.instances.get(&app_id) {
            // Already here: restoring a minimized instance is the "open".
            if inst.minimized {
                self.set_minimized(cx, &app_id, false);
            }
            return;
        }
        // The same room can be open in several tabs (= several docks); only
        // one of them may own the (app, room) instance.
        let already_running_elsewhere = with_a2app(|state| {
            state.room_instances.get(&app_id).is_some_and(|rooms| rooms.contains(&room_id))
        }).unwrap_or(false);
        if already_running_elsewhere {
            return;
        }
        let manifest = with_a2app(|state| state.registry.get(&app_id).cloned()).flatten();
        let Some(manifest) = manifest else { return };
        let grants = a2app_core::permissions::snapshot_grants_for(&app_id);
        let uid = self.widget_uid();
        let tag = instance_tag(&app_id, Some(room_id.as_str()));
        if self.host_set.ensure_host(cx, uid, &manifest, &grants, &tag).is_none() {
            return;
        }
        let Some(host) = self.host_set.host_of(&app_id) else { return };

        let Some(pane) = self.instantiate(cx, live_id!(PaneFrame)) else { return };
        pane.set_visible(cx, true);
        cx.widget_tree_insert_child_deep(uid, LiveId::from_str(&format!("pane_{app_id}")), pane.clone());
        pane.label(cx, ids!(pane_glyph)).set_text(cx, &manifest.icon);
        pane.label(cx, ids!(pane_title)).set_text(cx, &manifest.name);
        pane.label(cx, ids!(pane_room)).set_text(cx, &self.room_name);
        // Breaking out into a dock tab only exists in the desktop layout.
        let is_desktop = crate::home::home_screen::effective_is_desktop(cx);
        pane.button(cx, ids!(pane_tab_button)).set_visible(cx, is_desktop);
        pane.mini_app_host_area(cx, ids!(host_area)).set_host(Some(host));

        let Some(chip) = self.instantiate(cx, live_id!(Chip)) else { return };
        chip.set_text(cx, &format!("{} {}", manifest.icon, manifest.name));
        cx.widget_tree_insert_child_deep(uid, LiveId::from_str(&format!("chip_{app_id}")), chip.clone());

        let side = PaneSide::default();
        self.edge(cx, side).add_pane(&app_id, pane.clone());
        self.instances.insert(app_id.clone(), Instance {
            pane,
            header_width: 0.0,
            side,
            minimized: false,
            chip,
        });
        self.apply_edge_icon(cx, &app_id);
        cx.action(MiniAppDockAction::Opened { app_id, room_id });
        self.view.redraw(cx);
    }

    /// Instantiates one of this dock's DSL templates.
    fn instantiate(&mut self, cx: &mut Cx, template: LiveId) -> Option<WidgetRef> {
        let obj = self.host_set.template(template)?;
        let value: ScriptValue = obj.as_object().into();
        Some(cx.with_vm(|vm| WidgetRef::script_from_value(vm, value)))
    }

    /// Quits one instance (close = quit). `user_clicked` controls the action
    /// emitted so the runtime can drop its once-grants when the last instance
    /// of the app dies.
    fn quit_app(&mut self, cx: &mut Cx, app_id: MiniAppId, _user_clicked: bool) {
        let Some(inst) = self.instances.remove(&app_id) else { return };
        self.edge(cx, inst.side).remove_pane(&app_id);
        self.view.mini_app_chips_row(cx, ids!(chips_row)).remove_chip(&app_id);
        let uid = self.widget_uid();
        self.host_set.teardown(cx, uid, &app_id);
        if let Some(room_id) = self.room_id.clone() {
            cx.action(MiniAppDockAction::Quit { app_id, room_id });
        }
        self.view.redraw(cx);
    }

    fn restart_app(&mut self, cx: &mut Cx, app_id: &str, grants: &[String]) {
        if !self.host_set.is_running(app_id) {
            return;
        }
        let Some(room_id) = self.room_id.clone() else { return };
        let uid = self.widget_uid();
        self.host_set.teardown(cx, uid, app_id);
        let manifest = with_a2app(|state| state.registry.get(app_id).cloned()).flatten();
        let Some(manifest) = manifest else { return };
        let tag = instance_tag(app_id, Some(room_id.as_str()));
        self.host_set.ensure_host(cx, uid, &manifest, grants, &tag);
        if let Some(inst) = self.instances.get(app_id) {
            let host = self.host_set.host_of(app_id);
            inst.pane.mini_app_host_area(cx, ids!(host_area)).set_host(host);
        }
        self.view.redraw(cx);
    }

    /// One row of buttons while the title has width, otherwise as many rows
    /// as the pane's height allows so the title gets the width back.
    fn reflow_headers(&mut self, cx: &mut Cx) {
        const BUTTONS: &[&[LiveId]] = &[
            ids!(pane_edge_button),
            ids!(pane_tab_button),
            ids!(pane_minimize_button),
            ids!(pane_close_button),
        ];
        const SPACING: f64 = 2.0;
        let mut updates: Vec<(MiniAppId, f64)> = Vec::new();
        for (app_id, inst) in self.instances.iter() {
            if inst.minimized {
                continue;
            }
            // Measure the header itself, not the pane: the frame's padding
            // and the app glyph are not the title's to spend.
            let header_width = inst.pane.view(cx, ids!(header)).area().rect(cx).size.x;
            let glyph_width = inst.pane.label(cx, ids!(pane_glyph)).area().rect(cx).size.x;
            if header_width <= 0.0 {
                continue;
            }
            let for_title_and_buttons = header_width - glyph_width - HEADER_SPACING * 2.0;
            let widths: Vec<f64> = BUTTONS
                .iter()
                .map(|id| inst.pane.button(cx, id).area().rect(cx).size.x)
                .collect();
            // Nothing drawn yet: leave the box alone and try after a draw.
            if widths.iter().any(|w| *w <= 0.0) {
                continue;
            }
            let widest = widths.iter().copied().fold(0.0_f64, f64::max);
            let count = widths.len();
            let box_width = |cols: usize| {
                widest * cols as f64 + SPACING * (cols - 1) as f64
            };
            // Widest arrangement the title can still afford, 4 down to 1.
            let mut cols = (1..=count)
                .rev()
                .find(|c| for_title_and_buttons - box_width(*c) >= HEADER_TITLE_MIN)
                .unwrap_or(1);

            // Off a single row, keep trading columns for rows until the
            // stack would eat too much of a short pane.
            let button_height = inst.pane.button(cx, ids!(pane_close_button)).area().rect(cx).size.y;
            let pane_height = inst.pane.area().rect(cx).size.y;
            if cols < count && button_height > 0.0 && pane_height > 0.0 {
                let stack_height = |cols: usize| {
                    let rows = count.div_ceil(cols) as f64;
                    button_height * rows + SPACING * (rows - 1.0)
                };
                while cols > 1 && stack_height(cols - 1) <= pane_height * HEADER_STACK_SHARE {
                    cols -= 1;
                }
            }
            let target = box_width(cols);
            if (target - inst.header_width).abs() > 0.5 {
                updates.push((app_id.clone(), target));
            }
        }
        if updates.is_empty() {
            return;
        }
        for (app_id, target) in updates {
            let Some(inst) = self.instances.get_mut(&app_id) else { continue };
            inst.header_width = target;
            let buttons = inst.pane.view(cx, ids!(header_buttons));
            if let Some(mut buttons) = buttons.borrow_mut() {
                buttons.walk.width = Size::Fixed(target);
            }
        }
        self.view.redraw(cx);
    }

    /// Points the edge button at wherever the next click would send the
    /// pane, so it reads as "move to the bottom" rather than a bare pin.
    fn apply_edge_icon(&self, cx: &mut Cx, app_id: &str) {
        let Some(inst) = self.instances.get(app_id) else { return };
        let mut button = inst.pane.button(cx, ids!(pane_edge_button));
        match inst.side.next() {
            PaneSide::Top => {
                script_apply_eval!(cx, button, { draw_icon +: { svg: (mod.widgets.ICON_PANEL_TOP) } });
            }
            PaneSide::Bottom => {
                script_apply_eval!(cx, button, { draw_icon +: { svg: (mod.widgets.ICON_PANEL_BOTTOM) } });
            }
            PaneSide::Left => {
                script_apply_eval!(cx, button, { draw_icon +: { svg: (mod.widgets.ICON_PANEL_LEFT) } });
            }
            PaneSide::Right => {
                script_apply_eval!(cx, button, { draw_icon +: { svg: (mod.widgets.ICON_PANEL_RIGHT) } });
            }
        }
    }

    fn set_minimized(&mut self, cx: &mut Cx, app_id: &str, minimized: bool) {
        let Some(inst) = self.instances.get_mut(app_id) else { return };
        if inst.minimized == minimized {
            return;
        }
        inst.minimized = minimized;
        let side = inst.side;
        let (pane, chip) = (inst.pane.clone(), inst.chip.clone());
        let chips_row = self.view.mini_app_chips_row(cx, ids!(chips_row));
        if minimized {
            self.edge(cx, side).remove_pane(app_id);
            pane.set_visible(cx, false);
            chip.set_visible(cx, true);
            chips_row.add_chip(app_id, chip);
        } else {
            pane.set_visible(cx, true);
            chip.set_visible(cx, false);
            chips_row.remove_chip(app_id);
            self.edge(cx, side).add_pane(app_id, pane);
        }
        self.view.redraw(cx);
    }

    fn cycle_edge(&mut self, cx: &mut Cx, app_id: &str) {
        let Some(inst) = self.instances.get_mut(app_id) else { return };
        let old = inst.side;
        let new = old.next();
        inst.side = new;
        let pane = inst.pane.clone();
        self.edge(cx, old).remove_pane(app_id);
        self.edge(cx, new).add_pane(app_id, pane);
        self.apply_edge_icon(cx, app_id);
        self.view.redraw(cx);
    }
}

impl MiniAppDockRef {
    /// Tells the dock which room its RoomScreen now shows (plus the display
    /// name for pane headers). Reusing this screen for a DIFFERENT room quits
    /// the old room's instances: they belong to that room, not this screen.
    pub fn set_room(&self, cx: &mut Cx, room_id: Option<OwnedRoomId>, room_name: &str) {
        let Some(mut inner) = self.borrow_mut() else { return };
        if inner.room_id == room_id {
            inner.room_name = room_name.to_string();
            return;
        }
        let apps: Vec<MiniAppId> = inner.instances.keys().cloned().collect();
        for app_id in apps {
            inner.quit_app(cx, app_id, false);
        }
        inner.room_id = room_id;
        inner.room_name = room_name.to_string();
    }
}

// -----------------------------------------------------------------------
// One dock edge: draws its panes side by side plus a drag handle strip on
// its inner border for resizing the edge's one meaningful axis.
// -----------------------------------------------------------------------

const EDGE_HANDLE: f64 = 8.0;
/// The visible grab handle: a small pill centered on the inner border.
const GRAB_LEN: f64 = 38.0;
const GRAB_THICK: f64 = 8.0;
/// Small floor so the drag handle itself stays grabbable.
const EDGE_MIN_SIZE: f64 = 60.0;

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppEdge {
    #[deref] view: View,
    #[live] draw_handle: DrawColor,
    /// The (invisible) full-strip hit zone; the pill is just the visual.
    #[live] draw_grab: DrawColor,
    #[rust] side: PaneSide,
    #[rust] panes: Vec<(MiniAppId, WidgetRef)>,
    #[rust(300.0)] size: f64,
    #[rust] drag: Option<(f64, f64)>,
    #[rust] hovering: bool,
    #[rust] handle_area: Area,
}

/// Grip colors: darker than the pane border it sits on, so it reads as a
/// handle rather than part of the outline.
const GRAB_IDLE: Vec4 = Vec4 { x: 0.62, y: 0.62, z: 0.62, w: 1.0 };
const GRAB_HOVER: Vec4 = Vec4 { x: 0.42, y: 0.42, z: 0.42, w: 1.0 };
const GRAB_DRAG: Vec4 = Vec4 { x: 0.30, y: 0.30, z: 0.30, w: 1.0 };

/// What the title needs before the header stops giving room to the buttons
/// and wraps them into a second line instead.
const HEADER_TITLE_MIN: f64 = 104.0;
/// How much of a pane's height the stacked header buttons may take before
/// they stop growing; the rest belongs to the app.
const HEADER_STACK_SHARE: f64 = 0.5;
/// The header row's own spacing, between the glyph, the title and the buttons.
const HEADER_SPACING: f64 = 4.0;
/// `PaneFrame`'s border width; the grip centres on the middle of that line.
const PANE_BORDER: f64 = 1.0;
/// The grabbable strip, straddling that border.
const GRAB_STRIP: f64 = 12.0;

impl MiniAppEdge {
    fn resize_cursor(&self) -> MouseCursor {
        if self.side.is_vertical() { MouseCursor::ColResize } else { MouseCursor::RowResize }
    }

    /// Slop around the strip, across its thin axis only.
    fn grab_inset(&self, slop: f64) -> Inset {
        if self.side.is_vertical() {
            Inset { left: slop, right: slop, top: 0.0, bottom: 0.0 }
        } else {
            Inset { left: 0.0, right: 0.0, top: slop, bottom: slop }
        }
    }

    /// Writes this edge's size into its own view walk; the parent flow
    /// reads it on the next layout pass.
    fn apply_walk(&mut self) {
        let extent = if self.panes.is_empty() { 0.0 } else { self.size + EDGE_HANDLE };
        let (width, height) = if self.side.is_vertical() {
            (Size::Fixed(extent), Size::Fill { weight: 1.0, min: None, max: None })
        } else {
            (Size::Fill { weight: 1.0, min: None, max: None }, Size::Fixed(extent))
        };
        self.view.walk.width = width;
        self.view.walk.height = height;
    }
}

impl Widget for MiniAppEdge {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);

        // Fingers are blunter than a cursor, so the strip takes a wider hit
        // margin on touch, exactly as Makepad's own Splitter does.
        let hit = event.hits_with_options(
            cx,
            self.handle_area,
            HitOptions::new()
                .with_margin(self.grab_inset(3.0))
                .with_touch_margin(self.grab_inset(8.0)),
        );
        match hit {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(self.resize_cursor());
                self.hovering = true;
                self.draw_handle.redraw(cx);
            }
            Hit::FingerHoverOut(_) => {
                self.hovering = false;
                self.draw_handle.redraw(cx);
            }
            Hit::FingerDown(fe) if fe.is_primary_hit() => {
                cx.set_cursor(self.resize_cursor());
                let start = if self.side.is_vertical() { fe.abs.x } else { fe.abs.y };
                self.drag = Some((start, self.size));
                self.draw_handle.redraw(cx);
            }
            Hit::FingerMove(fe) => {
                if let Some((start, start_size)) = self.drag {
                    let now = if self.side.is_vertical() { fe.abs.x } else { fe.abs.y };
                    let delta = match self.side {
                        // Dragging the inner handle away from its edge grows it.
                        PaneSide::Left | PaneSide::Top => now - start,
                        PaneSide::Right | PaneSide::Bottom => start - now,
                    };
                    self.size = (start_size + delta).max(EDGE_MIN_SIZE);
                    self.apply_walk();
                    // Our own walk changed, and only the PARENT's layout pass
                    // reads it; redrawing just ourselves would keep the old rect.
                    cx.redraw_all();
                }
            }
            Hit::FingerUp(fe) => {
                self.drag = None;
                self.hovering = fe.is_over && fe.device.has_hovers();
                self.draw_handle.redraw(cx);
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        if self.panes.is_empty() {
            return DrawStep::done();
        }
        // The dock hands us our exact rect via the walk.
        cx.begin_turtle(walk, Layout::flow_overlay());
        let rect = cx.turtle().rect();

        // Split the edge's rect into the pane region and the handle strip on
        // the INNER border (the side facing the timeline).
        let (pane_rect, handle_rect) = match self.side {
            PaneSide::Left => (
                Rect { pos: rect.pos, size: Vec2d { x: self.size, y: rect.size.y } },
                Rect {
                    pos: Vec2d { x: rect.pos.x + self.size, y: rect.pos.y },
                    size: Vec2d { x: EDGE_HANDLE, y: rect.size.y },
                },
            ),
            PaneSide::Right => (
                Rect {
                    pos: Vec2d { x: rect.pos.x + EDGE_HANDLE, y: rect.pos.y },
                    size: Vec2d { x: self.size, y: rect.size.y },
                },
                Rect { pos: rect.pos, size: Vec2d { x: EDGE_HANDLE, y: rect.size.y } },
            ),
            PaneSide::Top => (
                Rect { pos: rect.pos, size: Vec2d { x: rect.size.x, y: self.size } },
                Rect {
                    pos: Vec2d { x: rect.pos.x, y: rect.pos.y + self.size },
                    size: Vec2d { x: rect.size.x, y: EDGE_HANDLE },
                },
            ),
            PaneSide::Bottom => (
                Rect {
                    pos: Vec2d { x: rect.pos.x, y: rect.pos.y + EDGE_HANDLE },
                    size: Vec2d { x: rect.size.x, y: self.size },
                },
                Rect { pos: rect.pos, size: Vec2d { x: rect.size.x, y: EDGE_HANDLE } },
            ),
        };

        // Panes split the edge evenly along its long axis.
        let n = self.panes.len() as f64;
        // Where the first pane's frame actually landed: the grip centers on
        // the border it draws, which is not the rect we hand it (an abs walk
        // and the border's own width both shift it).
        let mut frame = pane_rect;
        for (i, (_, pane)) in self.panes.iter().enumerate() {
            let i = i as f64;
            let sub = if self.side.is_vertical() {
                let h = pane_rect.size.y / n;
                Rect {
                    pos: Vec2d { x: pane_rect.pos.x, y: pane_rect.pos.y + i * h },
                    size: Vec2d { x: pane_rect.size.x, y: h },
                }
            } else {
                let w = pane_rect.size.x / n;
                Rect {
                    pos: Vec2d { x: pane_rect.pos.x + i * w, y: pane_rect.pos.y },
                    size: Vec2d { x: w, y: pane_rect.size.y },
                }
            };
            let pane_walk = Walk {
                abs_pos: Some(sub.pos),
                margin: Default::default(),
                width: Size::Fixed(sub.size.x),
                height: Size::Fixed(sub.size.y),
                metrics: Default::default(),
            };
            pane.draw_walk_all(cx, &mut Scope::empty(), pane_walk);
            if i == 0.0 {
                let drawn = pane.area().rect(cx);
                if drawn.size.x > 1.0 && drawn.size.y > 1.0 {
                    frame = drawn;
                }
            }
        }

        // Both the grip and its hit strip straddle the pane's OWN outer
        // border, so the handle looks attached to the pane's edge instead of
        // floating in the gutter beside it.
        let (grab_rect, hit_rect) = if self.side.is_vertical() {
            let border_x = match self.side {
                PaneSide::Right => frame.pos.x + PANE_BORDER * 0.5,
                _ => frame.pos.x + frame.size.x - PANE_BORDER * 0.5,
            };
            let len = GRAB_LEN.min(frame.size.y * 0.5);
            (
                Rect {
                    pos: Vec2d {
                        x: border_x - GRAB_THICK * 0.5,
                        y: frame.pos.y + (frame.size.y - len) * 0.5,
                    },
                    size: Vec2d { x: GRAB_THICK, y: len },
                },
                Rect {
                    pos: Vec2d { x: border_x - GRAB_STRIP * 0.5, y: frame.pos.y },
                    size: Vec2d { x: GRAB_STRIP, y: frame.size.y },
                },
            )
        } else {
            let border_y = match self.side {
                PaneSide::Bottom => frame.pos.y + PANE_BORDER * 0.5,
                _ => frame.pos.y + frame.size.y - PANE_BORDER * 0.5,
            };
            let len = GRAB_LEN.min(frame.size.x * 0.5);
            (
                Rect {
                    pos: Vec2d {
                        x: frame.pos.x + (frame.size.x - len) * 0.5,
                        y: border_y - GRAB_THICK * 0.5,
                    },
                    size: Vec2d { x: len, y: GRAB_THICK },
                },
                Rect {
                    pos: Vec2d { x: frame.pos.x, y: border_y - GRAB_STRIP * 0.5 },
                    size: Vec2d { x: frame.size.x, y: GRAB_STRIP },
                },
            )
        };
        // A splitter-style grip: subtle at rest, darker under the cursor, so
        // it reads as a divider you can grab rather than a scrollbar.
        self.draw_handle.color = match (self.drag.is_some(), self.hovering) {
            (true, _) => GRAB_DRAG,
            (_, true) => GRAB_HOVER,
            _ => GRAB_IDLE,
        };
        self.draw_grab.draw_abs(cx, hit_rect);
        self.draw_handle.draw_abs(cx, grab_rect);
        self.handle_area = self.draw_grab.area();
        let _ = handle_rect;

        cx.end_turtle();
        DrawStep::done()
    }
}

impl MiniAppEdgeRef {
    /// How much of the dock's cross-axis this edge takes right now.
    pub fn extent(&self) -> f64 {
        self.borrow()
            .map(|inner| if inner.panes.is_empty() { 0.0 } else { inner.size + EDGE_HANDLE })
            .unwrap_or(0.0)
    }

    pub fn add_pane(&self, app_id: &str, pane: WidgetRef) {
        let Some(mut inner) = self.borrow_mut() else { return };
        if !inner.panes.iter().any(|(id, _)| id == app_id) {
            inner.panes.push((app_id.to_string(), pane));
        }
        inner.apply_walk();
    }

    pub fn remove_pane(&self, app_id: &str) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.panes.retain(|(id, _)| id != app_id);
        inner.apply_walk();
    }

    pub fn set_side(&self, side: PaneSide) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.side = side;
            inner.apply_walk();
        }
    }
}


// -----------------------------------------------------------------------
// The minimized-chips strip: a real flow element above the room content, so
// chips never fight anything else for hits.
// -----------------------------------------------------------------------

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppChipsRow {
    #[deref] view: View,
    #[rust] chips: Vec<(MiniAppId, WidgetRef)>,
}

impl Widget for MiniAppChipsRow {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        if self.chips.is_empty() {
            return DrawStep::done();
        }
        cx.begin_turtle(walk, Layout::flow_overlay());
        let rect = cx.turtle().rect();
        let mut x = rect.pos.x + rect.size.x - 10.0;
        for (_, chip) in &self.chips {
            x -= CHIP_WIDTH + 6.0;
            let chip_walk = Walk {
                abs_pos: Some(Vec2d { x, y: rect.pos.y + 3.0 }),
                margin: Default::default(),
                width: Size::Fixed(CHIP_WIDTH),
                height: Size::fit(),
                metrics: Default::default(),
            };
            chip.draw_walk_all(cx, &mut Scope::empty(), chip_walk);
        }
        cx.end_turtle();
        DrawStep::done()
    }
}

impl MiniAppChipsRowRef {
    pub fn add_chip(&self, app_id: &str, chip: WidgetRef) {
        let Some(mut inner) = self.borrow_mut() else { return };
        if !inner.chips.iter().any(|(id, _)| id == app_id) {
            inner.chips.push((app_id.to_string(), chip));
        }
    }

    pub fn remove_chip(&self, app_id: &str) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.chips.retain(|(id, _)| id != app_id);
    }
}
