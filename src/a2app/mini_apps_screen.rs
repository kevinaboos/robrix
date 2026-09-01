//! The Mini Apps management screen: create/modify apps with AI, list and
//! run installed apps, and manage each app's permissions, versions, and data.
//!
//! Three panes in one widget (list, per-app info, AI providers) plus a
//! source-viewer overlay, toggled by visibility. All mutations go through
//! [`A2AppOp`] actions applied by [`crate::a2app::runtime`].

use makepad_widgets::*;
use makepad_code_editor::code_view::CodeViewWidgetExt;

use a2app_core::manifest::{A2AppScope, MiniAppId};
use a2app_core::permissions::{Effective, GrantState, Permission};
use a2app_core::persistence;
use a2app_core::versions::AppVersion;

use crate::a2app::runtime::{with_a2app, A2AppOp};
use crate::shared::popup_list::{enqueue_popup_notification, PopupKind};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // One installed app in the list: icon, name, details, and actions.
    mod.widgets.MiniAppRow = set_type_default() do #(MiniAppRow::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill, height: Fit
        flow: Right
        spacing: 10
        align: Align{y: 0.5}
        padding: Inset{top: 8, bottom: 8, left: 10, right: 10}

        show_bg: true
        draw_bg +: {
            color: (COLOR_PRIMARY)
            border_radius: 4.0
        }

        row_glyph := Label {
            width: 32, height: Fit
            padding: 0, margin: 0
            draw_text +: {
                text_style: TITLE_TEXT {font_size: 18},
                color: #000
            }
        }
        View {
            width: Fill, height: Fit
            flow: Down
            spacing: 2
            row_name := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: theme.font_bold {font_size: 12},
                    color: (COLOR_TEXT)
                }
            }
            row_detail := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10},
                    color: (MESSAGE_TEXT_COLOR)
                }
            }
        }
        row_open_button := RobrixIconButton {
            padding: 8,
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            text: "Open"
        }
        row_info_button := RobrixNeutralIconButton {
            padding: 8,
            draw_icon +: { svg: (ICON_INFO) }
            icon_walk: Walk{width: 14, height: 14, margin: 0}
            text: ""
        }
    }

    // One declared permission in the app info pane.
    mod.widgets.MiniAppPermissionRow = set_type_default() do #(MiniAppPermissionRow::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill, height: Fit
        flow: Right
        spacing: 8
        align: Align{y: 0.5}
        padding: Inset{top: 6, bottom: 6, left: 10, right: 10}

        perm_glyph := Label {
            width: 26, height: Fit
            padding: 0, margin: 0
            draw_text +: {
                text_style: TITLE_TEXT {font_size: 14},
                color: #000
            }
        }
        View {
            width: Fill, height: Fit
            flow: Down
            spacing: 2
            perm_title := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: theme.font_bold {font_size: 11},
                    color: (COLOR_TEXT)
                }
            }
            perm_blurb := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 9.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
            }
        }
        perm_state := Label {
            width: 90, height: Fit
            padding: 0, margin: 0
            align: Align{x: 1.0}
            draw_text +: {
                text_style: theme.font_bold {font_size: 10.5},
                color: (COLOR_TEXT)
            }
        }
        perm_change_button := RobrixNeutralIconButton {
            padding: 8,
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            text: "Change"
        }
    }

    // One capability under a permission group in the app info pane: the
    // single ability, its tags, and its own allow/block override.
    mod.widgets.MiniAppCapabilityRow = set_type_default() do #(MiniAppCapabilityRow::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill, height: Fit
        flow: Right
        spacing: 8
        align: Align{y: 0.5}
        padding: Inset{top: 3, bottom: 3, left: 44, right: 10}

        View {
            width: Fill, height: Fit
            flow: Down
            spacing: 1
            cap_title := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10.5},
                    color: (COLOR_TEXT)
                }
            }
            cap_tags := Label {
                width: Fill, height: Fit
                padding: 0, margin: 0
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 8.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
            }
        }
        cap_state := Label {
            width: 110, height: Fit
            padding: 0, margin: 0
            align: Align{x: 1.0}
            draw_text +: {
                text_style: REGULAR_TEXT {font_size: 9.5},
                color: (MESSAGE_TEXT_COLOR)
            }
        }
        cap_change_button := RobrixNeutralIconButton {
            padding: Inset{top: 4, bottom: 4, left: 8, right: 8},
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            text: "Change"
        }
    }

    // One archived version in the app info pane.
    mod.widgets.MiniAppVersionRow = set_type_default() do #(MiniAppVersionRow::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill, height: Fit
        flow: Right
        spacing: 8
        align: Align{y: 0.5}
        padding: Inset{top: 4, bottom: 4, left: 10, right: 10}

        version_label := Label {
            width: Fill, height: Fit
            padding: 0, margin: 0
            draw_text +: {
                text_style: REGULAR_TEXT {font_size: 10.5},
                color: (COLOR_TEXT)
            }
        }
        version_restore_button := RobrixIconButton {
            padding: 6,
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            text: "Restore"
        }
    }

    // One AI provider in the providers pane.
    mod.widgets.MiniAppProviderRow = set_type_default() do #(MiniAppProviderRow::register_widget(vm)) {
        ..mod.widgets.RoundedView

        width: Fill, height: Fit
        flow: Right
        spacing: 8
        align: Align{y: 0.5}
        padding: Inset{top: 6, bottom: 6, left: 10, right: 10}

        provider_name := Label {
            width: 150, height: Fit
            padding: 0, margin: 0
            draw_text +: {
                text_style: theme.font_bold {font_size: 11},
                color: (COLOR_TEXT)
            }
        }
        provider_state := Label {
            width: Fill, height: Fit
            padding: 0, margin: 0
            draw_text +: {
                text_style: REGULAR_TEXT {font_size: 10},
                color: (MESSAGE_TEXT_COLOR)
            }
        }
        provider_action_button := RobrixIconButton {
            padding: 8,
            icon_walk: Walk{width: 0, height: 0, margin: 0}
            text: "Add"
        }
        provider_forget_button := RobrixNegativeIconButton {
            visible: false,
            padding: 8,
            draw_icon +: { svg: (ICON_TRASH) }
            icon_walk: Walk{width: 14, height: 14, margin: 0}
            text: ""
        }
    }

    mod.widgets.MiniAppsScreen = #(MiniAppsScreen::register_widget(vm)) {
        width: Fill, height: Fill
        flow: Overlay

        list_pane := ScrollYView {
            width: Fill, height: Fill
            flow: Down
            spacing: 10
            padding: 15

            TitleLabel { text: "Mini Apps" }
            Label {
                width: Fill, height: Fit
                flow: Flow.Right{wrap: true}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
                text: "Sandboxed Splash mini-apps that run inside Robrix. Each app runs in its own isolate with no access to anything you haven't granted it."
            }
            Label {
                width: Fill, height: Fit
                flow: Flow.Right{wrap: true}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10},
                    color: (COLOR_FG_ACCEPT_GREEN)
                }
                text: "Matrix access is read-only for now: mini-apps can never send messages or events to your rooms."
            }

            create_section := RoundedView {
                width: Fill, height: Fit
                flow: Down
                spacing: 8
                padding: 12
                show_bg: true,
                draw_bg +: {
                    color: #F6F8F9
                    border_radius: 4.0
                }

                SubsectionLabel { text: "Create or modify a mini-app with AI" }

                prompt_input := RobrixTextInput {
                    width: Fill, height: Fit
                    empty_text: "Describe the app you want, or a change to one you have…"
                }

                View {
                    width: Fill, height: Fit
                    flow: Right
                    spacing: 8
                    align: Align{y: 0.5}

                    generate_button := RobrixPositiveIconButton {
                        padding: 10,
                        draw_icon +: { svg: (ICON_SPARKLE) }
                        icon_walk: Walk{width: 16, height: 16, margin: Inset{right: 2}}
                        text: "Generate"
                    }
                    scope_label := Label {
                        width: Fit, height: Fit
                        padding: 0, margin: 0
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 9.5},
                            color: (MESSAGE_TEXT_COLOR)
                        }
                        text: "New apps are available account-wide."
                    }
                    Filler {}
                    providers_button := RobrixNeutralIconButton {
                        padding: 8,
                        icon_walk: Walk{width: 0, height: 0, margin: 0}
                        text: "AI Providers…"
                    }
                }

                console_section := View {
                    visible: false,
                    width: Fill, height: Fit
                    flow: Down
                    spacing: 6

                    View {
                        width: Fill, height: Fit
                        flow: Right
                        spacing: 8
                        align: Align{y: 0.5}
                        console_status := Label {
                            width: Fill, height: Fit
                            padding: 0, margin: 0
                            draw_text +: {
                                text_style: theme.font_bold {font_size: 10.5},
                                color: (COLOR_TEXT)
                            }
                        }
                        stop_button := RobrixNegativeIconButton {
                            padding: 8,
                            icon_walk: Walk{width: 0, height: 0, margin: 0}
                            text: "Stop"
                        }
                        retry_button := RobrixIconButton {
                            visible: false,
                            padding: 8,
                            icon_walk: Walk{width: 0, height: 0, margin: 0}
                            text: "Retry"
                        }
                        new_prompt_button := RobrixNeutralIconButton {
                            visible: false,
                            padding: 8,
                            icon_walk: Walk{width: 0, height: 0, margin: 0}
                            text: "New prompt"
                        }
                    }

                    console_list := PortalList {
                        width: Fill, height: 240
                        flow: Down
                        auto_tail: true

                        ConsoleLine := Label {
                            width: Fill, height: Fit
                            padding: Inset{top: 1, bottom: 1}
                            margin: 0
                            draw_text +: {
                                text_style: MESSAGE_TEXT_STYLE {font_size: 9.5},
                                color: (MESSAGE_TEXT_COLOR)
                            }
                        }
                    }
                }
            }

            SubsectionLabel { text: "Your mini-apps" }
            no_apps_label := Label {
                visible: false,
                width: Fill, height: Fit
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
                text: "No mini-apps yet. Create one above!"
            }
            apps_list := FlatList {
                width: Fill, height: Fit
                spacing: 4
                flow: Down

                mini_app_row := mod.widgets.MiniAppRow { }
            }

            SubsectionLabel { text: "Import" }
            View {
                width: Fill, height: Fit
                // no wrap: a wrapping Right flow can't lay out the Fill-width input
                flow: Right
                spacing: 8
                align: Align{y: 0.5}

                import_input := RobrixTextInput {
                    width: Fill { max: 500 }, height: Fit
                    empty_text: "Paste a .splashapp bundle or bare Splash source here…"
                }
                import_button := RobrixIconButton {
                    padding: 8,
                    draw_icon +: { svg: (ICON_IMPORT) }
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
                    text: "Install"
                }
            }
        }

        info_pane := ScrollYView {
            visible: false,
            width: Fill, height: Fill
            flow: Down
            spacing: 10
            padding: 15

            View {
                width: Fill, height: Fit
                flow: Right
                spacing: 10
                align: Align{y: 0.5}

                info_back_button := RobrixNeutralIconButton {
                    padding: 8,
                    draw_icon +: { svg: (ICON_JUMP) }
                    icon_walk: Walk{width: 14, height: 14, margin: 0}
                    text: "Back"
                }
                info_glyph := Label {
                    width: Fit, height: Fit
                    padding: 0, margin: 0
                    draw_text +: {
                        text_style: TITLE_TEXT {font_size: 22},
                        color: #000
                    }
                }
                View {
                    width: Fill, height: Fit
                    flow: Down
                    spacing: 2
                    info_name := TitleLabel { margin: 0 }
                    info_kind := Label {
                        width: Fill, height: Fit
                        padding: 0, margin: 0
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 10},
                            color: (MESSAGE_TEXT_COLOR)
                        }
                    }
                }
            }

            restricted_banner := RoundedView {
                visible: false,
                width: Fill, height: Fit
                flow: Right
                spacing: 8
                align: Align{y: 0.5}
                padding: 10
                show_bg: true,
                draw_bg +: {
                    color: (COLOR_BG_DANGER_RED)
                    border_radius: 4.0
                }
                Label {
                    width: Fill, height: Fit
                    flow: Flow.Right{wrap: true}
                    padding: 0, margin: 0
                    draw_text +: {
                        text_style: REGULAR_TEXT {font_size: 10.5},
                        color: (COLOR_FG_DANGER_RED)
                    }
                    text: "This app was stopped for flooding the host with requests. It won't run until you let it."
                }
                unrestrict_button := RobrixIconButton {
                    padding: 8,
                    icon_walk: Walk{width: 0, height: 0, margin: 0}
                    text: "Let it run again"
                }
            }

            View {
                width: Fill, height: Fit
                flow: Flow.Right{wrap: true}
                spacing: 8

                info_open_button := RobrixIconButton {
                    padding: 10,
                    icon_walk: Walk{width: 0, height: 0, margin: 0}
                    text: "Open"
                }
                info_source_button := RobrixNeutralIconButton {
                    padding: 10,
                    draw_icon +: { svg: (ICON_VIEW_SOURCE) }
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
                    text: "View Source"
                }
                info_export_button := RobrixNeutralIconButton {
                    padding: 10,
                    draw_icon +: { svg: (ICON_COPY) }
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
                    text: "Export"
                }
                info_modify_button := RobrixNeutralIconButton {
                    padding: 10,
                    draw_icon +: { svg: (ICON_EDIT) }
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
                    text: "Modify with AI"
                }
                info_force_stop_button := RobrixNegativeIconButton {
                    visible: false,
                    padding: 10,
                    draw_icon +: { svg: (ICON_FORBIDDEN) }
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
                    text: "Force Stop"
                }
                info_clear_data_button := RobrixNegativeIconButton {
                    padding: 10,
                    icon_walk: Walk{width: 0, height: 0, margin: 0}
                    text: "Clear Data"
                }
                info_uninstall_button := RobrixNegativeIconButton {
                    padding: 10,
                    draw_icon +: { svg: (ICON_TRASH) }
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 2}}
                    text: "Uninstall"
                }
            }

            info_storage_label := Label {
                width: Fill, height: Fit
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10},
                    color: (MESSAGE_TEXT_COLOR)
                }
            }

            SubsectionLabel { text: "Permissions" }
            perm_hint := Label {
                visible: false,
                width: Fill, height: Fit
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 9.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
                text: "This app is running: permission changes apply immediately, and changing network access restarts it."
            }
            no_perms_label := Label {
                visible: false,
                width: Fill, height: Fit
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
                text: "This app declares no permissions: it can only draw its own UI and use its private storage."
            }
            perms_list := FlatList {
                width: Fill, height: Fit
                spacing: 2
                flow: Down

                permission_row := mod.widgets.MiniAppPermissionRow { }
                capability_row := mod.widgets.MiniAppCapabilityRow { }
            }

            SubsectionLabel { text: "Version history" }
            no_versions_label := Label {
                visible: false,
                width: Fill, height: Fit
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
                text: "No archived versions. Every AI modification (and restore) archives the previous state first."
            }
            versions_list := FlatList {
                width: Fill, height: Fit
                spacing: 2
                flow: Down

                version_row := mod.widgets.MiniAppVersionRow { }
            }
        }

        providers_pane := ScrollYView {
            visible: false,
            width: Fill, height: Fill
            flow: Down
            spacing: 10
            padding: 15

            View {
                width: Fill, height: Fit
                flow: Right
                spacing: 10
                align: Align{y: 0.5}
                providers_back_button := RobrixNeutralIconButton {
                    padding: 8,
                    draw_icon +: { svg: (ICON_JUMP) }
                    icon_walk: Walk{width: 14, height: 14, margin: 0}
                    text: "Back"
                }
                TitleLabel { text: "AI Providers" }
            }

            providers_blocker := Label {
                visible: false,
                width: Fill, height: Fit
                flow: Flow.Right{wrap: true}
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: theme.font_bold {font_size: 11},
                    color: (COLOR_FG_DANGER_RED)
                }
            }

            key_entry_section := View {
                visible: false,
                width: Fill, height: Fit
                flow: Down
                spacing: 6

                key_entry_label := SubsectionLabel {}
                View {
                    width: Fill, height: Fit
                    flow: Right
                    spacing: 8
                    align: Align{y: 0.5}
                    key_input := RobrixTextInput {
                        width: Fill { max: 500 }, height: Fit
                        empty_text: "Paste the provider's API key…"
                        is_password: true
                    }
                    key_save_button := RobrixPositiveIconButton {
                        padding: 8,
                        icon_walk: Walk{width: 0, height: 0, margin: 0}
                        text: "Save"
                    }
                    key_cancel_button := RobrixNeutralIconButton {
                        padding: 8,
                        icon_walk: Walk{width: 0, height: 0, margin: 0}
                        text: "Cancel"
                    }
                }
            }

            providers_list := FlatList {
                width: Fill, height: Fit
                spacing: 2
                flow: Down

                provider_row := mod.widgets.MiniAppProviderRow { }
            }

            providers_note := Label {
                width: Fill, height: Fit
                flow: Flow.Right{wrap: true}
                padding: 0, margin: Inset{left: 6}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 9.5},
                    color: (MESSAGE_TEXT_COLOR)
                }
                text: "Keys are stored in octos's own config file; switching providers is a one-field edit. An Ollama server running locally is detected automatically and needs no key."
            }
        }

        source_pane := View {
            visible: false,
            width: Fill, height: Fill
            flow: Down
            padding: 15
            spacing: 8

            show_bg: true,
            draw_bg.color: (COLOR_PRIMARY)

            View {
                width: Fill, height: Fit
                flow: Right
                spacing: 10
                align: Align{y: 0.5}
                source_title := TitleLabel { margin: 0 }
                source_close_button := RobrixNeutralIconButton {
                    padding: 8,
                    draw_icon +: { svg: (ICON_CLOSE) }
                    icon_walk: Walk{width: 14, height: 14, margin: 0}
                    text: ""
                }
            }
            source_code_view := mod.widgets.LightCodeView {
                editor +: {
                    width: Fill, height: Fill
                }
            }
        }
    }
}

