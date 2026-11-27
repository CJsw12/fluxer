//! User profile popover (`UserProfilePopout.tsx`): banner, avatar, bio,
//! guild roles, mutual guilds, private note editor and "Message" DM action.

use fluxer_client_core::rest::UserProfileFull;
use fluxer_client_core::state::{ChannelKind, ClientState, RelationshipKind, Snowflake};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, FontWeight, MouseButton, MouseDownEvent, ObjectFit,
    Pixels, Point, SharedString, Subscription, Window, div, img, prelude::*, px, rgb,
};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::profile::{ProfileCard, build_profile_card};
use crate::theme::Theme;
use crate::ui::avatar::{AvatarSpec, render_avatar};
use crate::ui::media_context_menu::{MediaContextMenuTarget, MediaMenuKind};
use crate::ui::moderation_modal::ModerationKind;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};
use crate::view_model::Place;

pub const CONTEXT: &str = "ProfilePopout";
const POPOUT_WIDTH: f32 = 300.;
const BANNER_HEIGHT: f32 = 105.;
const AVATAR_SIZE: f32 = 80.;
const MAX_NOTE_CHARS: usize = 256;

pub struct ProfilePopout {
    pub user_id: Snowflake,
    pub guild_id: Option<Snowflake>,
    pub anchor: Point<Pixels>,
    pub card: ProfileCard,
    static_cdn: Option<std::sync::Arc<str>>,
    pub error: Option<String>,
    pub note_input: Entity<InputState>,
    pub saved_note: String,
    pub confirm_blocked_dm: bool,
    pub can_edit_nickname: bool,
    pub can_timeout: bool,
    pub can_kick: bool,
    pub can_ban: bool,
    pub is_timed_out: bool,
    mutual_guild_menu: Option<(Snowflake, Point<Pixels>)>,
    pub _subscription: Subscription,
}

fn moderation_flags(
    state: &ClientState,
    user_id: Snowflake,
    guild_id: Option<Snowflake>,
) -> (bool, bool, bool, bool, bool) {
    let Some(gid) = guild_id else {
        return (false, false, false, false, false);
    };
    let now = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
    let is_timed_out = state
        .members
        .get(&gid)
        .and_then(|m| m.get(&user_id))
        .is_some_and(|m| m.is_timed_out(now));
    (
        state.can_edit_member_nickname(gid, user_id, now),
        state.can_timeout_member(gid, user_id),
        state.can_kick_member(gid, user_id),
        state.can_ban_member(gid, user_id),
        is_timed_out,
    )
}

pub(crate) fn find_1on1_dm(state: &ClientState, user_id: Snowflake) -> Option<Snowflake> {
    state
        .dm_channels()
        .into_iter()
        .find(|c| c.kind == ChannelKind::Dm && c.recipients.len() == 1 && c.recipients[0].id == user_id)
        .map(|c| c.id)
}

fn normalized_note(raw: &str) -> String {
    let trimmed = raw.trim();
    let mut utf16 = 0usize;
    let mut end = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        let next = utf16 + ch.len_utf16();
        if next > MAX_NOTE_CHARS {
            break;
        }
        utf16 = next;
        end = idx + ch.len_utf8();
    }
    trimmed[..end].to_owned()
}

