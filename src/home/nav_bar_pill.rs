//! The floating pill that the mobile nav bar shrinks into while the user scrolls down the rooms list,
//! which it does instead of sliding out of view when the user's preferences keep the bottom bar shown.
//!
//! Scrolling back up, reaching the top of the list, or tapping any of its buttons expands it again.

use makepad_widgets::{animator::Ease, *};

use crate::{
    home::{
        home_screen::effective_is_desktop,
        navigation_tab_bar::{ProfileIconRef, ProfileIconWidgetExt},
    },
    shared::navigation_bar_button::{NavigationBarButtonRef, NavigationBarButtonWidgetExt},
};

const PILL_WIDTH: f64 = 224.0;
/// The most of the docked bar's width that the pill takes up, so it still floats on very narrow screens.
const MAX_PILL_WIDTH_FRACTION: f64 = 0.9;
const PILL_HEIGHT: f64 = 50.0;
/// How far the pill floats above the bottom of the screen, at least.
const PILL_LIFT: f64 = 16.0;
/// How far the pill floats above the safe area at the bottom of the screen.
const PILL_SAFE_AREA_GAP: f64 = 4.0;
const PILL_PADDING: Inset = Inset { left: 3.0, top: 0.0, right: 3.0, bottom: 0.0 };
const PILL_BUTTON_MARGIN: Inset = Inset { left: 1.0, top: 3.0, right: 1.0, bottom: 3.0 };
/// Rounds the pill's button highlights into capsules, since an SDF box draws its corners twice as large.
const PILL_BUTTON_RADIUS: f64 = (PILL_HEIGHT - PILL_BUTTON_MARGIN.top - PILL_BUTTON_MARGIN.bottom) / 4.0;
/// How large the pill's icons and avatar are, compared to the docked bar's.
const PILL_ICON_SCALE: f64 = 0.74;

/// How far the list has to scroll in one direction before the bar shrinks or expands.
const SCROLL_THRESHOLD: f64 = 12.0;
/// The bar only shrinks once the list is scrolled this far from its top.
const MIN_SCROLL_TO_SHRINK: f64 = 24.0;
/// The bar always expands once the list is scrolled back to within this distance of its top.
const MAX_SCROLL_TO_EXPAND: f64 = 2.0;
const ANIMATION_DURATION_SECS: f64 = 0.32;
/// A slight spring, which overshoots by about 4%.
const ANIMATION_EASE: Ease = Ease::Bezier { cp0: 0.3, cp1: 1.35, cp2: 0.5, cp3: 1.0 };

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // The mobile nav bar's background, which shrinks into a floating pill with a shadow and a thin ring.
    mod.widgets.NavigationBarPill = RoundedShadowView {
        // This keeps hovers over the pill's empty space from reaching the rooms beneath it.
        cursor: MouseCursor.Default
        grab_key_focus: false

        draw_bg +: {
            color: (COLOR_SECONDARY)
            pill_color: uniform((COLOR_NAVIGATION_PILL_BG))
            border_radius: 4.0
            border_color: (COLOR_NAVIGATION_PILL_RING)
            shadow_color: (COLOR_SHADOW)
            // Only the pill shows a shadow, but the bar always leaves room around it for one.
            shadow_radius: 24.0
            shadow_offset: vec2(0.0, 6.0)
            pill_radius: uniform(#(PILL_HEIGHT / 4.0))
            shrunk_fraction: uniform(0.0)

            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size3)
                let pos = self.sdf_rect_pos
                let size = self.sdf_rect_size
                let radius = mix(self.border_radius, self.pill_radius, self.shrunk_fraction)
                if self.shrunk_fraction > 0.0 {
                    // Like CSS's `box-shadow: 0 6px 18px -4px`, plus a thin ring just outside the pill.
                    let v = GaussShadow.rounded_box_shadow(
                        pos + self.shadow_offset + vec2(4.0),
                        pos + size + self.shadow_offset - vec2(4.0),
                        self.pos * self.rect_size3,
                        9.0,
                        max(0.0, 2.0 * radius - 4.0)
                    )
                    sdf.clear(self.shadow_color * v * self.shrunk_fraction)
                    sdf.box(pos.x - 1.0, pos.y - 1.0, size.x + 2.0, size.y + 2.0, radius + 0.5)
                    sdf.fill(vec4(self.border_color.xyz, self.border_color.w * self.shrunk_fraction))
                }
                // While docked, the box reaches one anti-aliasing ramp past the bar's edges,
                // so that the rooms beneath it can't show through its edge rows, e.g., at the bottom of the screen.
                let grow = (1.0 - self.shrunk_fraction) / sdf.aa
                sdf.box(pos.x - grow, pos.y - grow, size.x + 2.0 * grow, size.y + 2.0 * grow, max(1.0, radius))
                sdf.fill_keep(mix(self.color, self.pill_color, self.shrunk_fraction))
                return sdf.result
            }
        }
    }
}


