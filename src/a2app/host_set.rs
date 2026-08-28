//! Shared Splash isolate hosting, used by both the generic host modal and
//! the in-room mini-app pane.
//!
//! One `AppHost` template instance per running app; its `Splash` child owns
//! the app's isolated VM. All isolate config (fs jail, host tag, caps, net)
//! is applied BEFORE the source evals, so boot code already sees it.

use std::collections::HashMap;

use makepad_widgets::*;
use makepad_widgets::widget_async::gc_dead_splash_isolates;

use a2app_core::manifest::{MiniAppId, MiniAppManifest};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // The slot a pane's ACTIVE host is drawn into. A custom widget so the
    // host is drawn inside this turtle's own coordinate space; abs-positioned
    // drawing from the pane level lands wrong inside a translated Modal.
    mod.widgets.MiniAppHostArea = #(MiniAppHostArea::register_widget(vm)) {
        width: Fill, height: Fill
    }

    // A mini-app's chrome-less host: the Splash child owns the isolate.
    // Splash apps style themselves for a dark backdrop, and the glass kit
    // samples the scene beneath it, so the dark fill must actually paint.
    mod.widgets.MiniAppHost = View {
        width: Fill, height: Fill
        content_bg := RoundedView {
            width: Fill, height: Fill
            flow: Down
            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_color: (COLOR_SECONDARY)
                border_size: 1.0
                border_radius: 4.0
            }
            content := ScrollYView {
                width: Fill, height: Fill
                flow: Down
                padding: Inset{left: 8, right: 8, top: 8, bottom: 8}
                splash := Splash {
                    width: Fill, height: Fit
                }
            }
        }
    }
}

/// Draws one pane's active host, filling its own rect.
#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppHostArea {
    #[deref] view: View,
    #[rust] host: Option<WidgetRef>,
    /// The content box the host was last drawn at, for `on_app_resize`.
    #[rust] last_size: Vec2d,
}

impl Widget for MiniAppHostArea {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        cx.begin_turtle(walk, Layout::flow_overlay());
        let rect = cx.turtle().rect();
        self.last_size = rect.size;
        if let Some(host) = self.host.clone()
            && rect.size.x > 1.0 && rect.size.y > 1.0
        {
            let host_walk = Walk {
                abs_pos: Some(rect.pos),
                margin: Default::default(),
                width: Size::Fixed(rect.size.x),
                height: Size::Fixed(rect.size.y),
                metrics: Default::default(),
            };
            host.draw_walk_all(cx, &mut Scope::empty(), host_walk);
        }
        cx.end_turtle();
        DrawStep::done()
    }
}

impl MiniAppHostAreaRef {
    /// Sets (or clears) the host this area draws.
    pub fn set_host(&self, host: Option<WidgetRef>) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.host = host;
        }
    }

    pub fn last_size(&self) -> Vec2d {
        self.borrow().map(|inner| inner.last_size).unwrap_or_default()
    }
}

/// The live hosts owned by one hosting widget.
#[derive(Default)]
pub struct SplashHostSet {
    templates: HashMap<LiveId, ScriptObjectRef>,
    /// Live hosts for every app opened (and not force-stopped) here.
    hosts: HashMap<MiniAppId, WidgetRef>,
    /// Each running app's isolate heap key, for IPC routing.
    heap_keys: HashMap<MiniAppId, usize>,
    /// Content size each app's script was last told about, so
    /// `on_app_resize` only fires on real changes.
    host_sizes: HashMap<MiniAppId, Vec2d>,
    /// Size changes seen during draw, delivered at the next event: a
    /// draw-time `ui` lookup would silently no-op against the
    /// freshly-evaluated Splash subtree.
    pending_resize_notify: Vec<(MiniAppId, Vec2d)>,
}

impl SplashHostSet {
    /// Captures the owning widget's DSL templates; call from `on_after_apply`.
    pub fn capture_templates(&mut self, vm: &mut ScriptVm, apply: &Apply, value: ScriptValue) {
        if !apply.is_eval()
            && let Some(obj) = value.as_object()
        {
            vm.vec_with(obj, |vm, vec| {
                for kv in vec {
                    if let Some(id) = kv.key.as_id()
                        && let Some(template_obj) = kv.value.as_object()
                    {
                        self.templates.insert(id, vm.bx.heap.new_object_ref(template_obj));
                    }
                }
            });
        }
    }

    /// Drops captured templates; call from `on_before_apply` on a reload.
    pub fn clear_templates(&mut self) {
        self.templates.clear();
    }

    /// A captured template by id, for the owner to instantiate itself.
    pub fn template(&self, id: LiveId) -> Option<ScriptObjectRef> {
        self.templates.get(&id).cloned()
    }

