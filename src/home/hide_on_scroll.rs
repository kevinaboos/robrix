//! `HideOnScrollView`: the mobile root view, whose rooms list header and bottom bars
//! slide out of view while the user scrolls down the rooms list, and back in when they scroll up.
//!
//! The bars move along with the list as it scrolls, then finish sliding in or out (whichever is closer)
//! once it stops. The bars are drawn over the rooms list, which starts and ends with blank spaces
//! as tall as the bars, so sliding them doesn't resize or move the list.
//! If the user keeps the bottom bars shown, the nav bar shrinks into a floating pill instead (see `nav_bar_pill`).

use makepad_widgets::{animator::Ease, makepad_platform::event::finger::TouchState, *};

use crate::{
    app::AppState,
    home::{
        home_screen::{SPACES_BAR_SHADOW_RADIUS, SpacesBarWrapperRef, SpacesBarWrapperWidgetExt},
        nav_bar_pill::{PillMotion, PillState, ShowSpacesBarFromPillAction},
        navigation_tab_bar::{NavigationBarAction, NavigationTabBarRef, NavigationTabBarWidgetExt, SelectedTab},
        rooms_list::{SIDE_PADDING, RoomsListRef, RoomsListWidgetExt},
    },
    settings::app_preferences::AppPreferencesGlobal,
};

/// How far the header's shadow reaches below it, which also has to slide out of view.
const HEADER_SHADOW_RADIUS: f64 = 12.0;
/// How long the shortest slide to fully shown or hidden takes. Longer slides take up to twice as long.
const SNAP_DURATION_SECS: f64 = 0.15;
/// How long a mouse wheel or trackpad has to stop scrolling the rooms list before the bars finish sliding.
const WHEEL_STOP_DELAY_SECS: f64 = 0.15;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*


    mod.widgets.HideOnScrollView = #(HideOnScrollView::register_widget(vm)) {
        width: Fill, height: Fill
        flow: Overlay
        align: Align{y: 1.0}

        content := View {
            width: Fill, height: Fill
        }

        // This doesn't clip, so the spaces bar's shadow still shows above it,
        // and its cursor keeps hovers over its empty space from reaching the rooms beneath it.
        bottom_bars := View {
            width: Fill, height: Fit
            flow: Down
            clip_x: false, clip_y: false
            cursor: MouseCursor.Default
            grab_key_focus: false
        }

        header_area := View {
            width: Fill, height: Fill
            flow: Overlay

            // While the header is hidden, the rooms beneath the status bar fade out under it,
            // like iOS's scroll edge effect.
            status_bar_fade := View {
                width: Fill, height: 0
                show_bg: true
                draw_bg +: {
                    color: (COLOR_PRIMARY)
                    pixel: fn() {
                        // Mostly opaque across the top fifth of the status bar, then easing out to nothing at its bottom edge.
                        let alpha = 0.835 * (1.0 - smoothstep(0.2, 1.0, self.pos.y))
                        return vec4(self.color.rgb * alpha, alpha)
                    }
                }
            }

            // The rooms list's title and filter bar, drawn over the top of the rooms list.
            header := RoundedShadowView {
                width: Fill, height: Fit
                margin: Inset{
                    left: (mod.widgets.SAFE_INSET_PAD_LEFT),
                    right: (mod.widgets.SAFE_INSET_PAD_RIGHT),
                }
                // The header's background extends up through the status bar area above its content.
                padding: Inset{top: (15.0 + mod.widgets.SAFE_INSET_PAD_TOP), left: #(SIDE_PADDING), right: #(SIDE_PADDING), bottom: 10}
                flow: Down,
                // This keeps hovers over the header's empty space from reaching the rooms beneath it.
                cursor: MouseCursor.Default
                grab_key_focus: false

                show_bg: true
                draw_bg +: {
                    color: (COLOR_PRIMARY_DARKER)
                    border_radius: 4.0
                    border_size: 0.0
                    shadow_color: (COLOR_HEADER_SHADOW)
                    shadow_radius: #(HEADER_SHADOW_RADIUS)
                    shadow_offset: vec2(0.0, 0.0)

                    pixel: fn() {
                        let sdf = Sdf2d.viewport(self.pos * self.rect_size3)

                        let mut fill_color = self.color
                        if self.color_2.x > -0.5 {
                            let dither = Math.random_2d(self.pos.xy) * 0.04 * self.color_dither
                            let dir = if self.gradient_fill_horizontal > 0.5 self.pos.x else self.pos.y
                            fill_color = mix(self.color self.color_2 dir + dither)
                        }

                        let mut stroke_color = self.border_color
                        if self.border_color_2.x > -0.5 {
                            let dither = Math.random_2d(self.pos.xy) * 0.04 * self.color_dither
                            let dir = if self.gradient_border_horizontal > 0.5 self.pos.x else self.pos.y
                            stroke_color = mix(self.border_color self.border_color_2 dir + dither)
                        }

                        // The box starts at the top of the shadow's area, so the clip cuts off its top edge and corners.
                        // Otherwise their anti-aliasing would let the list show through the header's top row.
                        sdf.box_y(
                            self.sdf_rect_pos.x
                            0.0
                            self.sdf_rect_size.x
                            self.sdf_rect_pos.y + self.sdf_rect_size.y
                            0.0
                            max(1.0 self.border_radius)
                        )
                        if sdf.shape > -1.0 {
                            let m = self.shadow_radius
                            let o = self.shadow_offset + self.rect_shift
                            let v = GaussShadow.rounded_box_shadow(vec2(m) + o self.rect_size2+o self.pos * (self.rect_size3+vec2(m)) self.shadow_radius*0.5 self.border_radius*2.0)
                            // Only draw shadow on the bottom half of the view
                            let pixel_y = self.pos.y * self.rect_size3.y
                            let mid_y = self.sdf_rect_pos.y + self.sdf_rect_size.y * 0.5
                            let bottom_mask = smoothstep(mid_y - m * 0.3 mid_y + m * 0.3 pixel_y)
                            sdf.clear(self.shadow_color * v * bottom_mask)
                        }

                        sdf.fill_keep(fill_color)

                        if self.border_size > 0.0 {
                            sdf.stroke(stroke_color self.border_size)
                        }
                        return sdf.result
                    }
                }

                CachedWidget {
                    rooms_list_header := RoomsListHeader {}
                }

                View {
                    width: Fill,
                    height: 45,
                    flow: Right
                    padding: Inset{top: 5, bottom: 2}
                    spacing: 5
                    align: Align{y: 0.5}

                    CachedWidget {
                        room_filter_input_bar := RoomFilterInputBar { is_main_filter: true }
                    }

                    // Hide this until it's implemented.
                    // search_messages_button := SearchMessagesButton { }
                }
            }
        }
    }
}


