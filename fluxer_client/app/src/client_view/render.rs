//! Render helpers: overlay, main pane, composer and jump-to-present.

use super::*;

impl ClientView {
    fn render_jump_to_present(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.loading_history { l10n::tr("Loading…") } else { l10n::tr("Jump to present") };
        div()
            .id("jump-to-present-bar")
            .debug_selector(|| "jump-to-present".to_owned())
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background_secondary)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Viewing older messages")),
            )
            .child(
                div().debug_selector(|| "jump-to-present-btn".to_owned()).child(
                    Button::new("jump-to-present-btn")
                        .primary()
                        .small()
                        .label(label)
                        .on_click(cx.listener(|v, _, _, cx| v.jump_to_present(cx))),
                ),
            )
    }

    #[inline(never)]
    pub(super) fn render_overlay(&self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        match &self.overlay {
            Overlay::QuickSwitcher(sw) => Some(ui::quick_switcher::render(sw, theme, cx)),
            Overlay::EmojiPicker(picker) => {
                Some(ui::emoji_picker::render(self, picker, self.backend.media(), window.viewport_size(), theme, cx))
            }
            Overlay::ChannelPopout(pop) => {
                Some(ui::channel_popouts::render(self, pop, window.viewport_size(), theme, cx))
            }
            Overlay::Profile(pop) => Some(ui::profile_popout::render(pop, theme, cx)),
            Overlay::Settings(modal) => Some(ui::settings::render(self, modal, theme, cx)),
            Overlay::AccountSwitcher(modal) => Some(ui::account_switcher::render(self, modal, theme, cx)),
            Overlay::JoinGuild(modal) => Some(ui::guild_modal::render_join(modal, theme, cx)),
            Overlay::LeaveGuild(modal) => Some(ui::guild_modal::render_leave(modal, theme, cx)),
            Overlay::CreateGroupDm(modal) => {
                let query = modal.filter_input.read(cx).value().to_string();
                Some(ui::group_dm::render_create_modal(modal, theme, cx, &query))
            }
            Overlay::LeaveGroupDm(modal) => Some(ui::group_dm::render_leave_modal(modal, theme, cx)),
            Overlay::MediaViewer(modal) => Some(ui::media_viewer::render(modal, theme, window, cx)),
            Overlay::LinkWarning(modal) => Some(ui::link_warning::render(modal, theme, cx)),
            Overlay::CreateChannel(modal) => Some(ui::channel_modal::render_create(modal, theme, cx)),
            Overlay::EditChannel(modal) => Some(ui::channel_modal::render_edit(modal, theme, cx)),
            Overlay::MemberModeration(modal) => Some(ui::moderation_modal::render_action(modal, theme, cx)),
            Overlay::GuildBans(modal) => Some(ui::moderation_modal::render_bans(modal, theme, cx)),
            Overlay::Access(modal) => Some(ui::access::render(self, modal, theme, cx)),
            Overlay::ForwardMessage(modal) => Some(ui::forward_modal::render(self, modal, theme, cx)),
            Overlay::CreateThread(modal) => Some(ui::create_thread_modal::render(modal, theme, cx)),
            Overlay::InviteMembers(modal) => Some(ui::invite_modal::render(self, modal, theme, cx)),
            Overlay::GuildNotifications(modal) => {
                Some(ui::guild_settings_modals::render_notifications(self, modal, theme, cx))
            }
            Overlay::GuildPrivacy(modal) => Some(ui::guild_settings_modals::render_privacy(self, modal, theme, cx)),
            Overlay::GuildProfile(modal) => Some(ui::guild_settings_modals::render_profile(modal, theme, cx)),
            Overlay::DeleteMyGuildMessages(modal) => {
                Some(ui::guild_settings_modals::render_delete_messages(modal, theme, cx))
            }
            Overlay::PurgePersonalNotes(modal) => {
                Some(ui::guild_settings_modals::render_purge_personal_notes(modal, theme, cx))
            }
            Overlay::DiscoveryPreview(modal) => Some(ui::explore::render_preview_modal(self, modal, theme, cx)),
            Overlay::ScreenSharePicker(modal) => Some(ui::screen_share_modal::render(self, modal, theme, cx)),
            Overlay::None => None,
        }
    }

    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_main_pane(
        &mut self,
        handle: gpui_kit::WeakEntity<Self>,
        composer: AnyElement,
        right_panel: &mut Option<AnyElement>,
        show_voice_stage: bool,
        show_explore: bool,
        show_friends: bool,
        split_height: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if show_voice_stage {
            let split_chat = !self.voice_stage_expanded && self.selected_channel.is_some();
            if split_chat {
                let search_panel = if self.search.is_some() {
                    right_panel.take()
                } else {
                    let _ = right_panel.take();
                    None
                };
                div()
                    .debug_selector(|| "voice-stage-split-chat".to_owned())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    // OutlineFrame.module.css: the pane sits inside the 1px frame border.
                    .pt(px(1.))
                    .pb(px(1.))
                    .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| v.add_paths(paths.paths(), cx)))
                    .child(ui::header::render(self, theme, cx))
                    .child(ui::voice_stage::render(self, theme, split_height, cx))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .when(
                                        !self.guild_unavailable
                                            && self.selected_channel.is_some()
                                            && self.unread_anchor.is_some(),
                                        |d| d.child(self.render_unread_bar(theme, cx)),
                                    )
                                    .child(ui::message_list::render(self, handle, theme))
                                    .when(self.has_more_after && self.selected_channel.is_some(), |d| {
                                        d.child(self.render_jump_to_present(theme, cx))
                                    })
                                    .child(composer),
                            )
                            .children(search_panel),
                    )
                    .into_any_element()
            } else {
                let _ = right_panel.take();
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(ui::voice_stage::render(self, theme, split_height, cx))
                    // Keep composer mounted in a 0px hidden slot so shortcut contexts stay live.
                    .child(div().h(px(0.)).overflow_hidden().child(composer))
                    .into_any_element()
            }
        } else if show_explore {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                .pt(px(1.))
                .pb(px(1.))
                .child(ui::explore::render_page(self, theme, cx))
                // Keep the composer mounted in a 0px hidden slot so shortcut contexts stay live.
                .child(div().h(px(0.)).overflow_hidden().child(composer))
                .into_any_element()
        } else if show_friends {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(ui::friends::render(self, theme, cx))
                // Keep the composer mounted in a 0px hidden slot so the
                // `ClientView > Composer > Input` shortcut context stays live.
                .child(div().h(px(0.)).overflow_hidden().child(composer))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                // OutlineFrame.module.css: the pane sits inside the 1px frame border.
                .pt(px(1.))
                .pb(px(1.))
                // Files dropped anywhere on the message pane go to the tray.
                .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| v.add_paths(paths.paths(), cx)))
                .child(ui::header::render(self, theme, cx))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .when_some(self.voice.watched_video_track.as_ref(), |d, track| {
                                    let label = ui::video_viewer::participant_display_name(self, track);
                                    d.child(ui::video_viewer::render(
                                        track,
                                        label,
                                        self.watched_video_frame.clone(),
                                        theme,
                                        cx,
                                    ))
                                })
                                .when(!self.guild_unavailable && self.selected_channel.is_some() && self.unread_anchor.is_some(), |d| {
                                    d.child(self.render_unread_bar(theme, cx))
                                })
                                .child(ui::message_list::render(self, handle, theme))
                                .when(self.has_more_after && self.selected_channel.is_some(), |d| {
                                    d.child(self.render_jump_to_present(theme, cx))
                                })
                                .child(composer),
                        )
                        .children(right_panel.take()),
                )
                .into_any_element()
        }
    }

    /// The composer with its mode bar (reply / edit), the typing row and
    /// the Esc / Up hooks.
    pub(super) fn render_composer(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (jump, mention, cancel) = (cx.weak_entity(), cx.weak_entity(), cx.weak_entity());
        let top_bar = ui::composer::mode_bar(
            self.composer_mode.mode(),
            theme,
            move |channel_id, message_id, _, cx| {
                let _ = jump.update(cx, |v, cx| v.jump_to_message(channel_id, message_id, cx));
            },
            move |_, cx| {
                let _ = mention.update(cx, |v, cx| {
                    v.composer_mode.toggle_mention();
                    cx.notify();
                });
            },
            move |window, cx| {
                let _ = cancel.update(cx, |v, cx| v.cancel_composer_mode(window, cx));
            },
        );
        let typing_row = ui::typing::typing_indicator(self.typing_text.clone(), theme);
        let keys = ComposerKeys {
            on_escape: Self::on_escape,
            on_up: Self::on_up,
            on_down: Self::on_down,
            on_accept: Self::on_accept,
        };
        let popup = ui::autocomplete::render(self, theme, cx);
        ui::composer::render_with(self, theme, popup, top_bar, typing_row, keys, cx).into_any_element()
    }
}