    /// Ensures `app` has a live host (creating its Splash isolate if needed).
    /// `tag` is the host-bridge instance tag (`app` or `app@room`): it names
    /// this INSTANCE to the service broker, which resolves the room from it.
    pub fn ensure_host(
        &mut self,
        cx: &mut Cx,
        owner_uid: WidgetUid,
        manifest: &MiniAppManifest,
        grants: &[String],
        tag: &str,
    ) -> Option<WidgetRef> {
        if let Some(host) = self.hosts.get(&manifest.id) {
            return Some(host.clone());
        }
        let Some(template) = self.templates.get(&live_id!(AppHost)) else {
            error!("BUG: mini-app host widget is missing its AppHost template");
            return None;
        };
        let template_value: ScriptValue = template.as_object().into();
        let host = cx.with_vm(|vm| WidgetRef::script_from_value(vm, template_value));
        // The template is declared invisible so the pane doesn't draw it.
        host.set_visible(cx, true);
        cx.widget_tree_insert_child_deep(owner_uid, LiveId::from_str(&manifest.id), host.clone());

        // Everything the isolate is allowed to touch, assigned BEFORE the
        // source evals: the net runtime (per the user's grant, not the
        // manifest), the storage jail, and the host-bridge identity/caps.
        if let Some(mut splash) = host.widget(cx, ids!(splash)).borrow_mut::<Splash>() {
            splash.set_allow_net(grants.iter().any(|g| g == "network"));
            splash.set_sandbox_dir(cx, Some(a2app_core::app_sandbox_dir(&manifest.id)));
            splash.set_host_tag(cx, Some(tag.to_string()));
            splash.set_host_caps(cx, grants.to_vec());
            splash.set_host_prompts(cx, true);
            splash.set_debug_name(&manifest.id);
        }
        // Evaluating the source spins up the app's own isolated Splash VM.
        host.widget(cx, ids!(splash)).set_text(cx, &manifest.source);
        if let Some(mut splash) = host.widget(cx, ids!(splash)).borrow_mut::<Splash>()
            && let Some(heap_key) = splash.isolate_heap_key(cx)
        {
            self.heap_keys.insert(manifest.id.clone(), heap_key);
        }
        self.hosts.insert(manifest.id.clone(), host.clone());
        Some(host)
    }

    /// Tears down the app's host and isolate entirely.
    /// Returns whether it was running here.
    pub fn teardown(&mut self, cx: &mut Cx, owner_uid: WidgetUid, app_id: &str) -> bool {
        if self.hosts.remove(app_id).is_none() {
            return false;
        }
        self.heap_keys.remove(app_id);
        self.host_sizes.remove(app_id);
        self.pending_resize_notify.retain(|(id, _)| id != app_id);
        cx.widget_tree_mark_dirty(owner_uid);
        // Dropping the host marks its isolate dead; the GC reclaims it.
        gc_dead_splash_isolates(cx);
        true
    }

    /// Queues an `on_app_resize` notification if the app's content box changed.
    pub fn note_host_size(&mut self, app_id: &str, size: Vec2d) {
        if size.x < 1.0 || size.y < 1.0 {
            return;
        }
        let changed = self.host_sizes.get(app_id)
            .is_none_or(|prev| (prev.x - size.x).abs() > 0.5 || (prev.y - size.y).abs() > 0.5);
        if changed {
            self.host_sizes.insert(app_id.to_string(), size);
            self.pending_resize_notify.retain(|(id, _)| id != app_id);
            self.pending_resize_notify.push((app_id.to_string(), size));
        }
    }

    /// The live host widget for an app, if it runs here.
    pub fn host_of(&self, app_id: &str) -> Option<WidgetRef> {
        self.hosts.get(app_id).cloned()
    }

    /// Delivers queued `on_app_resize` calls; call at event time.
    pub fn flush_resize_notifications(&mut self, cx: &mut Cx) {
        for (app_id, size) in std::mem::take(&mut self.pending_resize_notify) {
            let Some(host) = self.hosts.get(&app_id) else { continue };
            if let Some(mut splash) = host.widget(cx, ids!(splash)).borrow_mut::<Splash>() {
                splash.call_script_fn(
                    cx,
                    live_id!(on_app_resize),
                    &[size.x.into(), size.y.into()],
                );
            }
        }
    }

    pub fn has_pending_resize(&self) -> bool {
        !self.pending_resize_notify.is_empty()
    }

    /// Forwards an event to the given app's host only.
    pub fn handle_event_for(&self, cx: &mut Cx, event: &Event, scope: &mut Scope, app_id: &str) {
        if let Some(host) = self.hosts.get(app_id).cloned() {
            host.handle_event(cx, event, scope);
        }
    }

    /// Forwards network responses to ALL hosts, so backgrounded apps'
    /// in-flight requests still complete.
    pub fn handle_network_responses(&self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let hosts: Vec<WidgetRef> = self.hosts.values().cloned().collect();
        for host in hosts {
            host.handle_event(cx, event, scope);
        }
    }

    /// Pushes a new caps list into a running app's isolate and invokes its
    /// optional `on_permissions_changed(caps)` hook.
    pub fn update_app_caps(&self, cx: &mut Cx, app_id: &str, grants: Vec<String>) {
        let Some(host) = self.hosts.get(app_id).cloned() else { return };
        let caps_json = serde_json::to_string(&grants).unwrap_or_else(|_| String::from("[]"));
        if let Some(mut splash) = host.widget(cx, ids!(splash)).borrow_mut::<Splash>() {
            splash.set_host_caps(cx, grants);
            splash.call_script_fn_with_strings(cx, live_id!(on_permissions_changed), &[&caps_json]);
        }
    }

    /// Delivers an IPC message to the target app's isolate (skipping the
    /// sender's own heap). Returns whether anything received it.
    pub fn deliver_ipc(&self, cx: &mut Cx, from_heap: usize, from: &str, to: &str, data_json: &str) -> bool {
        if self.heap_keys.get(to).copied() == Some(from_heap) {
            return false;
        }
        let Some(host) = self.hosts.get(to).cloned() else { return false };
        let splash_ref = host.widget(cx, ids!(splash));
        let Some(mut splash) = splash_ref.borrow_mut::<Splash>() else {
            return false;
        };
        splash.call_script_fn_with_strings(cx, live_id!(on_ipc_message), &[from, data_json])
    }

    pub fn is_running(&self, app_id: &str) -> bool {
        self.hosts.contains_key(app_id)
    }

    pub fn running_ids(&self) -> Vec<MiniAppId> {
        self.hosts.keys().cloned().collect()
    }
}