/// The mobile root view, which slides the rooms list's header up and its `bottom_bars` down,
/// out of view, while the user scrolls down the rooms list. The user's preferences pick which of them hide.
///
/// Its `content` holds the current page, and its `bottom_bars` sit at the bottom, over the rooms list.
#[derive(Script, Widget)]
pub struct HideOnScrollView {
    #[deref] view: View,

    /// How far the bars have slid out of view, from 0 (fully shown) to 1 (fully hidden).
    #[rust] hidden_fraction: f64,
    /// The nav bar shrinking into its floating pill, which it does instead of hiding if the bottom bars don't hide.
    #[rust] pill: PillMotion,
    /// Whether the spaces bar waits out of view for the nav bar to fully expand from its pill, and then shows.
    #[rust] is_spaces_bar_waiting_to_show: bool,
    #[rust] header_height: f64,
    #[rust] bottom_bars_height: f64,
    /// Whether we drew the header last time, which we only do while the rooms list is shown.
    #[rust] was_header_drawn: bool,
    /// The touches that started on this view and haven't ended yet.
    #[rust] touches_down: Vec<u64>,
    #[rust] is_mouse_down: bool,
    #[rust] snap: Snap,
    #[rust] child_refs: Option<ChildRefs>,
    /// The rooms list and its PortalList, which only exist once the rooms list page has been shown.
    #[rust] rooms_list: Option<(RoomsListRef, PortalListRef)>,
}

#[derive(Clone)]
struct ChildRefs {
    content: ViewRef,
    header: ViewRef,
    status_bar_fade: ViewRef,
    bottom_bars: ViewRef,
    spaces_bar_wrapper: SpacesBarWrapperRef,
    navigation_tab_bar: NavigationTabBarRef,
    filter_input: TextInputRef,
}

impl ScriptHook for HideOnScrollView {
    fn on_after_apply(&mut self, _vm: &mut ScriptVm, apply: &Apply, _scope: &mut Scope, _value: ScriptValue) {
        if apply.is_reload() {
            self.child_refs = None;
            self.rooms_list = None;
        }
    }
}