/// Actions emitted by the per-row widgets, applied by the screen.
#[derive(Clone, Debug, Default)]
pub enum MiniAppsScreenAction {
    OpenApp(MiniAppId),
    ShowInfo(MiniAppId),
    CyclePermission { app_id: MiniAppId, perm: Permission },
    CycleCapability { app_id: MiniAppId, cap_id: String },
    RestoreVersion { app_id: MiniAppId, stamp: String },
    ProviderAction(String),
    ProviderForget(String),
    #[default]
    None,
}

// -----------------------------------------------------------------------
// Row widgets
// -----------------------------------------------------------------------

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppRow {
    #[deref] view: View,
    #[rust] app_id: MiniAppId,
}

impl Widget for MiniAppRow {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        if let Event::Actions(actions) = event {
            if self.view.button(cx, ids!(row_open_button)).clicked(actions) {
                cx.action(MiniAppsScreenAction::OpenApp(self.app_id.clone()));
            } else if self.view.button(cx, ids!(row_info_button)).clicked(actions) {
                cx.action(MiniAppsScreenAction::ShowInfo(self.app_id.clone()));
            }
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppRow {
    fn populate(&mut self, cx: &mut Cx, app_id: &str, icon: &str, name: &str, detail: &str) {
        self.app_id = app_id.to_string();
        self.view.label(cx, ids!(row_glyph)).set_text(cx, icon);
        self.view.label(cx, ids!(row_name)).set_text(cx, name);
        self.view.label(cx, ids!(row_detail)).set_text(cx, detail);
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppPermissionRow {
    #[deref] view: View,
    #[rust] app_id: MiniAppId,
    #[rust] perm: Option<Permission>,
}

impl Widget for MiniAppPermissionRow {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        if let Event::Actions(actions) = event
            && self.view.button(cx, ids!(perm_change_button)).clicked(actions)
            && let Some(perm) = self.perm
        {
            cx.action(MiniAppsScreenAction::CyclePermission {
                app_id: self.app_id.clone(),
                perm,
            });
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppPermissionRow {
    fn populate(&mut self, cx: &mut Cx, app_id: &str, perm: Permission, effective: Effective) {
        self.app_id = app_id.to_string();
        self.perm = Some(perm);
        self.view.label(cx, ids!(perm_glyph)).set_text(cx, perm.glyph());
        self.view.label(cx, ids!(perm_title)).set_text(cx, perm.title());
        self.view.label(cx, ids!(perm_blurb)).set_text(cx, perm.blurb());
        let (state_text, color) = match effective {
            Effective::Granted => ("Allowed", crate::shared::styles::COLOR_FG_ACCEPT_GREEN),
            Effective::NeedsPrompt => ("Asks", crate::shared::styles::COLOR_ACTIVE_PRIMARY),
            Effective::Denied => ("Blocked", crate::shared::styles::COLOR_FG_DANGER_RED),
            Effective::Undeclared => ("Undeclared", crate::shared::styles::COLOR_FG_DISABLED),
        };
        let mut state_label = self.view.label(cx, ids!(perm_state));
        script_apply_eval!(cx, state_label, {
            text: #(state_text),
            draw_text +: { color: #(color) },
        });
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppCapabilityRow {
    #[deref] view: View,
    #[rust] app_id: MiniAppId,
    #[rust] cap_id: String,
}

impl Widget for MiniAppCapabilityRow {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        if let Event::Actions(actions) = event
            && self.view.button(cx, ids!(cap_change_button)).clicked(actions)
        {
            cx.action(MiniAppsScreenAction::CycleCapability {
                app_id: self.app_id.clone(),
                cap_id: self.cap_id.clone(),
            });
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppCapabilityRow {
    fn populate(
        &mut self,
        cx: &mut Cx,
        app_id: &str,
        cap: &a2app_core::capabilities::Capability,
        own: GrantState,
        effective: Effective,
    ) {
        self.app_id = app_id.to_string();
        self.cap_id = cap.id.to_string();
        self.view.label(cx, ids!(cap_title)).set_text(cx, cap.title);
        self.view.label(cx, ids!(cap_tags)).set_text(cx, &cap.tags());
        // An explicit answer reads as such; otherwise it follows the group.
        let (state_text, color) = match (own, effective) {
            (GrantState::Granted, _) => (String::from("Allowed"), crate::shared::styles::COLOR_FG_ACCEPT_GREEN),
            (GrantState::Denied, _) => (String::from("Blocked"), crate::shared::styles::COLOR_FG_DANGER_RED),
            (GrantState::Ask, Effective::Granted) => (String::from("Allowed · group"), crate::shared::styles::COLOR_FG_DISABLED),
            (GrantState::Ask, Effective::NeedsPrompt) => (String::from("Asks · group"), crate::shared::styles::COLOR_FG_DISABLED),
            (GrantState::Ask, _) => (String::from("Blocked · group"), crate::shared::styles::COLOR_FG_DISABLED),
        };
        let mut state_label = self.view.label(cx, ids!(cap_state));
        script_apply_eval!(cx, state_label, {
            text: #(state_text),
            draw_text +: { color: #(color) },
        });
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppVersionRow {
    #[deref] view: View,
    #[rust] app_id: MiniAppId,
    #[rust] stamp: String,
}

impl Widget for MiniAppVersionRow {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        if let Event::Actions(actions) = event
            && self.view.button(cx, ids!(version_restore_button)).clicked(actions)
        {
            cx.action(MiniAppsScreenAction::RestoreVersion {
                app_id: self.app_id.clone(),
                stamp: self.stamp.clone(),
            });
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppVersionRow {
    fn populate(&mut self, cx: &mut Cx, app_id: &str, version: &AppVersion) {
        self.app_id = app_id.to_string();
        self.stamp = version.stamp.clone();
        let note = if version.note.is_empty() {
            String::new()
        } else {
            format!(" · {}", version.note)
        };
        let when = a2app_core::versions::label_for(
            version.at_unix,
            crate::a2app::runtime::utc_offset_secs(),
        );
        self.view.label(cx, ids!(version_label)).set_text(cx, &format!("{when}{note}"));
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppProviderRow {
    #[deref] view: View,
    #[rust] provider_id: String,
}

impl Widget for MiniAppProviderRow {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        if let Event::Actions(actions) = event {
            if self.view.button(cx, ids!(provider_action_button)).clicked(actions) {
                cx.action(MiniAppsScreenAction::ProviderAction(self.provider_id.clone()));
            } else if self.view.button(cx, ids!(provider_forget_button)).clicked(actions) {
                cx.action(MiniAppsScreenAction::ProviderForget(self.provider_id.clone()));
            }
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl MiniAppProviderRow {
    fn populate(&mut self, cx: &mut Cx, id: &str, label: &str, state: &str, action: &str, forgettable: bool) {
        self.provider_id = id.to_string();
        self.view.label(cx, ids!(provider_name)).set_text(cx, label);
        self.view.label(cx, ids!(provider_state)).set_text(cx, state);
        self.view.button(cx, ids!(provider_action_button)).set_text(cx, action);
        self.view.button(cx, ids!(provider_forget_button)).set_visible(cx, forgettable);
    }
}

// -----------------------------------------------------------------------
// The screen itself
// -----------------------------------------------------------------------

/// Which of the screen's panes is showing.
#[derive(Default, PartialEq)]
enum Pane {
    #[default]
    List,
    Info,
    Providers,
    Source,
}

#[derive(Script, ScriptHook, Widget)]
pub struct MiniAppsScreen {
    #[deref] view: View,
    #[rust] pane: Pane,
    /// The app shown in the info (or source) pane.
    #[rust] info_app: Option<MiniAppId>,
    /// Versions of the info app, loaded once when the pane opens.
    #[rust] versions: Vec<AppVersion>,
    /// The provider awaiting a pasted key, if any.
    #[rust] key_entry: Option<String>,
}

impl Widget for MiniAppsScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);

        // Console output streams in on Signal events; keep it painting.
        if let Event::Signal = event
            && with_a2app(|state| state.console.active).unwrap_or(false)
        {
            self.view.redraw(cx);
        }

        let Event::Actions(actions) = event else { return };

        for action in actions {
            // A /miniapp generation lands on this screen; make sure the
            // console (list pane) is showing, not a leftover info pane.
            if let Some(A2AppOp::StartGeneration { .. }) = action.downcast_ref::<A2AppOp>()
                && self.pane != Pane::List
            {
                self.set_pane(cx, Pane::List);
            }
            match action.downcast_ref::<MiniAppsScreenAction>() {
                Some(MiniAppsScreenAction::OpenApp(app_id)) => {
                    cx.action(A2AppOp::OpenApp { app_id: app_id.clone(), room_id: None, in_room_pane: false });
                    continue;
                }
                Some(MiniAppsScreenAction::ShowInfo(app_id)) => {
                    self.show_info(cx, app_id.clone());
                    continue;
                }
                Some(MiniAppsScreenAction::CyclePermission { app_id, perm }) => {
                    self.cycle_permission(cx, app_id, *perm);
                    continue;
                }
                Some(MiniAppsScreenAction::CycleCapability { app_id, cap_id }) => {
                    // Follows group -> Blocked -> Allowed -> follows group.
                    let current = with_a2app(|state| state.permissions.capability_state(app_id, cap_id))
                        .unwrap_or_default();
                    let next = match current {
                        GrantState::Ask => GrantState::Denied,
                        GrantState::Denied => GrantState::Granted,
                        GrantState::Granted => GrantState::Ask,
                    };
                    cx.action(A2AppOp::SetCapability {
                        app_id: app_id.clone(),
                        cap_id: cap_id.clone(),
                        state: next,
                    });
                    continue;
                }
                Some(MiniAppsScreenAction::RestoreVersion { app_id, stamp }) => {
                    cx.action(A2AppOp::RestoreVersion {
                        app_id: app_id.clone(),
                        stamp: stamp.clone(),
                    });
                    // Reload the version list on the next info draw.
                    self.versions.clear();
                    continue;
                }
                Some(MiniAppsScreenAction::ProviderAction(id)) => {
                    self.provider_action(cx, id.clone());
                    continue;
                }
                Some(MiniAppsScreenAction::ProviderForget(id)) => {
                    match a2app_agent::providers::forget(id) {
                        Ok(()) => enqueue_popup_notification("Forgot that provider's key.", PopupKind::Success, Some(3.0)),
                        Err(e) => enqueue_popup_notification(e, PopupKind::Error, Some(5.0)),
                    }
                    self.view.redraw(cx);
                    continue;
                }
                Some(MiniAppsScreenAction::None) | None => {}
            }
        }

        // ----- list pane -----
        if self.view.button(cx, ids!(generate_button)).clicked(actions) {
            let request = self.view.text_input(cx, ids!(prompt_input)).text();
            let request = request.trim().to_string();
            if request.is_empty() {
                enqueue_popup_notification("Describe the app you want first.", PopupKind::Warning, Some(3.0));
            } else {
                cx.action(A2AppOp::StartGeneration { request, room_id: None });
                self.view.redraw(cx);
            }
        }
        if self.view.button(cx, ids!(providers_button)).clicked(actions) {
            self.set_pane(cx, Pane::Providers);
        }
        if self.view.button(cx, ids!(stop_button)).clicked(actions) {
            cx.action(A2AppOp::CancelGeneration);
        }
        if self.view.button(cx, ids!(retry_button)).clicked(actions) {
            cx.action(A2AppOp::RetryGeneration);
        }
        if self.view.button(cx, ids!(new_prompt_button)).clicked(actions) {
            self.view.text_input(cx, ids!(prompt_input)).set_text(cx, "");
            cx.action(A2AppOp::NewPrompt);
        }
        if self.view.button(cx, ids!(import_button)).clicked(actions) {
            let text = self.view.text_input(cx, ids!(import_input)).text();
            if text.trim().is_empty() {
                enqueue_popup_notification("Paste a bundle to import first.", PopupKind::Warning, Some(3.0));
            } else {
                self.view.text_input(cx, ids!(import_input)).set_text(cx, "");
                cx.action(A2AppOp::ImportText(text));
            }
        }

        // ----- info pane -----
        if self.view.button(cx, ids!(info_back_button)).clicked(actions) {
            self.set_pane(cx, Pane::List);
        }
        if let Some(app_id) = self.info_app.clone() {
            if self.view.button(cx, ids!(info_open_button)).clicked(actions) {
                cx.action(A2AppOp::OpenApp { app_id: app_id.clone(), room_id: None, in_room_pane: false });
            }
            if self.view.button(cx, ids!(info_source_button)).clicked(actions) {
                self.show_source(cx, &app_id);
            }
            if self.view.button(cx, ids!(info_export_button)).clicked(actions) {
                cx.action(A2AppOp::Export(app_id.clone()));
            }
            if self.view.button(cx, ids!(info_modify_button)).clicked(actions) {
                // Prefill the composer with a modify hint and go back to it.
                let name = with_a2app(|state| {
                    state.registry.get(&app_id).map(|a| a.name.clone())
                }).flatten().unwrap_or_else(|| app_id.clone());
                self.view.text_input(cx, ids!(prompt_input))
                    .set_text(cx, &format!("Change the {name} app: "));
                self.set_pane(cx, Pane::List);
            }
            if self.view.button(cx, ids!(info_force_stop_button)).clicked(actions) {
                cx.action(A2AppOp::ForceStop(app_id.clone()));
            }
            if self.view.button(cx, ids!(info_clear_data_button)).clicked(actions) {
                cx.action(A2AppOp::ClearData(app_id.clone()));
            }
            if self.view.button(cx, ids!(info_uninstall_button)).clicked(actions) {
                cx.action(A2AppOp::Uninstall(app_id.clone()));
                self.set_pane(cx, Pane::List);
            }
            if self.view.button(cx, ids!(unrestrict_button)).clicked(actions) {
                cx.action(A2AppOp::Unrestrict(app_id.clone()));
            }
        }

        // ----- providers pane -----
        if self.view.button(cx, ids!(providers_back_button)).clicked(actions) {
            self.set_pane(cx, Pane::List);
        }
        if self.view.button(cx, ids!(key_save_button)).clicked(actions)
            && let Some(provider) = self.key_entry.clone()
        {
            let key = self.view.text_input(cx, ids!(key_input)).text();
            let key = key.trim().to_string();
            if key.is_empty() {
                enqueue_popup_notification("Paste the key first.", PopupKind::Warning, Some(3.0));
            } else {
                match a2app_agent::providers::save_key(&provider, &key) {
                    Ok(()) => {
                        enqueue_popup_notification("Provider key saved.", PopupKind::Success, Some(3.0));
                        self.close_key_entry(cx);
                    }
                    Err(e) => enqueue_popup_notification(e, PopupKind::Error, Some(5.0)),
                }
            }
        }
        if self.view.button(cx, ids!(key_cancel_button)).clicked(actions) {
            self.close_key_entry(cx);
        }

        // ----- source pane -----
        if self.view.button(cx, ids!(source_close_button)).clicked(actions) {
            self.set_pane(cx, Pane::Info);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.populate_before_draw(cx);

        // Resolved before the draw loop: once a FlatList is mutably borrowed
        // below, a widget query for it would fail and return a zero uid.
        let perms_list_uid = self.view.widget(cx, ids!(perms_list)).widget_uid();

        while let Some(subview) = self.view.draw_walk(cx, scope, walk).step() {
            let uid = subview.widget_uid();
            if let Some(mut list) = subview.as_flat_list().borrow_mut() {
                self.draw_flat_list(cx, uid, perms_list_uid, &mut list);
                continue;
            }
            if let Some(mut list) = subview.as_portal_list().borrow_mut() {
                self.draw_console_list(cx, &mut list);
            }
        }
        DrawStep::done()
    }
}

impl MiniAppsScreen {
    fn set_pane(&mut self, cx: &mut Cx, pane: Pane) {
        self.pane = pane;
        let show = |p: Pane| self.pane == p;
        self.view.widget(cx, ids!(list_pane)).set_visible(cx, show(Pane::List));
        self.view.widget(cx, ids!(info_pane)).set_visible(cx, show(Pane::Info));
        self.view.widget(cx, ids!(providers_pane)).set_visible(cx, show(Pane::Providers));
        self.view.widget(cx, ids!(source_pane)).set_visible(cx, show(Pane::Source));
        self.view.redraw(cx);
    }

    fn show_info(&mut self, cx: &mut Cx, app_id: MiniAppId) {
        self.versions = persistence::list_versions(&app_id);
        self.info_app = Some(app_id);
        self.set_pane(cx, Pane::Info);
    }

    fn show_source(&mut self, cx: &mut Cx, app_id: &str) {
        let Some(Some((name, source))) = with_a2app(|state| {
            state.registry.get(app_id).map(|a| (a.name.clone(), a.source.clone()))
        }) else { return };
        self.view.label(cx, ids!(source_title)).set_text(cx, &format!("{name} — Splash source"));
        self.view.code_view(cx, ids!(source_code_view)).set_text(cx, &source);
        self.set_pane(cx, Pane::Source);
    }

    /// Cycles a grant Allowed -> Asks -> Blocked -> Allowed. Normal-tier
    /// permissions have no meaningful Ask state, so they skip it.
    fn cycle_permission(&mut self, cx: &mut Cx, app_id: &str, perm: Permission) {
        let current = with_a2app(|state| {
            state.registry.get(app_id)
                .map(|m| state.permissions.effective(m, perm))
        }).flatten();
        let next = match current {
            Some(Effective::Granted) => {
                if perm.tier() == a2app_core::permissions::Tier::Runtime {
                    GrantState::Ask
                } else {
                    GrantState::Denied
                }
            }
            Some(Effective::NeedsPrompt) => GrantState::Denied,
            Some(Effective::Denied) => GrantState::Granted,
            Some(Effective::Undeclared) | None => return,
        };
        cx.action(A2AppOp::SetPermission {
            app_id: app_id.to_string(),
            perm,
            state: next,
        });
    }

    fn provider_action(&mut self, cx: &mut Cx, provider_id: String) {
        // An inactive configured provider is switched to; anything else
        // ("Add" on a fresh one, "Replace key" on the active one) opens
        // the key-entry field.
        let known = a2app_agent::providers::list()
            .into_iter()
            .find(|p| p.id == provider_id);
        match known {
            Some(p) if !p.active => {
                match a2app_agent::providers::set_active(&provider_id) {
                    Ok(()) => enqueue_popup_notification(
                        format!("Now using {provider_id}."), PopupKind::Success, Some(3.0)),
                    Err(e) => enqueue_popup_notification(e, PopupKind::Error, Some(5.0)),
                }
                self.view.redraw(cx);
            }
            _ => {
                self.key_entry = Some(provider_id.clone());
                self.view.label(cx, ids!(key_entry_label))
                    .set_text(cx, &format!("API key for {provider_id}"));
                self.view.widget(cx, ids!(key_entry_section)).set_visible(cx, true);
                self.view.text_input(cx, ids!(key_input)).set_text(cx, "");
                self.view.redraw(cx);
            }
        }
    }

    fn close_key_entry(&mut self, cx: &mut Cx) {
        self.key_entry = None;
        self.view.text_input(cx, ids!(key_input)).set_text(cx, "");
        self.view.widget(cx, ids!(key_entry_section)).set_visible(cx, false);
        self.view.redraw(cx);
    }

    /// Refreshes all code-set labels/visibility from the a2app state.
    fn populate_before_draw(&mut self, cx: &mut Cx2d) {
        match self.pane {
            Pane::List => {
                let (any_apps, console) = with_a2app(|state| {
                    (
                        state.registry.iter().next().is_some(),
                        (state.console.active, state.console.status.clone(),
                         state.generation.is_some(), state.failed_request.is_some()),
                    )
                }).unwrap_or((false, (false, String::new(), false, false)));
                self.view.widget(cx, ids!(no_apps_label)).set_visible(cx, !any_apps);
                let (active, status, running, can_retry) = console;
                self.view.widget(cx, ids!(console_section)).set_visible(cx, active);
                if active {
                    self.view.label(cx, ids!(console_status)).set_text(cx, &status);
                    self.view.widget(cx, ids!(stop_button)).set_visible(cx, running);
                    self.view.widget(cx, ids!(retry_button)).set_visible(cx, !running && can_retry);
                    self.view.widget(cx, ids!(new_prompt_button)).set_visible(cx, !running);
                }
            }
            Pane::Info => {
                let Some(app_id) = self.info_app.clone() else { return };
                let info = with_a2app(|state| {
                    state.registry.get(&app_id).map(|m| (
                        m.icon.clone(),
                        m.name.clone(),
                        m.builtin,
                        m.scope.clone(),
                        state.permissions.is_restricted(&app_id),
                    ))
                }).flatten();
                let Some((icon, name, builtin, scope, restricted)) = info else { return };
                self.view.label(cx, ids!(info_glyph)).set_text(cx, &icon);
                self.view.label(cx, ids!(info_name)).set_text(cx, &name);
                let kind = match (builtin, &scope) {
                    (true, _) => String::from("Built-in mini-app · available account-wide"),
                    (false, A2AppScope::Account) => String::from("Your mini-app · available account-wide"),
                    (false, A2AppScope::Room { room_id }) => format!("Your mini-app · scoped to room {room_id}"),
                };
                self.view.label(cx, ids!(info_kind)).set_text(cx, &kind);
                self.view.widget(cx, ids!(restricted_banner)).set_visible(cx, restricted);
                let running = crate::a2app::runtime::with_a2app(|state| {
                    state.is_running(&app_id)
                }).unwrap_or(false);
                self.view.widget(cx, ids!(info_force_stop_button)).set_visible(cx, running);
                self.view.widget(cx, ids!(perm_hint)).set_visible(cx, running);
                self.view.widget(cx, ids!(info_uninstall_button)).set_visible(cx, !builtin);
                let bytes = persistence::app_data_bytes(&app_id);
                self.view.label(cx, ids!(info_storage_label))
                    .set_text(cx, &format!("Saved data: {} bytes in this app's private storage.", bytes));
                let declares_any = with_a2app(|state| {
                    state.registry.get(&app_id).is_some_and(|m| !m.permissions.is_empty())
                }).unwrap_or(false);
                self.view.widget(cx, ids!(no_perms_label)).set_visible(cx, !declares_any);
                self.view.widget(cx, ids!(no_versions_label)).set_visible(cx, self.versions.is_empty());
            }
            Pane::Providers => {
                let blocker = a2app_agent::blocker();
                self.view.widget(cx, ids!(providers_blocker)).set_visible(cx, blocker.is_some());
                if let Some(blocker) = blocker {
                    self.view.label(cx, ids!(providers_blocker)).set_text(cx, &blocker.headline());
                }
            }
            Pane::Source => {}
        }
    }

    fn draw_flat_list(&mut self, cx: &mut Cx2d, uid: WidgetUid, perms_list_uid: WidgetUid, list: &mut FlatList) {
        // Which list this is depends on which pane is visible; hidden panes
        // don't draw, so only one of these runs per draw pass.
        match self.pane {
            Pane::List => {
                // (id, icon, name, detail) only; cloning whole manifests here
                // would copy every app's source per draw.
                let rows: Vec<(MiniAppId, String, String, String)> = with_a2app(|state| {
                    state.registry.iter().map(|m| {
                        let running = state.is_running(&m.id);
                        let mut detail = match (&m.scope, m.builtin) {
                            (_, true) => String::from("Built-in"),
                            (A2AppScope::Account, false) => String::from("Account-wide"),
                            (A2AppScope::Room { room_id }, false) => format!("Room: {room_id}"),
                        };
                        if state.permissions.is_restricted(&m.id) {
                            detail.push_str(" · stopped for abuse");
                        } else if running {
                            detail.push_str(" · running");
                        }
                        (m.id.clone(), m.icon.clone(), m.name.clone(), detail)
                    }).collect()
                }).unwrap_or_default();
                for (app_id, icon, name, detail) in &rows {
                    let item_live_id = LiveId::from_str(app_id);
                    let Some(item) = list.item(cx, item_live_id, id!(mini_app_row)) else { continue };
                    if let Some(mut row) = item.borrow_mut::<MiniAppRow>() {
                        row.populate(cx, app_id, icon, name, detail);
                    }
                    item.draw_all(cx, &mut Scope::empty());
                }
            }
            Pane::Info => {
                let Some(app_id) = self.info_app.clone() else { return };
                // The perms and versions FlatLists both land here; tell them
                // apart by widget identity.
                if uid == perms_list_uid {
                    let rows: Vec<(Permission, Effective)> = with_a2app(|state| {
                        state.registry.get(&app_id).map(|m| {
                            m.permissions.iter()
                                .filter_map(|p| Permission::from_str(p))
                                .map(|p| (p, state.permissions.effective(m, p)))
                                .collect()
                        }).unwrap_or_default()
                    }).unwrap_or_default();
                    for (perm, effective) in rows {
                        let item_live_id = LiveId::from_str(perm.as_str());
                        let Some(item) = list.item(cx, item_live_id, id!(permission_row)) else { continue };
                        if let Some(mut row) = item.borrow_mut::<MiniAppPermissionRow>() {
                            row.populate(cx, &app_id, perm, effective);
                        }
                        item.draw_all(cx, &mut Scope::empty());

                        // The single abilities this group unlocks, each with
                        // its own override.
                        let caps: Vec<(&'static a2app_core::capabilities::Capability, GrantState, Effective)> =
                            with_a2app(|state| {
                                state.registry.get(&app_id).map(|m| {
                                    a2app_core::capabilities::in_group(perm)
                                        .filter(|c| c.is_available() && m.declares_capability(c))
                                        .map(|c| (
                                            c,
                                            state.permissions.capability_state(&app_id, c.id),
                                            state.permissions.effective_capability(m, c),
                                        ))
                                        .collect()
                                }).unwrap_or_default()
                            }).unwrap_or_default();
                        for (cap, own, cap_effective) in caps {
                            let cap_item_id = LiveId::from_str(cap.id);
                            let Some(item) = list.item(cx, cap_item_id, id!(capability_row)) else { continue };
                            if let Some(mut row) = item.borrow_mut::<MiniAppCapabilityRow>() {
                                row.populate(cx, &app_id, cap, own, cap_effective);
                            }
                            item.draw_all(cx, &mut Scope::empty());
                        }
                    }
                } else {
                    for version in self.versions.clone() {
                        let item_live_id = LiveId::from_str(&version.stamp);
                        let Some(item) = list.item(cx, item_live_id, id!(version_row)) else { continue };
                        if let Some(mut row) = item.borrow_mut::<MiniAppVersionRow>() {
                            row.populate(cx, &app_id, &version);
                        }
                        item.draw_all(cx, &mut Scope::empty());
                    }
                }
            }
            Pane::Providers => {
                let configured = a2app_agent::providers::list();
                let draw_row = |cx: &mut Cx2d, list: &mut FlatList, id: &str, label: &str,
                                    state: &str, action: &str, forgettable: bool| {
                    let item_live_id = LiveId::from_str(id);
                    let Some(item) = list.item(cx, item_live_id, id!(provider_row)) else { return };
                    if let Some(mut row) = item.borrow_mut::<MiniAppProviderRow>() {
                        row.populate(cx, id, label, state, action, forgettable);
                    }
                    item.draw_all(cx, &mut Scope::empty());
                };
                // The full catalog first, each row showing its own state...
                for spec in a2app_agent::providers::CATALOG {
                    let known = configured.iter().find(|p| p.id == spec.id);
                    let (state, action, forgettable) = match known {
                        Some(p) if p.active && p.editable() => (p.detail(), "Replace key", true),
                        Some(p) if p.active => (p.detail(), "", false),
                        Some(p) => (p.detail(), "Use", p.editable()),
                        None => (String::from("Not set up"), "Add", false),
                    };
                    draw_row(cx, list, spec.id, spec.label, &state, action, forgettable);
                }
                // ...then anything configured outside the catalog (a local
                // Ollama, or a ROBRIX_AGENT_CMD override).
                for p in configured.iter().filter(|p| !a2app_agent::providers::CATALOG.iter().any(|s| s.id == p.id)) {
                    let action = if !p.active && !p.external() { "Use" } else { "" };
                    draw_row(cx, list, &p.id, &p.label, &p.detail(), action, false);
                }
            }
            Pane::Source => {}
        }
    }

    fn draw_console_list(&mut self, cx: &mut Cx2d, list: &mut PortalList) {
        let lines: Vec<String> = with_a2app(|state| state.console.lines.clone()).unwrap_or_default();
        list.set_item_range(cx, 0, lines.len());
        while let Some(item_id) = list.next_visible_item(cx) {
            let Some(line) = lines.get(item_id) else { continue };
            let item = list.item(cx, item_id, id!(ConsoleLine));
            item.set_text(cx, line);
            item.draw_all(cx, &mut Scope::empty());
        }
    }
}
