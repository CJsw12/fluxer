//! Place/channel selection, explore/discovery and rail hover state.

use super::*;

impl ClientView {
    pub(crate) fn select_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.voice_stage_open = false;
        self.favorites_selected = false;
        self.place = place;
        if let Place::Guild(id) = place {
            self.last_guild = Some(id);
            self.backend.subscribe_guild(id);
        }
        if place == Place::Explore {
            self.clear_selection(cx);
            self.ensure_explore_loaded(cx);
            return;
        }
        self.refresh(cx);
        let opens = |id: Snowflake| self.channels.iter().any(|c| c.id == id && c.has_messages);
        let remembered = self.last_channel.get(&nav::place_key(place)).copied().filter(|id| opens(*id));
        let first = remembered.or_else(|| self.channels.iter().find(|c| c.has_messages).map(|c| c.id));
        match first {
            Some(id) => self.select_channel(id, cx),
            None => self.clear_selection(cx),
        }
    }

    pub(crate) fn open_explore(&mut self, cx: &mut Context<Self>) {
        self.select_place(Place::Explore, cx);
    }

    fn ensure_explore_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.explore.categories_fetched {
            self.explore.categories_fetched = true;
            self.backend.fetch_discovery_categories();
        }
        if !self.explore.has_loaded && !self.explore.loading {
            self.fetch_explore_guilds(0, cx);
        }
    }

    fn fetch_explore_guilds(&mut self, offset: u32, cx: &mut Context<Self>) {
        self.explore.search_seq = self.explore.search_seq.wrapping_add(1);
        let seq = self.explore.search_seq;
        self.explore.loading = true;
        self.explore.error = None;
        if offset == 0 {
            self.explore.guilds.clear();
            self.explore.total = 0;
        }
        let query_text = self.explore.submitted_query.trim().to_owned();
        let query = DiscoveryGuildQuery {
            query: (!query_text.is_empty()).then_some(query_text),
            category: self.explore.selected_category,
            language: self.explore.selected_language.clone(),
            tag: None,
            sort_by: None,
            limit: ui::explore::PAGE_SIZE,
            offset,
        };
        self.backend.search_discovery_guilds(seq, query);
        cx.notify();
    }

    pub(crate) fn explore_select_category(&mut self, category: Option<u32>, cx: &mut Context<Self>) {
        self.explore.selected_category = category;
        self.explore.submitted_query.clear();
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn explore_select_language(&mut self, language: Option<String>, cx: &mut Context<Self>) {
        self.explore.selected_language = language;
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn submit_explore_search(&mut self, cx: &mut Context<Self>) {
        let q = self.explore.search_input.read(cx).value().trim().to_owned();
        self.explore.submitted_query = q;
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn clear_explore_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.explore.submitted_query.clear();
        self.explore.search_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn explore_reset_to_all(&mut self, cx: &mut Context<Self>) {
        self.explore.selected_category = None;
        self.explore.submitted_query.clear();
        let (input, handle) = (self.explore.search_input.clone(), self.window_handle);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                input.update(cx, |s, cx| s.set_value("", window, cx));
            });
        });
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn explore_load_more(&mut self, cx: &mut Context<Self>) {
        if self.explore.loading || (self.explore.guilds.len() as u64) >= self.explore.total {
            return;
        }
        let offset = u32::try_from(self.explore.guilds.len()).unwrap_or(u32::MAX);
        self.fetch_explore_guilds(offset, cx);
    }

    pub(crate) fn explore_retry(&mut self, cx: &mut Context<Self>) {
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn open_discovery_preview(
        &mut self,
        guild: DiscoveryGuild,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::DiscoveryPreview(Box::new(ui::explore::DiscoveryPreviewModal {
            guild,
            joining: false,
            join_seq: None,
            error: None,
        }));
        cx.notify();
    }

    pub(crate) fn confirm_join_discovery_guild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::DiscoveryPreview(modal) = &mut self.overlay else {
            return;
        };
        let Some(guild_id) = Snowflake::parse(&modal.guild.id) else {
            modal.error = Some(l10n::tr("Invalid community ID."));
            cx.notify();
            return;
        };
        if self.backend.state().guilds.contains_key(&guild_id) {
            self.close_overlay(window, cx);
            self.select_place(Place::Guild(guild_id), cx);
            return;
        }
        if modal.joining {
            return;
        }
        self.explore.join_seq = self.explore.join_seq.wrapping_add(1);
        let seq = self.explore.join_seq;
        if let Overlay::DiscoveryPreview(modal) = &mut self.overlay {
            modal.joining = true;
            modal.join_seq = Some(seq);
            modal.error = None;
        }
        self.backend.join_discovery_guild(seq, guild_id);
        cx.notify();
    }

    pub(crate) fn select_favorites(&mut self, cx: &mut Context<Self>) {
        self.select_place(Place::Home, cx);
        self.favorites_selected = true;
        cx.notify();
    }

    pub(crate) fn set_rail_hover(&mut self, target: RailHoverTarget, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(target)
        } else if self.hovered_rail_item == Some(target) {
            None
        } else {
            self.hovered_rail_item
        };
        if self.hovered_rail_item != next {
            self.hovered_rail_item = next;
            cx.notify();
        }
    }

    pub(crate) fn set_header_hover(&mut self, target: HeaderHoverTarget, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(target)
        } else if self.hovered_header_item == Some(target) {
            None
        } else {
            self.hovered_header_item
        };
        if self.hovered_header_item != next {
            self.hovered_header_item = next;
            cx.notify();
        }
    }

    pub(crate) fn set_user_control_hover(
        &mut self,
        target: UserControlHoverTarget,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let next = if hovered {
            Some(target)
        } else if self.hovered_user_control == Some(target) {
            None
        } else {
            self.hovered_user_control
        };
        if self.hovered_user_control != next {
            self.hovered_user_control = next;
            cx.notify();
        }
    }

    pub(crate) fn select_channel(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        self.voice_stage_open = false;
        self.select_channel_inner(channel_id, true, cx);
    }

    pub(crate) fn select_channel_inner(
        &mut self,
        channel_id: Snowflake,
        auto_load_latest: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let opens_pane = {
            let state = self.backend.state();
            state.can_view_channel(channel_id)
                && state.channels.get(&channel_id).is_some_and(|c| opens_message_pane(c.kind))
        };
        if !opens_pane {
            return false;
        }
        self.cancel_ack();
        let old_channel = self.selected_channel;
        self.selected_channel = Some(channel_id);
        nav::record_visit(&mut self.recent_visits, channel_id);
        self.last_channel.insert(nav::place_key(self.place), channel_id);
        if !self.navigating_history {
            self.nav_history.push(nav::HistoryEntry { place: self.place, channel_id });
        }
        self.autocomplete = None;
        self.error = None;
        self.reaction_picker = None;
        self.hovered_message = None;
        self.keyboard_message = None;
        // Replies and edits belong to the old channel; stash/restore plain + token drafts.
        if old_channel != Some(channel_id) {
            self.manual_unread_channel = None;
            self.pending_jump = None;
            self.highlighted_message = None;
            let old_draft = self
                .composer_mode
                .channel_changed()
                .unwrap_or_else(|| self.current_composer_content(cx));
            if let Some(old_id) = old_channel {
                self.stash_channel_draft(old_id, old_draft);
            }
            let next_draft = self.drafts.remove(&channel_id).unwrap_or_else(|| InputContent::new(""));
            self.set_composer_later(next_draft, cx);
        } else if let Some(restore) = self.composer_mode.channel_changed() {
            self.set_composer_later(restore, cx);
        }
        let needs_load = {
            let mut state = self.backend.state();
            self.unread_anchor = state.unread_anchor(channel_id);
            state.messages.select(Some(channel_id));
            state.messages.window(channel_id).is_none_or(|w| !w.ready)
        };
        // New channel: start from an empty list that follows the tail.
        self.rows.borrow_mut().clear();
        self.row_groups.borrow_mut().clear();
        self.suppress_jump_scroll = false;
        self.scroller.update(cx, |s, cx| s.reset(0, cx));
        self.loading_history = needs_load && auto_load_latest;
        if needs_load && auto_load_latest {
            self.backend.load_messages(channel_id, None);
        }
        self.refresh(cx);
        true
    }

    pub(super) fn load_older(&mut self, cx: &mut Context<Self>) {
        if self.loading_history || !self.has_more_before {
            return;
        }
        let oldest = self.rows.borrow().iter().find(|r| r.state == MessageState::Sent).map(|r| r.id);
        let (Some(channel_id), Some(oldest)) = (self.selected_channel, oldest) else {
            return;
        };
        self.loading_history = true;
        self.backend.load_messages(channel_id, Some(oldest));
        cx.notify();
    }

    /// At the bottom of a window whose newer messages were trimmed: the
    /// page after the newest loaded message (until caught up, incoming
    /// messages are not appended to such a window).
    pub(super) fn load_newer(&mut self, cx: &mut Context<Self>) {
        if self.loading_history || !self.has_more_after {
            return;
        }
        let newest = self.rows.borrow().iter().rev().find(|r| r.state == MessageState::Sent).map(|r| r.id);
        let (Some(channel_id), Some(newest)) = (self.selected_channel, newest) else { return };
        self.loading_history = true;
        self.backend.load_newer(channel_id, newest);
        cx.notify();
    }
}