/// Whether the nav bar is a pill, and how far it has shrunk into one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PillState {
    /// Whether the bar is shrinking into, or has shrunk into, the pill.
    pub is_shrunk: bool,
    /// How far the bar has shrunk, from 0 (docked) to 1 (pill). Its spring overshoots both a little.
    pub shrunk_fraction: f64,
}

/// An action emitted when the user taps the spaces button on the pill,
/// which expands the bar and then shows the spaces bar above it.
#[derive(Debug)]
pub struct ShowSpacesBarFromPillAction;

/// Draws the mobile nav bar shrunk into its floating pill, as far as its `state` says.
#[derive(Default)]
pub struct NavBarPill {
    pub state: PillState,
    /// The `shrunk_fraction` that the bar was last drawn with.
    drawn_fraction: f64,
    widgets: Option<PillWidgets>,
}

impl NavBarPill {
    /// Drops our refs to the bar's widgets, which a reload can rebuild.
    pub fn drop_widgets(&mut self) {
        self.widgets = None;
        self.drawn_fraction = 0.0;
    }

    /// The pill's area, which is empty until the mobile bar has been drawn shrunk.
    pub fn area(&self) -> Area {
        self.widgets.as_ref().map_or(Area::Empty, |widgets| widgets.pill.area())
    }

    /// Draws the nav bar, with the mobile bar shrunk only for the length of this draw,
    /// since the desktop bar draws some of the same buttons.
    pub fn draw_nav_bar(&mut self, cx: &mut Cx2d, nav_bar: &mut AdaptiveView, scope: &mut Scope, walk: Walk) -> DrawStep {
        // The desktop bar is never a pill, and the mobile bar gets rebuilt docked when the layout switches back.
        if effective_is_desktop(cx) || nav_bar.active_variant() != Some(id!(Mobile)) {
            *self = Self::default();
            return nav_bar.draw_walk(cx, scope, walk);
        }
        // A docked bar draws just like its DSL says, once its pill has been put away.
        if self.state.shrunk_fraction == 0.0 && self.drawn_fraction == 0.0 {
            return nav_bar.draw_walk(cx, scope, walk);
        }
        self.drawn_fraction = self.state.shrunk_fraction;
        let docked_size = cx.peek_walk_turtle(walk).size;
        let widgets = self.widgets.get_or_insert_with(|| PillWidgets::new(cx, nav_bar));
        let docked = widgets.shrink(cx, docked_size, self.drawn_fraction);
        let step = nav_bar.draw_walk(cx, scope, walk);
        widgets.restore(cx, &docked);
        step
    }
}

/// The widgets that shrink along with the bar.
struct PillWidgets {
    pill: ViewRef,
    buttons: [NavigationBarButtonRef; 3],
    profile_icon: ProfileIconRef,
    /// The views whose sizes, margins and padding all scale down along with the icons.
    scaled_views: [ViewRef; 7],
    avatar_text: LabelRef,
}

/// What the bar's widgets looked like before they shrank for one draw.
#[derive(Default)]
struct DockedSizes {
    pill: (Walk, Inset),
    /// Each button's walk and highlight radius.
    buttons: [(Walk, f32); 4],
    scaled_views: [(Walk, Inset); 7],
    avatar_font_scale: f32,
}

