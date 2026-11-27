//! Invite members modal (`InviteModal.tsx`, `InviteUtils.ts`).

use std::collections::HashSet;

use fluxer_client_core::rest::CreateInviteRequest;
use fluxer_client_core::state::{ChannelKind, ClientState, Snowflake};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, Focusable as _, FontWeight, SharedString,
    Subscription, Window, div, prelude::*, px,
};

use crate::backend::CreatedInvite;
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};
use crate::view_model::channel_name;

pub const CONTEXT: &str = "InviteMembersModal";

const DEFAULT_MAX_AGE: u32 = 604_800; // 7 days
const DEFAULT_MAX_USES: u32 = 0; // unlimited

const AGE_PRESETS: &[(u32, &str)] = &[
    (1800, "30m"),
    (3600, "1h"),
    (21_600, "6h"),
    (43_200, "12h"),
    (86_400, "1d"),
    (604_800, "7d"),
    (0, "Never"),
];

const USES_PRESETS: &[(u32, &str)] = &[
    (0, "No limit"),
    (1, "1"),
    (5, "5"),
    (10, "10"),
    (25, "25"),
    (50, "50"),
    (100, "100"),
];

pub struct InviteMembersModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub channel_id: Snowflake,
    pub guild_name: SharedString,
    pub channel_name: SharedString,
    pub query: Entity<InputState>,
    pub invite: Option<CreatedInvite>,
    pub loading: bool,
    pub copied: bool,
    pub error: Option<String>,
    pub sent_channels: HashSet<Snowflake>,
    pub editing_settings: bool,
    pub max_age: u32,
    pub max_uses: u32,
    pub temporary: bool,
    pub _subscriptions: Vec<Subscription>,
}

fn is_invitable_channel(state: &ClientState, guild_id: Snowflake, channel_id: Snowflake) -> bool {
    state.channels.get(&channel_id).is_some_and(|c| {
        c.guild_id == Some(guild_id)
            && !c.kind.is_category()
            && !c.kind.is_private()
            && !matches!(
                c.kind,
                ChannelKind::AnnouncementThread | ChannelKind::PublicThread | ChannelKind::PrivateThread
            )
            && state.can_create_invite(c.id)
    })
}

/// Resolves the channel to target for a guild invite, preferring `preferred` when
/// invitable, else sorting text/announcement channels first, then `(position, id)`.
pub fn invitable_channel_id(
    state: &ClientState,
    guild_id: Snowflake,
    preferred: Option<Snowflake>,
) -> Option<Snowflake> {
    let guild = state.guilds.get(&guild_id)?;
    if guild.invites_disabled {
        return None;
    }
    if let Some(cid) = preferred
        && is_invitable_channel(state, guild_id, cid)
    {
        return Some(cid);
    }
    let mut candidates: Vec<(bool, i32, Snowflake)> = state
        .channels
        .values()
        .filter(|c| is_invitable_channel(state, guild_id, c.id))
        .map(|c| {
            let not_text_first = !matches!(c.kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement);
            (not_text_first, c.position, c.id)
        })
        .collect();
    candidates.sort_unstable();
    candidates.first().map(|(_, _, id)| *id)
}

pub fn can_invite_in_guild(state: &ClientState, guild_id: Snowflake, preferred: Option<Snowflake>) -> bool {
    invitable_channel_id(state, guild_id, preferred).is_some()
}

fn age_summary(max_age: u32) -> &'static str {
    match max_age {
        1800 => "30 minutes",
        3600 => "1 hour",
        21_600 => "6 hours",
        43_200 => "12 hours",
        86_400 => "1 day",
        604_800 => "7 days",
        0 => "never",
        _ => "custom duration",
    }
}

impl ClientView {
    pub(crate) fn open_invite_members(&mut self, guild_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        self.open_invite_members_for(guild_id, self.selected_channel, window, cx);
    }