impl Widget for HideOnScrollView {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let ChildRefs { header, bottom_bars, spaces_bar_wrapper, navigation_tab_bar, filter_input, .. } = self.child_refs(cx).clone();
        let (old_fraction, old_pill_state) = (self.hidden_fraction, self.pill.state);
        if let Event::Actions(actions) = event {
            if actions.iter().any(|action| matches!(action.downcast_ref(), Some(NavigationBarAction::TabSelected(_)))) {
                // Every page starts out with its bars shown.
                self.snap.stop(cx);
                self.hidden_fraction = 0.0;
                self.pill = PillMotion::default();
            } else if actions.iter().any(|action| action.downcast_ref::<ShowSpacesBarFromPillAction>().is_some()) {
                // A spaces bar that's still partly in view just slides back up along with the nav bar.
                self.is_spaces_bar_waiting_to_show = !spaces_bar_wrapper.is_shown(cx) || self.pill.state.shrunk_fraction >= 1.0;
                self.pill.expand_on_tap(cx);
            }
        }
        self.pill.handle_event(cx, event);
        // The spaces bar shows as soon as the nav bar first reaches its full size,
        // but if the nav bar shrinks back into its pill first, the spaces bar stays as it was.
        if self.is_spaces_bar_waiting_to_show {
            let PillState { is_shrunk, shrunk_fraction } = self.pill.state;
            if !is_shrunk && shrunk_fraction <= 0.0 {
                self.is_spaces_bar_waiting_to_show = false;
                spaces_bar_wrapper.show_from_hidden(cx);
            } else if is_shrunk && shrunk_fraction >= 1.0 {
                self.is_spaces_bar_waiting_to_show = false;
            }
        }

        let prefs = &cx.global::<AppPreferencesGlobal>().0;
        let (hides_header, hides_bottom_bars) = (prefs.hide_header_on_scroll, prefs.hide_bottom_bars_on_scroll);
        let list = self.was_header_drawn.then(|| self.rooms_list(cx)).flatten().map(|(_, list)| list);
        let position_before = list.as_ref().map(|list| (list.first_id(), list.scroll_position()));

        // A press on the header or the bottom bars doesn't go to the rooms list beneath them,
        // since the list would also start scrolling from that press.
        let is_pressed_on = |area: Area| match event {
            Event::MouseDown(e) => area.clipped_rect(cx).contains(e.abs),
            Event::TouchUpdate(e) => e.touches.iter().any(|touch|
                touch.state == TouchState::Start && area.clipped_rect(cx).contains(touch.abs)
            ),
            _ => false,
        };
        // While the nav bar is a pill, the rooms beside it get the presses there.
        let bars_area = if self.pill.state.is_shrunk { navigation_tab_bar.pill_area() } else { bottom_bars.area() };
        let is_pressed_on_bars = self.was_header_drawn && (is_pressed_on(header.area()) || is_pressed_on(bars_area));
        if is_pressed_on_bars {
            header.handle_event(cx, event, scope);
            bottom_bars.handle_event(cx, event, scope);
        } else {
            self.view.handle_event(cx, event, scope);
        }

        let was_pressed = !self.touches_down.is_empty() || self.is_mouse_down;
        // A press that ended while this view was covered never reached us,
        // so we only keep the presses that are still down.
        if cx.fingers.first_mouse_button.is_none() {
            self.is_mouse_down = false;
        }
        match event {
            Event::TouchUpdate(e) => {
                self.touches_down.retain(|uid| e.touches.iter().any(|touch|
                    touch.uid == *uid && !matches!(touch.state, TouchState::Stop | TouchState::Cancel)
                ));
                // Touching this view catches the bars wherever they are,
                // and touching the rooms list starts a new scroll, which lets a tapped pill shrink again.
                for touch in &e.touches {
                    if touch.state == TouchState::Start && self.view.area().rect(cx).contains(touch.abs) {
                        self.touches_down.push(touch.uid);
                        self.snap.stop(cx);
                        if !is_pressed_on_bars {
                            self.pill.is_held_open = false;
                        }
                    }
                }
            }
            Event::MouseDown(e) if self.view.area().rect(cx).contains(e.abs) => {
                self.is_mouse_down = true;
                self.snap.stop(cx);
                if !is_pressed_on_bars {
                    self.pill.is_held_open = false;
                }
            }
            Event::MouseUp(_) => self.is_mouse_down = false,
            Event::Scroll(_) => self.pill.is_held_open = false,
            _ => {}
        }
        let is_pressed = !self.touches_down.is_empty() || self.is_mouse_down;