impl PillWidgets {
    fn new(cx: &mut Cx, nav_bar: &AdaptiveView) -> Self {
        Self {
            pill: nav_bar.view(cx, ids!(pill)),
            buttons: [
                nav_bar.navigation_bar_button(cx, ids!(home_button)),
                nav_bar.navigation_bar_button(cx, ids!(add_room_button)),
                nav_bar.navigation_bar_button(cx, ids!(toggle_spaces_bar_button)),
            ],
            profile_icon: nav_bar.profile_icon(cx, ids!(profile_icon)),
            scaled_views: [
                nav_bar.view(cx, ids!(home_button.icon_box)),
                nav_bar.view(cx, ids!(add_room_button.icon_box)),
                nav_bar.view(cx, ids!(toggle_spaces_bar_button.icon_box)),
                nav_bar.view(cx, ids!(profile_icon.avatar_with_badge)),
                nav_bar.view(cx, ids!(profile_icon.avatar_box)),
                nav_bar.view(cx, ids!(profile_icon.badge_box)),
                nav_bar.view(cx, ids!(profile_icon.verification_icons)),
            ],
            avatar_text: nav_bar.label(cx, ids!(profile_icon.our_own_avatar.text_view.text)),
        }
    }

    /// Calls `f` with each button's view, the profile icon's included.
    fn for_each_button(&self, mut f: impl FnMut(usize, &mut View)) {
        for (i, button) in self.buttons.iter().enumerate() {
            if let Some(mut button) = button.borrow_mut() {
                f(i, &mut button.view);
            }
        }
        if let Some(mut profile) = self.profile_icon.borrow_mut() {
            f(self.buttons.len(), &mut profile.inner.view);
        }
    }

    /// Shrinks the bar from its `docked_size` by `shrunk_fraction`, and returns what it looked like before.
    fn shrink(&self, cx: &mut Cx, docked_size: Vec2d, shrunk_fraction: f64) -> DockedSizes {
        let lerp = |docked: f64, pill: f64| docked + (pill - docked) * shrunk_fraction;
        let lerp_inset = |docked: Inset, pill: Inset| Inset {
            left: lerp(docked.left, pill.left),
            top: lerp(docked.top, pill.top),
            right: lerp(docked.right, pill.right),
            bottom: lerp(docked.bottom, pill.bottom),
        };
        let mut docked = DockedSizes::default();

        let mut inner_height = docked_size.y;
        if let Some(mut pill) = self.pill.borrow_mut() {
            docked.pill = (pill.walk, pill.layout.padding);
            let pill_width = PILL_WIDTH.min(docked_size.x * MAX_PILL_WIDTH_FRACTION);
            let side_margin = shrunk_fraction * (docked_size.x - pill_width) / 2.0;
            let lift = shrunk_fraction * (cx.display_context.safe_area_insets.bottom + PILL_SAFE_AREA_GAP).max(PILL_LIFT);
            let height = lerp(docked_size.y, PILL_HEIGHT);
            // The margins always add up to the bar's height, since the bar can't grow to fit a pill lifted above it.
            pill.walk.margin = Inset { left: side_margin, top: docked_size.y - height - lift, right: side_margin, bottom: lift };
            pill.walk.height = Size::Fixed(height);
            pill.layout.padding = lerp_inset(docked.pill.1, PILL_PADDING);
            inner_height = height - pill.layout.padding.top - pill.layout.padding.bottom;
            pill.draw_bg.draw_vars.set_uniform(cx, id!(shrunk_fraction), &[shrunk_fraction.clamp(0.0, 1.0) as f32]);
        }

        // Each button keeps filling the bar's height, minus its margins.
        self.for_each_button(|i, button| {
            let mut radius = [0.0];
            button.draw_bg.draw_vars.get_uniform(cx, id!(border_radius), &mut radius);
            docked.buttons[i] = (button.walk, radius[0]);
            button.walk.margin = lerp_inset(button.walk.margin, PILL_BUTTON_MARGIN);
            button.walk.height = Size::Fixed(inner_height - button.walk.margin.top - button.walk.margin.bottom);
            button.draw_bg.draw_vars.set_uniform(cx, id!(border_radius), &[lerp(radius[0] as f64, PILL_BUTTON_RADIUS) as f32]);
        });

        let scale = lerp(1.0, PILL_ICON_SCALE);
        let scale_size = |size: Size| if let Size::Fixed(size) = size { Size::Fixed(size * scale) } else { size };
        let scale_inset = |inset: Inset| Inset {
            left: inset.left * scale,
            top: inset.top * scale,
            right: inset.right * scale,
            bottom: inset.bottom * scale,
        };
        for (view, docked_view) in self.scaled_views.iter().zip(&mut docked.scaled_views) {
            if let Some(mut view) = view.borrow_mut() {
                *docked_view = (view.walk, view.layout.padding);
                view.walk.width = scale_size(view.walk.width);
                view.walk.height = scale_size(view.walk.height);
                view.walk.margin = scale_inset(view.walk.margin);
                view.layout.padding = scale_inset(view.layout.padding);
            }
        }
        if let Some(mut text) = self.avatar_text.borrow_mut() {
            docked.avatar_font_scale = text.draw_text.font_scale;
            text.draw_text.font_scale *= scale as f32;
        }
        docked
    }