    /// Opens the invite modal targeting `preferred` when it is invitable
    /// (channel row `UserPlusIcon`: `<InviteModal channelId={channel.id} />`).
    pub(crate) fn open_invite_members_for(
        &mut self,
        guild_id: Snowflake,
        preferred: Option<Snowflake>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (channel_id, guild_name, ch_name) = {
            let state = self.backend.state();
            let Some(cid) = invitable_channel_id(&state, guild_id, preferred) else {
                return;
            };
            let guild_name: SharedString = state
                .guilds
                .get(&guild_id)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| "Community".to_owned())
                .into();
            let ch_name: SharedString = state
                .channels
                .get(&cid)
                .map(channel_name)
                .unwrap_or_else(|| "general".to_owned())
                .into();
            (cid, guild_name, ch_name)
        };

        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search for friends")));
        let sub_q = cx.subscribe_in(&query, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) && matches!(this.overlay, Overlay::InviteMembers(_)) {
                cx.notify();
            }
        });
        let seq = self.next_invite_seq();
        let focus = query.read(cx).focus_handle(cx);
        self.overlay = Overlay::InviteMembers(InviteMembersModal {
            seq,
            guild_id,
            channel_id,
            guild_name,
            channel_name: ch_name,
            query,
            invite: None,
            loading: true,
            copied: false,
            error: None,
            sent_channels: HashSet::new(),
            editing_settings: false,
            max_age: DEFAULT_MAX_AGE,
            max_uses: DEFAULT_MAX_USES,
            temporary: false,
            _subscriptions: vec![sub_q],
        });
        window.focus(&focus, cx);
        self.backend.create_channel_invite(
            seq,
            channel_id,
            CreateInviteRequest {
                max_age: Some(DEFAULT_MAX_AGE),
                max_uses: Some(DEFAULT_MAX_USES),
                temporary: Some(false),
                unique: None,
            },
        );
        cx.notify();
    }

    pub(crate) fn copy_invite_link(&mut self, cx: &mut Context<Self>) {
        let Some(url) = ({
            let Overlay::InviteMembers(modal) = &mut self.overlay else { return };
            let Some(inv) = modal.invite.as_ref() else { return };
            modal.copied = true;
            Some(self.backend.invite_url(&inv.code))
        }) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        cx.notify();
    }

    pub(crate) fn send_invite_to_dm(&mut self, dm_channel_id: Snowflake, cx: &mut Context<Self>) {
        let url = {
            let Overlay::InviteMembers(modal) = &self.overlay else { return };
            if modal.sent_channels.contains(&dm_channel_id) {
                return;
            }
            let Some(inv) = modal.invite.as_ref() else { return };
            self.backend.invite_url(&inv.code)
        };
        if self.backend.send_message(dm_channel_id, url)
            && let Overlay::InviteMembers(modal) = &mut self.overlay
        {
            modal.sent_channels.insert(dm_channel_id);
            modal.error = None;
            cx.notify();
        }
    }

    pub(crate) fn toggle_invite_settings(&mut self, editing: bool, cx: &mut Context<Self>) {
        let Overlay::InviteMembers(modal) = &mut self.overlay else { return };
        modal.editing_settings = editing;
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_invite_max_age(&mut self, max_age: u32, cx: &mut Context<Self>) {
        let Overlay::InviteMembers(modal) = &mut self.overlay else { return };
        modal.max_age = max_age;
        cx.notify();
    }

    pub(crate) fn set_invite_max_uses(&mut self, max_uses: u32, cx: &mut Context<Self>) {
        let Overlay::InviteMembers(modal) = &mut self.overlay else { return };
        modal.max_uses = max_uses;
        cx.notify();
    }

    pub(crate) fn set_invite_temporary(&mut self, temporary: bool, cx: &mut Context<Self>) {
        let Overlay::InviteMembers(modal) = &mut self.overlay else { return };
        modal.temporary = temporary;
        cx.notify();
    }

    pub(crate) fn regenerate_invite_link(&mut self, cx: &mut Context<Self>) {
        let next_seq = self.next_invite_seq();
        let (channel_id, request) = {
            let Overlay::InviteMembers(modal) = &mut self.overlay else { return };
            if modal.loading {
                return;
            }
            modal.seq = next_seq;
            modal.loading = true;
            modal.copied = false;
            modal.error = None;
            modal.editing_settings = false;
            (
                modal.channel_id,
                CreateInviteRequest {
                    max_age: Some(modal.max_age),
                    max_uses: Some(modal.max_uses),
                    temporary: Some(modal.temporary),
                    unique: Some(true),
                },
            )
        };
        self.backend.create_channel_invite(next_seq, channel_id, request);
        cx.notify();
    }
}