        if let (Some(list), Some(position_before)) = (list, position_before) {
            let position_after = (list.first_id(), list.scroll_position());
            // Only the user scrolling the list moves the bars. The list jumping to another item,
            // stretching past its top, or being pushed past its end doesn't count.
            let scroll_delta = if position_after.0 != position_before.0 {
                0.0
            } else {
                let clamp_to_top = |(first_id, first_scroll): (usize, f64)| if first_id == 0 { first_scroll.min(0.0) } else { first_scroll };
                let delta = clamp_to_top(position_before) - clamp_to_top(position_after);
                if delta > 0.0 && list.is_at_end() { 0.0 } else { delta }
            };

            // The nav bar shrinks into its pill instead of hiding, but not while the user is typing into the filter bar.
            if !hides_bottom_bars {
                if list.is_smooth_scrolling().is_some() {
                    self.pill.set_shrunk(cx, false);
                } else if scroll_delta != 0.0 {
                    let distance_from_top = if position_after.0 == 0 { -position_after.1 } else { f64::INFINITY };
                    self.pill.on_scroll(cx, scroll_delta, distance_from_top, !filter_input.key_focus(cx));
                }
            }

            if hides_header || hides_bottom_bars {
                // The header moves along with the list, or the bottom bars do if the header doesn't hide.
                let hide_distance = if hides_header {
                    self.header_height + HEADER_SHADOW_RADIUS
                } else {
                    self.bottom_bars_height + SPACES_BAR_SHADOW_RADIUS
                };
                // How far the list scrolls from its top to rest its first room just below the status bar.
                let rest_scroll = self.header_height - cx.display_context.safe_area_insets.top;

                if list.is_smooth_scrolling().is_some() {
                    // The list is scrolling by itself, e.g., to a room the user navigated to.
                    if !matches!(&self.snap, Snap::Sliding(slide) if slide.to_fraction == 0.0) {
                        self.slide_to(cx, 0.0, None);
                    }
                }
                else if scroll_delta != 0.0 {
                    // The bars stay shown while the user is typing into the filter bar.
                    if scroll_delta < 0.0 || !filter_input.key_focus(cx) {
                        self.hidden_fraction = (self.hidden_fraction + scroll_delta / hide_distance).clamp(0.0, 1.0);
                    }
                    self.snap.stop(cx);
                    if let Event::Scroll(_) = event {
                        self.snap = Snap::AwaitingWheelStop(cx.start_timeout(WHEEL_STOP_DELAY_SECS));
                    } else if !is_pressed {
                        self.snap = Snap::AwaitingListStop(cx.new_next_frame());
                    }
                }
                // Once the user lets go, the bars finish sliding as soon as the list stops moving.
                if was_pressed && !is_pressed {
                    self.snap.stop(cx);
                    self.snap = Snap::AwaitingListStop(cx.new_next_frame());
                }

                let mut should_snap = false;
                let mut should_slide_back = false;
                match &mut self.snap {
                    Snap::AwaitingListStop(next_frame) if next_frame.is_event(event).is_some() => {
                        if position_after != position_before {
                            *next_frame = cx.new_next_frame();
                        } else {
                            should_snap = true;
                        }
                    }
                    Snap::AwaitingWheelStop(timer) if timer.is_event(event).is_some() => should_snap = !is_pressed,
                    Snap::Sliding(slide) if slide.next_frame.is_event(event).is_some() => {
                        // A list that's too short to scroll its header space away stops short of where we moved it,
                        // so the bars slide back in instead.
                        if slide.list_scroll.is_some_and(|(_, to)| to > 0.0)
                            && position_after.0 == 0
                            && -position_after.1 < slide.last_list_scroll - 0.5
                        {
                            should_slide_back = true;
                        } else {
                            let progress = Ease::OutQuad.map(((Cx::monotonic_now() - slide.started_at) / slide.duration_secs).min(1.0));
                            self.hidden_fraction = slide.from_fraction + (slide.to_fraction - slide.from_fraction) * progress;
                            if let Some((from, to)) = slide.list_scroll {
                                slide.last_list_scroll = from + (to - from) * progress;
                                list.set_first_id_and_scroll_in_place(0, -slide.last_list_scroll);
                                list.redraw(cx);
                            }
                            if progress < 1.0 {
                                slide.next_frame = cx.new_next_frame();
                            } else {
                                self.snap = Snap::None;
                            }
                        }
                    }
                    _ => {}
                }
                if should_slide_back {
                    self.slide_to(cx, 0.0, None);
                }
                if should_snap {
                    self.snap.stop(cx);
                    if self.hidden_fraction > 0.0 && self.hidden_fraction < 1.0 {
                        let scrolled = -list.scroll_position();
                        let shows_header_space = list.first_id() == 0 && scrolled < self.header_height - 0.5;
                        let is_too_short = list.first_id() == 0 && scrolled < rest_scroll - 0.5 && list.is_at_end();
                        // The bars slide fully in or out, whichever is closer. They slide in if the filter bar
                        // has key focus, or if the list is too short to scroll its header space away.
                        let to_fraction = if self.hidden_fraction < 0.5 || filter_input.key_focus(cx) || is_too_short {
                            0.0
                        } else {
                            1.0
                        };
                        // While the list shows part of its header space, it scrolls along with the header,
                        // either back to its top or until it comes to rest below the status bar.
                        let list_scroll = (hides_header && shows_header_space && !is_too_short)
                            .then(|| (scrolled.max(0.0), if to_fraction == 0.0 { 0.0 } else { rest_scroll }));
                        self.slide_to(cx, to_fraction, list_scroll);
                    }
                }

                // Near the top, the header can only be as hidden as the list is close to coming to rest,
                // so it's fully shown at the very top, and it never jumps when the list leaves its resting place.
                let scrolled = -list.scroll_position();
                if hides_header && list.first_id() == 0 && scrolled < rest_scroll - 0.5 {
                    self.hidden_fraction = self.hidden_fraction.min(scrolled.max(0.0) / rest_scroll);
                }
                // The bars never stay partly hidden, e.g., if the list got shorter under a hidden header.
                if matches!(self.snap, Snap::None) && !is_pressed && self.hidden_fraction > 0.0 && self.hidden_fraction < 1.0 {
                    self.snap = Snap::AwaitingListStop(cx.new_next_frame());
                }
            }
        }