    fn restore(&self, cx: &mut Cx, docked: &DockedSizes) {
        if let Some(mut pill) = self.pill.borrow_mut() {
            (pill.walk, pill.layout.padding) = docked.pill;
        }
        self.for_each_button(|i, button| {
            let (walk, radius) = docked.buttons[i];
            button.walk = walk;
            button.draw_bg.draw_vars.set_uniform(cx, id!(border_radius), &[radius]);
        });
        for (view, (walk, padding)) in self.scaled_views.iter().zip(docked.scaled_views) {
            if let Some(mut view) = view.borrow_mut() {
                view.walk = walk;
                view.layout.padding = padding;
            }
        }
        if let Some(mut text) = self.avatar_text.borrow_mut() {
            text.draw_text.font_scale = docked.avatar_font_scale;
        }
    }
}


/// Shrinks the nav bar into its pill or expands it back out, depending on how the user scrolls the rooms list.
#[derive(Default)]
pub struct PillMotion {
    pub state: PillState,
    /// Whether a tap on the pill expanded it, which keeps it from shrinking again
    /// until the user starts another scroll, even while the list keeps moving.
    pub is_held_open: bool,
    /// How far the list has scrolled in its current direction.
    scroll_run: f64,
    animation: Option<Animation>,
}

struct Animation {
    /// When the animation started, from [`Cx::monotonic_now()`].
    started_at: f64,
    from_fraction: f64,
    next_frame: NextFrame,
}

impl PillMotion {
    /// Shrinks the bar once the list has scrolled down a little, and expands it once it scrolls up a little
    /// or gets back to its top. `distance_from_top` is how far the list is scrolled from its very top.
    pub fn on_scroll(&mut self, cx: &mut Cx, delta: f64, distance_from_top: f64, can_shrink: bool) {
        if distance_from_top <= MAX_SCROLL_TO_EXPAND {
            self.scroll_run = 0.0;
            self.set_shrunk(cx, false);
            return;
        }
        if delta.signum() != self.scroll_run.signum() {
            self.scroll_run = 0.0;
        }
        self.scroll_run += delta;
        if self.scroll_run > SCROLL_THRESHOLD && distance_from_top > MIN_SCROLL_TO_SHRINK && can_shrink && !self.is_held_open {
            self.set_shrunk(cx, true);
        } else if self.scroll_run < -SCROLL_THRESHOLD {
            self.set_shrunk(cx, false);
        }
    }

    /// Expands the pill because the user tapped it.
    pub fn expand_on_tap(&mut self, cx: &mut Cx) {
        self.set_shrunk(cx, false);
        self.is_held_open = true;
        self.scroll_run = 0.0;
    }

    pub fn set_shrunk(&mut self, cx: &mut Cx, is_shrunk: bool) {
        if is_shrunk != self.state.is_shrunk {
            self.state.is_shrunk = is_shrunk;
            self.animation = Some(Animation {
                started_at: Cx::monotonic_now(),
                from_fraction: self.state.shrunk_fraction,
                next_frame: cx.new_next_frame(),
            });
        }
    }

    /// Moves the bar along its animation on each frame.
    pub fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        let Some(animation) = &mut self.animation else { return };
        if animation.next_frame.is_event(event).is_none() {
            return;
        }
        let time_fraction = ((Cx::monotonic_now() - animation.started_at) / ANIMATION_DURATION_SECS).min(1.0);
        let to_fraction = if self.state.is_shrunk { 1.0 } else { 0.0 };
        if time_fraction < 1.0 {
            self.state.shrunk_fraction = animation.from_fraction + (to_fraction - animation.from_fraction) * ANIMATION_EASE.map(time_fraction);
            animation.next_frame = cx.new_next_frame();
        } else {
            self.state.shrunk_fraction = to_fraction;
            self.animation = None;
        }
    }
}