pub fn render(
    view: &ClientView,
    modal: &InviteMembersModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let header_title = if modal.editing_settings {
        l10n::tr("Invite link settings")
    } else {
        format!("{} {}", l10n::tr("Invite friends to"), modal.guild_name)
    };

    let header = div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(div().text_lg().font_weight(FontWeight::BOLD).truncate().child(header_title))
                .when(!modal.editing_settings, |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(Icon::new(AppIcon::Hash).with_size(px(13.)).text_color(theme.muted_foreground))
                            .child(modal.channel_name.clone()),
                    )
                }),
        )
        .child(
            div().debug_selector(|| "invite-close-btn".to_owned()).child(
                Button::new("invite-close-btn")
                    .ghost()
                    .small()
                    .icon(AppIcon::X)
                    .accessibility_label(l10n::tr("Close"))
                    .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            ),
        );

    let body = if modal.editing_settings {
        let mut age_row = div().flex().flex_row().flex_wrap().gap_1();
        for &(secs, label) in AGE_PRESETS {
            let selected = modal.max_age == secs;
            let sel = format!("invite-age-{secs}");
            age_row = age_row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("invite-age", secs as u64))
                        .ghost()
                        .small()
                        .selected(selected)
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_invite_max_age(secs, cx))),
                ),
            );
        }

        let mut uses_row = div().flex().flex_row().flex_wrap().gap_1();
        for &(uses, label) in USES_PRESETS {
            let selected = modal.max_uses == uses;
            let sel = format!("invite-uses-{uses}");
            uses_row = uses_row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("invite-uses", uses as u64))
                        .ghost()
                        .small()
                        .selected(selected)
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_invite_max_uses(uses, cx))),
                ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Expire after")),
                    )
                    .child(age_row),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Max number of uses")),
                    )
                    .child(uses_row),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Grant temporary membership")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Temporary members are kicked when they disconnect unless a role is assigned.")),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "invite-temp-switch".to_owned()).child(
                            Switch::new("invite-temp-switch")
                                .checked(modal.temporary)
                                .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                    this.set_invite_temporary(*checked, cx);
                                })),
                        ),
                    ),
            )
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
                        div().debug_selector(|| "invite-settings-cancel-btn".to_owned()).child(
                            Button::new("invite-settings-cancel-btn")
                                .ghost()
                                .small()
                                .label(l10n::tr("Cancel"))
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_invite_settings(false, cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "invite-generate-btn".to_owned()).child(
                            Button::new("invite-generate-btn")
                                .primary()
                                .small()
                                .disabled(modal.loading)
                                .label(l10n::tr("Generate a new link"))
                                .on_click(cx.listener(|this, _, _, cx| this.regenerate_invite_link(cx))),
                        ),
                    ),
            )
    } else {
        let q = modal.query.read(cx).value().trim().to_lowercase();
        let dms: Vec<(Snowflake, String, bool)> = {
            let state = view.backend.state();
            state
                .dm_channels()
                .into_iter()
                .filter(|c| c.kind != ChannelKind::DmPersonalNotes && c.kind.has_messages())
                .filter_map(|c| {
                    let name = channel_name(c);
                    if !q.is_empty() && !name.to_lowercase().contains(&q) {
                        return None;
                    }
                    Some((c.id, name, c.kind == ChannelKind::GroupDm))
                })
                .collect()
        };

        let mut dm_list = div()
            .id("invite-dm-list")
            .max_h(px(190.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();

        if dms.is_empty() {
            dm_list = dm_list.child(
                div()
                    .py_4()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("No matching direct messages")),
            );
        } else {
            for (cid, name, is_group) in dms {
                let sent = modal.sent_channels.contains(&cid);
                let sel = format!("invite-send-dm-{}", cid.0);
                let can_send = modal.invite.is_some() && !modal.loading && !sent;
                dm_list = dm_list.child(
                    div()
                        .id(("invite-dm-row", cid.0))
                        .px_2()
                        .py_1p5()
                        .rounded(px(6.))
                        .hover(|s| s.bg(theme.hover_bg))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    Icon::new(if is_group { AppIcon::UsersThree } else { AppIcon::Chats })
                                        .with_size(px(16.))
                                        .text_color(theme.muted_foreground),
                                )
                                .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name)),
                        )
                        .child(
                            div().debug_selector(move || sel.clone()).child(
                                Button::new(("invite-send-dm", cid.0))
                                    .ghost()
                                    .small()
                                    .disabled(!can_send)
                                    .label(if sent { l10n::tr("Sent") } else { l10n::tr("Send") })
                                    .on_click(cx.listener(move |this, _, _, cx| this.send_invite_to_dm(cid, cx))),
                            ),
                        ),
                );
            }
        }

        let invite_url_text: SharedString = match (&modal.invite, modal.loading) {
            (_, true) => l10n::tr("Generating invite link…").into(),
            (Some(inv), false) => view.backend.invite_url(&inv.code).into(),
            (None, false) => l10n::tr("Unavailable").into(),
        };
        let copy_label = if modal.copied {
            l10n::tr("Copied")
        } else {
            l10n::tr("Copy")
        };

        let link_footnote = modal.invite.as_ref().map(|inv| {
            let expiry = age_summary(inv.max_age);
            if inv.max_uses == 0 {
                format!("Your invite link expires in {expiry}.")
            } else {
                format!("Your invite link expires in {expiry} ({} max uses).", inv.max_uses)
            }
        });

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().debug_selector(|| "invite-search-input".to_owned()).child(Input::new(&modal.query).small()))
            .child(dm_list)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Or, send a community invite link to a friend")),
                    )
                    .child(
                        div()
                            .px_2p5()
                            .py_1p5()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .id("invite-url-text")
                                    .debug_selector(|| "invite-url-text".to_owned())
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .child(invite_url_text),
                            )
                            .child(
                                div().debug_selector(|| "invite-copy-btn".to_owned()).child(
                                    Button::new("invite-copy-btn")
                                        .primary()
                                        .small()
                                        .disabled(modal.invite.is_none() || modal.loading)
                                        .label(copy_label)
                                        .on_click(cx.listener(|this, _, _, cx| this.copy_invite_link(cx))),
                                ),
                            ),
                    )
                    .when_some(link_footnote, |d, note| {
                        d.child(div().text_xs().text_color(theme.muted_foreground).child(note))
                    })
                    .child(
                        div().flex().flex_row().justify_start().child(
                            div().debug_selector(|| "invite-edit-link-btn".to_owned()).child(
                                Button::new("invite-edit-link-btn")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Edit invite link"))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_invite_settings(true, cx))),
                            ),
                        ),
                    ),
            )
            .when_some(modal.error.clone(), |d, err| {
                d.child(div().text_xs().text_color(theme.danger).child(err))
            })
    };

    let content = div()
        .id("invite-members-modal")
        .debug_selector(|| "invite-members-modal".to_owned())
        .key_context(CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(header)
        .child(body);

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
