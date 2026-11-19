//! Community invite join (`Overlay::JoinGuild`) and leave confirmation
//! (`Overlay::LeaveGuild`) modals.

use fluxer_client_core::state::Snowflake;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, FocusHandle, Focusable as _, FontWeight, SharedString, Subscription,
    Window, div, prelude::*,
};

use crate::backend::{InvitePreview, extract_invite_code};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};

pub const JOIN_CONTEXT: &str = "JoinGuildModal";
pub const LEAVE_CONTEXT: &str = "LeaveGuildModal";

pub struct JoinGuildModal {
    pub input: Entity<InputState>,
    pub loading: bool,
    pub preview: Option<InvitePreview>,
    pub error: Option<String>,
    pub _subscription: Subscription,
}

pub struct LeaveGuildModal {
    pub guild_id: Snowflake,
    pub guild_name: SharedString,
    pub focus: FocusHandle,
}

impl ClientView {
    pub(crate) fn current_guild_is_owner(&self) -> bool {
        let crate::view_model::Place::Guild(guild_id) = self.place else {
            return false;
        };
        let state = self.backend.state();
        let me = state.current_user_id();
        me.is_some() && state.guilds.get(&guild_id).and_then(|g| g.owner_id) == me
    }

    pub(crate) fn open_join_guild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_join_guild_initial(None, window, cx);
    }

    /// Opens the Join Guild confirmation modal pre-filled with an invite code
    /// and immediately fetches its preview (never auto-joining without consent).
    pub(crate) fn open_join_guild_with_code(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_join_guild_initial(Some(code), window, cx);
        self.preview_join_invite(cx);
    }

    fn open_join_guild_initial(&mut self, initial: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.pending_accept_code = None;
        let initial_value = initial.map(str::to_owned);
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(l10n::tr("Enter an invite link or code"));
            if let Some(code) = &initial_value {
                state.set_value(code, window, cx);
            }
            state
        });
        let subscription = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::PressEnter { shift: false, .. } => this.preview_or_join_invite(cx),
            InputEvent::Change => {
                this.pending_accept_code = None;
                if let Overlay::JoinGuild(modal) = &mut this.overlay {
                    modal.loading = false;
                    modal.preview = None;
                    modal.error = None;
                    cx.notify();
                }
            }
            _ => {}
        });
        self.overlay = Overlay::JoinGuild(JoinGuildModal {
            input: input.clone(),
            loading: false,
            preview: None,
            error: None,
            _subscription: subscription,
        });
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn preview_or_join_invite(&mut self, cx: &mut Context<Self>) {
        let should_join = matches!(
            &self.overlay,
            Overlay::JoinGuild(modal) if modal.preview.is_some()
        );
        if should_join {
            self.confirm_join_invite(cx);
        } else {
            self.preview_join_invite(cx);
        }
    }

    pub(crate) fn preview_join_invite(&mut self, cx: &mut Context<Self>) {
        if matches!(&self.overlay, Overlay::JoinGuild(modal) if modal.loading) {
            return;
        }
        let Some(code) = self.read_invite_code(cx) else { return };
        if let Overlay::JoinGuild(modal) = &mut self.overlay {
            modal.loading = true;
            modal.error = None;
        }
        self.backend.fetch_invite(code);
        cx.notify();
    }

    pub(crate) fn confirm_join_invite(&mut self, cx: &mut Context<Self>) {
        if matches!(&self.overlay, Overlay::JoinGuild(modal) if modal.loading) {
            return;
        }
        let Some(code) = self.read_invite_code(cx) else { return };
        if let Overlay::JoinGuild(modal) = &mut self.overlay {
            modal.loading = true;
            modal.error = None;
        }
        self.pending_accept_code = Some(code.clone());
        self.backend.accept_invite(code);
        cx.notify();
    }

    fn read_invite_code(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let raw = {
            let Overlay::JoinGuild(modal) = &self.overlay else { return None };
            modal.input.read(cx).value().to_string()
        };
        match extract_invite_code(&raw) {
            Some(code) => Some(code),
            None => {
                if let Overlay::JoinGuild(modal) = &mut self.overlay {
                    modal.loading = false;
                    modal.error = Some(l10n::tr("Enter a valid invite link or code."));
                    cx.notify();
                }
                None
            }
        }
    }

    pub(crate) fn open_leave_guild(&mut self, guild_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        let (is_owner, guild_name) = {
            let state = self.backend.state();
            let Some(guild) = state.guilds.get(&guild_id) else { return };
            (
                state.current_user_id().is_some() && guild.owner_id == state.current_user_id(),
                SharedString::from(guild.name.clone()),
            )
        };
        if is_owner {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus = cx.focus_handle();
        self.overlay = Overlay::LeaveGuild(LeaveGuildModal { guild_id, guild_name, focus: focus.clone() });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn confirm_leave_guild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::LeaveGuild(modal) = &self.overlay else { return };
        let guild_id = modal.guild_id;
        self.close_overlay(window, cx);
        self.backend.leave_guild(guild_id);
    }
}

pub fn render_join(modal: &JoinGuildModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let preview_box = modal.preview.as_ref().map(|p| {
        let counts = match (p.member_count, p.presence_count) {
            (Some(members), Some(online)) => format!("{online} {} · {members} {}", l10n::tr("Online"), l10n::tr("Members")),
            (Some(members), None) => format!("{members} {}", l10n::tr("Members")),
            _ => String::new(),
        };
        div()
            .p_3()
            .rounded(gpui_kit::px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(p.guild_name.clone()))
            .when_some(p.channel_name.clone(), |d, ch| {
                d.child(div().text_xs().text_color(theme.muted_foreground).child(format!("#{ch}")))
            })
            .when(!counts.is_empty(), |d| {
                d.child(div().text_xs().text_color(theme.muted_foreground).child(counts))
            })
    });

    let content = div()
        .id("join-guild-modal")
        .key_context(JOIN_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Join a community")))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Enter an invite below to join an existing community.")),
        )
        .child(Input::new(&modal.input))
        .children(preview_box)
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("join-guild-cancel")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    Button::new("join-guild-preview")
                        .ghost()
                        .small()
                        .label(l10n::tr("Preview"))
                        .on_click(cx.listener(|this, _, _, cx| this.preview_join_invite(cx))),
                )
                .child(
                    div().debug_selector(|| "join-guild-submit".to_owned()).child(
                        Button::new("join-guild-submit")
                            .primary()
                            .small()
                            .label(if modal.loading { l10n::tr("Loading…") } else { l10n::tr("Join") })
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_join_invite(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_leave(modal: &LeaveGuildModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let prompt = l10n::tr_args(
        "Are you sure you want to leave {guildName}?",
        &[("guildName", modal.guild_name.as_ref())],
    );
    let content = div()
        .id("leave-guild-modal")
        .key_context(LEAVE_CONTEXT)
        .track_focus(&modal.focus)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Leave community")))
        .child(div().text_sm().text_color(theme.muted_foreground).child(prompt))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("leave-guild-cancel")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div().debug_selector(|| "leave-guild-confirm".to_owned()).child(
                        Button::new("leave-guild-confirm")
                            .danger()
                            .small()
                            .label(l10n::tr("Leave community"))
                            .on_click(cx.listener(|this, _, window, cx| this.confirm_leave_guild(window, cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