impl ClientView {
    pub(crate) fn open_profile(
        &mut self,
        user_id: Snowflake,
        guild_id: Option<Snowflake>,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let (card, (can_edit_nickname, can_timeout, can_kick, can_ban, is_timed_out)) = {
            let state = self.backend.state();
            let fetched = self.profile_cache.get(&(user_id, guild_id));
            (
                build_profile_card(&state, self.backend.media(), user_id, guild_id, fetched),
                moderation_flags(&state, user_id, guild_id),
            )
        };
        let initial_note = card.note.clone();
        let note_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Click to add a note")));
        if !initial_note.is_empty() {
            note_input.update(cx, |s, cx| s.set_value(initial_note.clone(), window, cx));
        }
        let subscription = cx.subscribe_in(&note_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Blur | InputEvent::PressEnter { shift: false, .. } => this.save_profile_note(cx),
            _ => {}
        });
        self.overlay = Overlay::Profile(ProfilePopout {
            user_id,
            guild_id,
            anchor,
            card,
            static_cdn: self.backend.static_cdn.clone(),
            error: None,
            note_input,
            saved_note: initial_note,
            confirm_blocked_dm: false,
            can_edit_nickname,
            can_timeout,
            can_kick,
            can_ban,
            is_timed_out,
            mutual_guild_menu: None,
            _subscription: subscription,
        });
        if self.profile_in_flight.insert((user_id, guild_id)) {
            self.backend.request_profile(user_id, guild_id);
        }
        cx.notify();
    }

    pub(crate) fn profile_loaded(
        &mut self,
        user_id: Snowflake,
        guild_id: Option<Snowflake>,
        result: Result<UserProfileFull, String>,
        cx: &mut Context<Self>,
    ) {
        self.profile_in_flight.remove(&(user_id, guild_id));
        match result {
            Ok(full) => {
                self.profile_cache.insert((user_id, guild_id), full);
            }
            Err(err) => {
                tracing::warn!(%user_id, %err, "profile fetch failed");
                if let Overlay::Profile(pop) = &mut self.overlay
                    && pop.user_id == user_id
                    && pop.guild_id == guild_id
                {
                    pop.card.loaded = true;
                    pop.error = Some(err);
                    cx.notify();
                }
                return;
            }
        }
        if let Overlay::Profile(pop) = &mut self.overlay
            && pop.user_id == user_id
            && pop.guild_id == guild_id
        {
            let state = self.backend.state();
            let fetched = self.profile_cache.get(&(user_id, guild_id));
            pop.card = build_profile_card(&state, self.backend.media(), user_id, guild_id, fetched);
            (
                pop.can_edit_nickname,
                pop.can_timeout,
                pop.can_kick,
                pop.can_ban,
                pop.is_timed_out,
            ) = moderation_flags(&state, user_id, guild_id);
            pop.error = None;
            cx.notify();
        }
    }

    /// Rebuilds the currently open profile card from live state so friend/block
    /// changes update immediately.
    pub(crate) fn refresh_open_profile(&mut self) {
        if let Overlay::Profile(pop) = &mut self.overlay {
            let was_loaded = pop.card.loaded;
            let state = self.backend.state();
            let fetched = self.profile_cache.get(&(pop.user_id, pop.guild_id));
            pop.card = build_profile_card(&state, self.backend.media(), pop.user_id, pop.guild_id, fetched);
            (
                pop.can_edit_nickname,
                pop.can_timeout,
                pop.can_kick,
                pop.can_ban,
                pop.is_timed_out,
            ) = moderation_flags(&state, pop.user_id, pop.guild_id);
            if was_loaded {
                pop.card.loaded = true;
            }
        }
    }

    /// Resyncs `saved_note` after a failed save rollback so the user can retry.
    pub(crate) fn sync_profile_note_from_state(&mut self, user_id: Snowflake) {
        if let Overlay::Profile(pop) = &mut self.overlay
            && pop.user_id == user_id
        {
            pop.saved_note = self.backend.state().note(user_id).unwrap_or_default().to_owned();
        }
    }

    pub(crate) fn save_profile_note(&mut self, cx: &mut Context<Self>) {
        let (user_id, next_note) = {
            let Overlay::Profile(pop) = &mut self.overlay else { return };
            let raw = pop.note_input.read(cx).value().to_string();
            let capped = normalized_note(&raw);
            if capped == pop.saved_note {
                return;
            }
            pop.saved_note = capped.clone();
            (pop.user_id, (!capped.is_empty()).then_some(capped))
        };
        self.backend.save_note(user_id, next_note);
    }

    pub(crate) fn profile_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let user_id = {
            let Overlay::Profile(pop) = &mut self.overlay else { return };
            if pop.card.is_blocked && !pop.confirm_blocked_dm {
                pop.confirm_blocked_dm = true;
                cx.notify();
                return;
            }
            pop.user_id
        };
        self.close_overlay(window, cx);
        let existing = find_1on1_dm(&self.backend.state(), user_id);
        match existing {
            Some(channel_id) => self.open_channel(None, channel_id, cx),
            None => self.backend.open_dm(user_id),
        }
    }

    pub(crate) fn profile_open_guild(&mut self, guild_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(window, cx);
        self.select_place(Place::Guild(guild_id), cx);
    }

    fn close_profile_guild_menu(&mut self, cx: &mut Context<Self>) -> bool {
        if let Overlay::Profile(pop) = &mut self.overlay
            && pop.mutual_guild_menu.take().is_some()
        {
            cx.notify();
            return true;
        }
        false
    }
}