        let has_pill_changed = self.pill.state != old_pill_state;
        if self.hidden_fraction != old_fraction || has_pill_changed {
            self.view.area().redraw(cx);
            // The nav bar keeps its own draw list, which doesn't see the bars moving.
            if has_pill_changed || (hides_bottom_bars && self.hidden_fraction != old_fraction) {
                bottom_bars.redraw(cx);
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        let ChildRefs { content, header, status_bar_fade, bottom_bars, spaces_bar_wrapper, navigation_tab_bar, .. } = self.child_refs(cx).clone();
        // The header only belongs to the rooms list, not to the Settings or AddRoom pages, which also
        // stay below the status bar and end above the bottom bars, instead of going under them.
        let is_rooms_list_shown = scope.data.get::<AppState>().is_some_and(|app_state|
            matches!(app_state.selected_tab, SelectedTab::Home | SelectedTab::Space { .. })
        );
        header.set_visible(cx, is_rooms_list_shown);
        if let Some(mut content) = content.borrow_mut() {
            content.walk.margin.top = if is_rooms_list_shown { 0.0 } else { cx.display_context.safe_area_insets.top };
            content.walk.margin.bottom = if is_rooms_list_shown { 0.0 } else { self.bottom_bars_height };
        }
        let prefs = &cx.global::<AppPreferencesGlobal>().0;
        let header_fraction = if prefs.hide_header_on_scroll { self.hidden_fraction } else { 0.0 };
        let bottom_bars_fraction = if prefs.hide_bottom_bars_on_scroll { self.hidden_fraction } else { 0.0 };
        if let Some(mut header) = header.borrow_mut() {
            header.walk.margin.top = -header_fraction * (self.header_height + HEADER_SHADOW_RADIUS);
        }
        let status_bar_height = cx.display_context.safe_area_insets.top;
        status_bar_fade.set_visible(cx, is_rooms_list_shown && header_fraction > 0.0 && status_bar_height > 0.0);
        if let Some(mut fade) = status_bar_fade.borrow_mut() {
            fade.walk.height = Size::Fixed(status_bar_height);
        }
        if let Some(mut bottom_bars) = bottom_bars.borrow_mut() {
            bottom_bars.walk.margin.bottom = -bottom_bars_fraction * (self.bottom_bars_height + SPACES_BAR_SHADOW_RADIUS);
            // While the nav bar is a pill, hovers beside it reach the rooms beneath it.
            bottom_bars.cursor = (!self.pill.state.is_shrunk).then_some(MouseCursor::Default);
        }
        // The nav bar draws its pill from this, so a rebuilt mobile layout can't leave it out of date.
        navigation_tab_bar.set_pill_state(self.pill.state);
        // The spaces bar slides down behind the nav bar while the nav bar shrinks into its pill,
        // and it stays down there while it waits for the nav bar to fully expand again.
        let out_of_view_offset = self.bottom_bars_height + SPACES_BAR_SHADOW_RADIUS;
        spaces_bar_wrapper.set_slide_offset(if self.is_spaces_bar_waiting_to_show {
            out_of_view_offset
        } else {
            self.pill.state.shrunk_fraction.clamp(0.0, 1.0) * out_of_view_offset
        });
        if self.header_height > 0.0
            && let Some((rooms_list, _)) = self.rooms_list(cx)
        {
            rooms_list.set_bar_space_heights(cx, self.header_height, self.bottom_bars_height);
        }

        let step = self.view.draw_walk(cx, scope, walk);
        if step.is_done() {
            self.was_header_drawn = is_rooms_list_shown;
            let header_height = if is_rooms_list_shown { header.area().rect(cx).size.y } else { self.header_height };
            let bottom_bars_height = bottom_bars.area().rect(cx).size.y;
            if header_height != self.header_height || bottom_bars_height != self.bottom_bars_height {
                self.header_height = header_height;
                self.bottom_bars_height = bottom_bars_height;
                cx.redraw_area_in_draw(self.view.area());
            }
        }
        step
    }
}

impl HideOnScrollView {
    fn child_refs(&mut self, cx: &mut Cx) -> &ChildRefs {
        self.child_refs.get_or_insert_with(|| ChildRefs {
            content: self.view.view(cx, ids!(content)),
            header: self.view.view(cx, ids!(header)),
            status_bar_fade: self.view.view(cx, ids!(status_bar_fade)),
            bottom_bars: self.view.view(cx, ids!(bottom_bars)),
            spaces_bar_wrapper: self.view.spaces_bar_wrapper(cx, ids!(spaces_bar_wrapper)),
            navigation_tab_bar: self.view.navigation_tab_bar(cx, ids!(navigation_tab_bar)),
            filter_input: self.view.text_input(cx, ids!(room_filter_input_bar.input.text_input)),
        })
    }

