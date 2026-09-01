//! The runtime permission prompt for mini-apps: "«App» wants to «do X»",
//! with Allow / Allow Once / Don't Allow / Not Now.
//!
//! Shown one at a time; the queue lives in [`crate::a2app::runtime`].

use makepad_widgets::*;
use a2app_core::permissions::Permission;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.MiniAppPermissionPrompt = set_type_default() do #(MiniAppPermissionPrompt::register_widget(vm)) {
        ..mod.widgets.SmallModal

        // Wide enough for all four answer buttons on one row.
        width: Fill { max: 560 }

        prompt_glyph := Label {
            width: Fill, height: Fit
            align: Align{x: 0.5}
            margin: Inset{bottom: 10}
            draw_text +: {
                text_style: TITLE_TEXT {font_size: 28},
                color: #000
            }
        }
        prompt_title := ModalTitle {
            margin: Inset{bottom: 10}
        }
        prompt_blurb := ModalBody {}
        prompt_reason := ModalBody {
            margin: Inset{top: 10}
            draw_text +: {
                text_style: REGULAR_TEXT {font_size: 10.5},
                color: (MESSAGE_TEXT_COLOR)
            }
        }

        ModalButtonsRow {
            align: Align{x: 0.5, y: 0.5}
            spacing: 10

            not_now_button := RobrixNeutralIconButton {
                padding: 12,
                icon_walk: Walk{width: 0, height: 0, margin: 0}
                text: "Not Now"
            }
            deny_button := RobrixNegativeIconButton {
                padding: 12,
                draw_icon +: { svg: (ICON_FORBIDDEN) }
                icon_walk: Walk{width: 14, height: 14, margin: Inset{left: -2, right: -1}}
                text: "Don't Allow"
            }
            allow_once_button := RobrixIconButton {
                padding: 12,
                icon_walk: Walk{width: 0, height: 0, margin: 0}
                text: "Allow Once"
            }
            allow_button := RobrixPositiveIconButton {
                padding: 12,
                draw_icon +: { svg: (ICON_CHECKMARK) }
                icon_walk: Walk{width: 14, height: 14, margin: Inset{left: -2, right: -1}}
                text: "Allow"
            }
        }
    }
}

/// What the prompt modal needs to display one request.
pub struct PromptInfo {
    pub app_name: String,
    pub app_icon: String,
    pub perm: Permission,
    /// The app author's own `why-<perm>` reason, if declared.
    pub reason: Option<String>,
    /// The specific ability that triggered the ask (its catalog title), when
    /// a parked request identifies one.
    pub capability: Option<String>,
}

/// The user's answer, emitted as a global action for the runtime to apply.
#[derive(Clone, Copy, Debug, Default)]
pub enum PermissionPromptAction {
    Allow,
    /// A session-only grant: dropped when the app's isolate is torn down.
    AllowOnce,
    Deny,
    /// Nothing persists; this (app, permission) stops asking for the session.
    NotNow,
    #[default]
    None,
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppPermissionPrompt {
    #[deref] view: View,
    /// Runtime-tier permissions offer Allow Once; normal-tier ones don't.
    #[rust] show_once: bool,
}

impl Widget for MiniAppPermissionPrompt {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            if self.view.button(cx, ids!(allow_button)).clicked(actions) {
                cx.action(PermissionPromptAction::Allow);
            } else if self.view.button(cx, ids!(allow_once_button)).clicked(actions) {
                cx.action(PermissionPromptAction::AllowOnce);
            } else if self.view.button(cx, ids!(deny_button)).clicked(actions) {
                cx.action(PermissionPromptAction::Deny);
            } else if self.view.button(cx, ids!(not_now_button)).clicked(actions) {
                cx.action(PermissionPromptAction::NotNow);
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppPermissionPromptRef {
    /// Populates the prompt for the given request.
    pub fn show(&self, cx: &mut Cx, info: &PromptInfo) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show_once = info.perm.tier() == a2app_core::permissions::Tier::Runtime;
        inner.view.label(cx, ids!(prompt_glyph)).set_text(cx, info.perm.glyph());
        let asked = info.capability.as_deref().unwrap_or(info.perm.title());
        inner.view.label(cx, ids!(prompt_title)).set_text(cx, &format!(
            "{} \"{}\" wants to: {}",
            info.app_icon, info.app_name, asked,
        ));
        // Allowing answers for the whole group; App Info can narrow it.
        let blurb = match info.capability {
            Some(_) => format!(
                "{} Allowing covers the \"{}\" group; single abilities can be blocked in App Info.",
                info.perm.blurb(), info.perm.title(),
            ),
            None => info.perm.blurb().to_string(),
        };
        inner.view.label(cx, ids!(prompt_blurb)).set_text(cx, &blurb);
        let reason_text = match info.reason.as_deref() {
            Some(reason) => format!("The app's stated reason: \"{reason}\""),
            None => String::from("The app gave no reason for needing this."),
        };
        inner.view.label(cx, ids!(prompt_reason)).set_text(cx, &reason_text);
        inner.view.button(cx, ids!(allow_once_button)).set_visible(cx, inner.show_once);
        inner.view.redraw(cx);
    }
}