pub fn render(pop: &ProfilePopout, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let card = &pop.card;
    let banner = div()
        .id("profile-banner")
        .relative()
        .w_full()
        .h(px(BANNER_HEIGHT))
        .rounded_t(px(8.))
        .overflow_hidden()
        .bg(rgb(card.banner_color))
        .flx("user.profile.profile-card.profile-card-banner.banner-wrapper.banner-context-menu")
        .when_some(card.banner_url.clone(), |el, url| {
            el.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let target = MediaContextMenuTarget::new(MediaMenuKind::Image, url.clone());
                    this.open_media_context_menu_at(target, ev.position, window, cx);
                }),
            )
        })
        .children(card.banner_url.clone().map(|url| {
            img(url)
                .id(SharedString::from(format!("profile-banner-{}", card.user_id.0)))
                .size_full()
                .object_fit(ObjectFit::Cover)
        }));

    let avatar_el = div()
        .absolute()
        .left(px(12.))
        .top(px(BANNER_HEIGHT - AVATAR_SIZE * 0.55 - 2.))
        .p(px(6.))
        .rounded_full()
        .bg(theme.background_secondary)
        .child(render_avatar(
            AvatarSpec {
                user_id: card.user_id,
                avatar_url: card.avatar_url.clone().map(SharedString::from),
                display_name: card.display_name.clone().into(),
                size: AVATAR_SIZE,
                status: None,
                status_border_bg: Some(theme.background_secondary),
            },
            theme,
        ));

    let header_block = div()
        .relative()
        .w_full()
        .pb(px(AVATAR_SIZE * 0.45 + 8.))
        .child(banner)
        .child(avatar_el);

    let display_name_tip = card.display_name.clone();
    let badges: Vec<_> = card.profile_badges.iter().filter_map(|badge| {
        let image = crate::profile::profile_badge_image(badge, pop.static_cdn.as_deref());
        let caption = badge.caption.as_deref().filter(|caption| !caption.is_empty());
        let content = if let Some(url) = image {
            let ratio = badge.aspect_ratio.unwrap_or(1.0).clamp(0.25, 3.0);
            img(url).h(px(20.)).w(px(20. * ratio)).object_fit(ObjectFit::Contain).into_any_element()
        } else {
            div().text_xs().child(caption?.to_owned()).into_any_element()
        };
        let tooltip = crate::profile::profile_badge_tooltip(badge);
        Some(div()
            .id(SharedString::from(badge.key.clone()))
            .aria_label(caption.unwrap_or(&badge.label).to_owned())
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .child(content))
    }).collect();
    let user_id_text = card.user_id.to_string();
    let mut body = div()
        .px_4()
        .pb_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("profile-display-name")
                                .min_w_0()
                                .truncate()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .flx("user.profile.profile-card.profile-card-user-info.display-name-tooltip")
                                .tooltip(move |window, cx| Tooltip::new(display_name_tip.clone()).build(window, cx))
                                .child(card.display_name.clone()),
                        )
                        .when(!badges.is_empty(), |d| {
                            d.child(div().flex().items_center().gap_1().flx("profile-badges").children(badges))
                        })
                        .when(card.bot, |d| {
                            d.child(
                                div()
                                    .px_1()
                                    .rounded(px(3.))
                                    .bg(theme.brand)
                                    .text_xs()
                                    .text_color(theme.on_fill)
                                    .child("BOT"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(card.username_tag.clone()))
                        .child(
                            div()
                                .id("profile-copy-user-id")
                                .size(px(16.))
                                .flex_none()
                                .cursor_pointer()
                                .aria_label(l10n::tr("Copy user ID"))
                                .flx("user.profile.profile-card.profile-card-actions.tooltip--2")
                                .tooltip(|window, cx| Tooltip::new(l10n::tr("Copy user ID")).build(window, cx))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(user_id_text.clone()));
                                })
                                .child(Icon::new(AppIcon::Snowflake).with_size(px(16.)).text_color(theme.muted_foreground)),
                        ),
                )
                .children(
                    card.pronouns
                        .as_ref()
                        .filter(|p| !p.trim().is_empty())
                        .map(|p| div().text_xs().text_color(theme.muted_foreground).child(p.clone())),
                ),
        );

    if !card.is_self {
        let user_id = card.user_id;
        let label = if pop.confirm_blocked_dm {
            l10n::tr("Open DM")
        } else {
            l10n::tr("Message")
        };
        let mut actions = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .child(
                Button::new("profile-message-btn")
                    .primary()
                    .small()
                    .label(label)
                    .on_click(cx.listener(|this, _, window, cx| this.profile_message(window, cx))),
            );
        match card.relationship {
            None | Some(RelationshipKind::Unknown(_)) => {
                actions = actions
                    .child(
                        div()
                            .id("profile-add-friend-wrap")
                            .aria_label(l10n::tr("Send friend request"))
                            .flx("user.user-profile-modal.render-primary-action-button.tooltip--5")
                            .child(
                                Button::new("profile-add-friend-btn")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Send friend request"))
                                    .tooltip(l10n::tr("Send friend request"))
                                    .on_click(cx.listener(move |this, _, _, _| this.backend.send_friend_request(user_id))),
                            ),
                    )
                    .child(
                        Button::new("profile-block-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Block"))
                            .on_click(cx.listener(move |this, _, _, _| this.backend.block_user(user_id))),
                    );
            }
            Some(RelationshipKind::Friend) => {
                actions = actions
                    .child(
                        div()
                            .id("profile-remove-friend-wrap")
                            .aria_label(l10n::tr("Remove friend"))
                            .flx("user.user-profile-modal.render-primary-action-button.tooltip")
                            .child(
                                Button::new("profile-remove-friend-btn")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Remove friend"))
                                    .tooltip(l10n::tr("Remove friend"))
                                    .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(user_id))),
                            ),
                    )
                    .child(
                        Button::new("profile-block-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Block"))
                            .on_click(cx.listener(move |this, _, _, _| this.backend.block_user(user_id))),
                    );
            }
            Some(RelationshipKind::IncomingRequest) => {
                actions = actions
                    .child(
                        div()
                            .id("profile-accept-wrap")
                            .aria_label(l10n::tr("Accept friend request"))
                            .flx("user.user-profile-modal.render-primary-action-button.tooltip--3")
                            .child(
                                Button::new("profile-accept-btn")
                                    .primary()
                                    .small()
                                    .label(l10n::tr("Accept friend request"))
                                    .tooltip(l10n::tr("Accept friend request"))
                                    .on_click(cx.listener(move |this, _, _, _| this.backend.accept_friend_request(user_id))),
                            ),
                    )
                    .child(
                        Button::new("profile-ignore-btn")
                            .ghost()
                            .small()
                            .label(l10n::tr("Ignore"))
                            .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(user_id))),
                    )
                    .child(
                        Button::new("profile-block-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Block"))
                            .on_click(cx.listener(move |this, _, _, _| this.backend.block_user(user_id))),
                    );
            }
            Some(RelationshipKind::OutgoingRequest) => {
                actions = actions
                    .child(
                        div()
                            .id("profile-cancel-req-wrap")
                            .aria_label(l10n::tr("Cancel friend request"))
                            .flx("user.user-profile-modal.render-primary-action-button.tooltip--4")
                            .child(
                                Button::new("profile-cancel-req-btn")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Cancel friend request"))
                                    .tooltip(l10n::tr("Cancel friend request"))
                                    .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(user_id))),
                            ),
                    )
                    .child(
                        Button::new("profile-block-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Block"))
                            .on_click(cx.listener(move |this, _, _, _| this.backend.block_user(user_id))),
                    );
            }
            Some(RelationshipKind::Blocked) => {
                actions = actions.child(
                    div()
                        .id("profile-unblock-wrap")
                        .aria_label(l10n::tr("Unblock user"))
                        .flx("user.user-profile-modal.render-primary-action-button.tooltip--2")
                        .child(
                            Button::new("profile-unblock-btn")
                                .ghost()
                                .small()
                                .label(l10n::tr("Unblock user"))
                                .tooltip(l10n::tr("Unblock user"))
                                .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(user_id))),
                        ),
                );
            }
        }
        body = body.child(actions);
    }

    if let Some(guild_id) = pop.guild_id {
        let user_id = card.user_id;
        if card.is_self && pop.can_edit_nickname {
            body = body.child(
                div().debug_selector(|| "profile-nickname-btn".to_owned()).child(
                    Button::new("profile-nickname-btn")
                        .ghost()
                        .small()
                        .label(l10n::tr("Change nickname"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_member_moderation(
                                guild_id,
                                user_id,
                                ModerationKind::Nickname { is_self: true, original_nick: String::new() },
                                window,
                                cx,
                            );
                        })),
                ),
            );
        } else if !card.is_self && (pop.can_edit_nickname || pop.can_timeout || pop.can_kick || pop.can_ban) {
            let mut mod_row = div().flex().flex_row().flex_wrap().gap_1();
            if pop.can_edit_nickname {
                mod_row = mod_row.child(
                    div().debug_selector(|| "profile-nickname-btn".to_owned()).child(
                        Button::new("profile-nickname-btn")
                            .ghost()
                            .small()
                            .label(l10n::tr("Change nickname"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_member_moderation(
                                    guild_id,
                                    user_id,
                                    ModerationKind::Nickname { is_self: false, original_nick: String::new() },
                                    window,
                                    cx,
                                );
                            })),
                    ),
                );
            }
            if pop.can_timeout {
                let is_timed_out = pop.is_timed_out;
                let timeout_label = if is_timed_out {
                    l10n::tr("Remove timeout")
                } else {
                    l10n::tr("Timeout")
                };
                mod_row = mod_row.child(
                    div().debug_selector(|| "profile-timeout-btn".to_owned()).child(
                        Button::new("profile-timeout-btn")
                            .ghost()
                            .small()
                            .label(timeout_label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_member_moderation(
                                    guild_id,
                                    user_id,
                                    ModerationKind::Timeout { currently_timed_out: is_timed_out },
                                    window,
                                    cx,
                                );
                            })),
                    ),
                );
            }
            if pop.can_kick {
                mod_row = mod_row.child(
                    div().debug_selector(|| "profile-kick-btn".to_owned()).child(
                        Button::new("profile-kick-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Kick"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_member_moderation(guild_id, user_id, ModerationKind::Kick, window, cx);
                            })),
                    ),
                );
            }
            if pop.can_ban {
                mod_row = mod_row.child(
                    div().debug_selector(|| "profile-ban-btn".to_owned()).child(
                        Button::new("profile-ban-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Ban"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_member_moderation(guild_id, user_id, ModerationKind::Ban, window, cx);
                            })),
                    ),
                );
            }
            body = body.child(
                div()
                    .debug_selector(|| "profile-moderation-card".to_owned())
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Moderation")),
                    )
                    .child(mod_row),
            );
        }
    }

    if let Some(bio) = card.bio.as_ref().filter(|b| !b.trim().is_empty()) {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("About Me")),
                )
                .child(div().text_sm().child(bio.clone())),
        );
    }

    if card.guild_id.is_some() {
        let roles_content = if card.roles.is_empty() {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("This user has no roles in this community."))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .children(card.roles.iter().map(|role| {
                    let dot_color = role.color.map(|c| rgb(c).into()).unwrap_or(theme.muted_foreground);
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py(px(2.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .text_xs()
                        .child(div().size(px(8.)).rounded_full().bg(dot_color))
                        .child(role.name.clone())
                }))
                .into_any_element()
        };
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Roles")),
                )
                .child(roles_content),
        );
    }

    if card.created_date.is_some() || card.joined_date.is_some() {
        let mut rows = div().flex().flex_col().gap_1();
        if let Some(created) = card.created_date.clone() {
            let tip = l10n::tr("Account creation date");
            rows = rows.child(
                div()
                    .id("profile-membership-created")
                    .text_sm()
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .child(format!("{} {created}", l10n::tr("Member since"))),
            );
        }
        if let Some(joined) = card.joined_date.clone() {
            let tip = l10n::tr("Community join date");
            rows = rows.child(
                div()
                    .id("profile-membership-joined")
                    .text_sm()
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .child(format!("{} {joined}", l10n::tr("Joined this community"))),
            );
        }
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flx("membership")
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Membership")),
                )
                .child(rows),
        );
    }

    if let Some(voice) = card.voice_activity.clone() {
        let tip = l10n::tr("Currently in a voice channel");
        let place = match &voice.guild_name {
            Some(guild) => format!("{} / {guild}", voice.channel_name),
            None => voice.channel_name.clone(),
        };
        body = body.child(
            div()
                .id("profile-voice-activity")
                .flex()
                .flex_col()
                .gap_1()
                .flx("voice-activity")
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Voice Activity")),
                )
                .child(div().text_sm().truncate().child(place)),
        );
    }

    if !card.mutual_friends.is_empty() {
        let tip = l10n::tr("Friends you both have");
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flx("mutual-friends")
                .child(
                    div()
                        .id("profile-mutual-friends-title")
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .child(format!("{} \u{2014} {}", l10n::tr("Mutual Friends"), card.mutual_friends.len())),
                )
                // TODO(backlog): official rows open the friend's profile; display-only for now.
                .children(card.mutual_friends.iter().map(|f| {
                    div()
                        .id(("profile-mutual-friend", f.id.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .child(
                            div()
                                .size(px(22.))
                                .rounded_full()
                                .bg(theme.muted)
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .child(f.initials.clone()),
                        )
                        .child(div().text_sm().truncate().child(f.name.clone()))
                })),
        );
    }

    if !card.connected_accounts.is_empty() {
        body = body.child(
            div().flex().flex_col().gap_1().flx("profile-connected-accounts")
                .children(card.connected_accounts.iter().enumerate().map(|(index, account)| {
                    let platform = match account.kind.as_str() {
                        "bsky" => l10n::tr("Bluesky"),
                        "domain" => l10n::tr("Domain"),
                        "oauth" => account.provider_id.clone().unwrap_or_else(|| "OAuth".to_owned()),
                        other => other.to_owned(),
                    };
                    let verified = l10n::tr("Verified");
                    let mut tip = format!("{platform} · {}", account.name);
                    if account.verified {
                        tip.push_str(&format!(" · {verified}"));
                    }
                    div().id(("profile-connected-account", index))
                        .rounded(px(4.)).border_1().border_color(theme.border)
                        .flex().flex_col().gap_1().px_2().py_1()
                        .text_xs().text_color(theme.muted_foreground)
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .child(platform)
                        .child(div().text_sm().text_color(theme.foreground).truncate().child(account.name.clone()))
                        .when(account.verified, |d| d.child(verified))
                })),
        );
    }

    if !card.mutual_guilds.is_empty() {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Mutual Communities")),
                )
                .children(card.mutual_guilds.iter().map(|g| {
                    let guild_id = g.id;
                    div()
                        .id(("profile-mutual-guild", guild_id.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .flx("user.user-profile-modal.render-mutual-communities-groups-list.mutual-guild-item.guild-click")
                        .flx("user.user-profile-modal.mutual-guild-item.button.click")
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if let Overlay::Profile(pop) = &mut this.overlay {
                                    pop.mutual_guild_menu = Some((guild_id, ev.position));
                                    cx.notify();
                                }
                            }),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| this.profile_open_guild(guild_id, window, cx)))
                        .child(
                            div()
                                .size(px(22.))
                                .rounded(px(6.))
                                .bg(theme.muted)
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .child(g.initials.clone()),
                        )
                        .child(div().text_sm().truncate().child(g.name.clone()))
                })),
        );
    }

    let note_tip = if pop.saved_note.is_empty() { l10n::tr("Add note") } else { pop.saved_note.clone() };
    body = body.child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id("profile-note-tooltip")
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .flx("user.profile.profile-card.profile-card-actions.focus-ring")
                    .flx("user.profile.profile-card.profile-card-actions.tooltip")
                    .tooltip(move |window, cx| Tooltip::new(note_tip.clone()).build(window, cx))
                    .child(l10n::tr("Note")),
            )
            .child(Input::new(&pop.note_input)),
    );

    if !card.loaded {
        body = body.child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("Loading…")));
    } else if let Some(err) = &pop.error {
        body = body.child(div().text_xs().text_color(theme.danger).child(err.clone()));
    }

    let content = div()
        .id("profile-popout")
        .key_context(CONTEXT)
        .w(px(POPOUT_WIDTH))
        .max_h(px(540.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            if !this.close_profile_guild_menu(cx) {
                this.close_overlay(window, cx);
            }
        }))
        .child(header_block)
        .child(body);

    let view = cx.weak_entity();
    let profile = overlay::popover(content, pop.anchor, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    });
    if let Some((guild_id, anchor)) = pop.mutual_guild_menu {
        let view = cx.weak_entity();
        let menu = div()
            .id("profile-mutual-guild-menu")
            .w(px(208.))
            .p_1()
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                if ev.button == MouseButton::Right {
                    this.close_profile_guild_menu(cx);
                }
            }))
            .flx("ui.action-menu.guild-context-menu.data-menu-renderer")
            .child(
                Button::new("profile-copy-community-id")
                    .ghost()
                    .small()
                    .label(l10n::tr("Copy community ID"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(guild_id.to_string()));
                        this.close_profile_guild_menu(cx);
                    })),
            );
        return div()
            .absolute()
            .inset_0()
            .child(profile)
            .child(overlay::popover(menu, anchor, theme, move |_, cx| {
                let _ = view.update(cx, |v, cx| v.close_profile_guild_menu(cx));
            }))
            .into_any_element();
    }
    profile
}