    fn rooms_list(&mut self, cx: &mut Cx) -> Option<(RoomsListRef, PortalListRef)> {
        if self.rooms_list.is_none() {
            let rooms_list = self.view.rooms_list(cx, ids!(rooms_list));
            if !rooms_list.is_empty() {
                let list = rooms_list.portal_list(cx, ids!(list));
                self.rooms_list = Some((rooms_list, list));
            }
        }
        self.rooms_list.clone()
    }

    /// Starts sliding the bars to the given hidden fraction, optionally along with the list's scroll position.
    fn slide_to(&mut self, cx: &mut Cx, to_fraction: f64, list_scroll: Option<(f64, f64)>) {
        self.snap.stop(cx);
        self.snap = Snap::Sliding(Slide {
            started_at: Cx::monotonic_now(),
            from_fraction: self.hidden_fraction,
            to_fraction,
            list_scroll,
            last_list_scroll: list_scroll.map_or(0.0, |(from, _)| from),
            duration_secs: SNAP_DURATION_SECS * (1.0 + (to_fraction - self.hidden_fraction).abs()),
            next_frame: cx.new_next_frame(),
        });
    }
}

/// How the bars finish sliding fully in or out once the user is done scrolling the rooms list.
#[derive(Default)]
enum Snap {
    /// Waiting for the list to stop moving, which we check on each frame.
    AwaitingListStop(NextFrame),
    /// Waiting for the mouse wheel or trackpad to stop scrolling the list.
    AwaitingWheelStop(Timer),
    Sliding(Slide),
    #[default]
    None,
}

impl Snap {
    /// Stops waiting or sliding, which leaves the bars where they are.
    fn stop(&mut self, cx: &mut Cx) {
        if let Snap::AwaitingWheelStop(timer) = std::mem::take(self) {
            cx.stop_timer(timer);
        }
    }
}

struct Slide {
    /// When the slide started, from [`Cx::monotonic_now()`].
    started_at: f64,
    from_fraction: f64,
    to_fraction: f64,
    /// The list's scroll position (from its top) to move from and to, if it moves along with the header.
    list_scroll: Option<(f64, f64)>,
    last_list_scroll: f64,
    duration_secs: f64,
    next_frame: NextFrame,
}
