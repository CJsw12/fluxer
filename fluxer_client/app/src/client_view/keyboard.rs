//! `ClientView` actions behind the keyboard: channel/guild navigation and
//! read marking (`crate::shortcuts`), the quick switcher (Ctrl+K) and the
//! composer's autocomplete popup.

use fluxer_client_core::search::switcher::{SwitcherContext, SwitcherItem, SwitcherTarget, quick_switcher_results};
use fluxer_client_core::state::Snowflake;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, EntityInputHandler as _, Focusable as _, ScrollHandle, Window};

use super::ClientView;
use crate::accounts::now_ms;
use crate::nav::{self, ChannelFilter, place_key};
use crate::ui::autocomplete;
use crate::ui::composer_text;
use crate::ui::overlay::Overlay;
use crate::ui::quick_switcher::QuickSwitcher;
use crate::view_model::Place;

/// Navigation and read marking.
impl ClientView {
    /// Alt+Up/Down: the previous/next channel of the guild, or of the DM
    /// list (which has a "no channel" slot like the web client).
    pub(crate) fn cycle_channel(&mut self, delta: i32, cx: &mut Context<Self>) {
        match self.place {
            Place::Guild(guild_id) => {
                let list = nav::guild_channels(&self.backend.state(), guild_id);
                if let Some(id) = nav::cycle(&list, self.selected_channel, delta) {
                    self.select_channel(id, cx);
                }
            }
            Place::Home => {
                let list = nav::dm_channels(&self.backend.state());
                match nav::cycle_slots(&list, self.selected_channel, delta) {
                    Some(id) => self.select_channel(id, cx),
                    None => self.clear_selection(cx),
                }
            }
        }
    }

    /// Alt+Shift+Up/Down (unread) and Ctrl+Alt+Shift+Up/Down (mentions):
    /// channels of the current guild only.
    pub(crate) fn cycle_filtered(&mut self, filter: ChannelFilter, delta: i32, cx: &mut Context<Self>) {
        let Place::Guild(guild_id) = self.place else { return };
        let list = nav::filtered_guild_channels(&self.backend.state(), guild_id, filter);
        if let Some(id) = nav::cycle(&list, self.selected_channel, delta) {
            self.select_channel(id, cx);
        }
    }

    /// Ctrl+Alt+Up/Down: the previous/next guild, Home being slot 0.
    pub(crate) fn cycle_guild(&mut self, delta: i32, cx: &mut Context<Self>) {
        let list = nav::guilds(&self.backend.state());
        let next = nav::cycle_slots(&list, place_key(self.place), delta);
        self.select_place(next.map_or(Place::Home, Place::Guild), cx);
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.cancel_ack();
        self.selected_channel = None;
        self.autocomplete = None;
        if let Some(restore) = self.composer_mode.channel_changed() {
            self.set_composer_later(restore, cx);
        }
        self.backend.state().messages.select(None);
        self.refresh(cx);
    }

    /// Esc: acks the open channel up to its newest message when it is
    /// unread, and jumps to the present (`ESCAPE_PRESSED`).
    pub(crate) fn mark_channel_read(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel else { return };
        let last = {
            let state = self.backend.state();
            let unread = state.channel_unread(channel_id);
            if !unread.unread && unread.mention_count == 0 {
                return;
            }
            state.channels.get(&channel_id).and_then(|c| c.last_message_id)
        };
        let Some(last) = last else { return };
        self.cancel_ack();
        self.backend.ack(channel_id, last);
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        self.refresh(cx);
    }

    /// Shift+Esc: acks every unread channel of the current guild.
    pub(crate) fn mark_guild_read(&mut self, cx: &mut Context<Self>) {
        let Place::Guild(guild_id) = self.place else { return };
        let acks = nav::guild_unread_acks(&self.backend.state(), guild_id);
        if acks.is_empty() {
            return;
        }
        self.cancel_ack();
        self.backend.ack_many(&acks);
        self.refresh(cx);
    }

    /// Opens a channel in its place (DMs: Home).
    pub(crate) fn open_channel(&mut self, guild_id: Option<Snowflake>, channel_id: Snowflake, cx: &mut Context<Self>) {
        let place = guild_id.map_or(Place::Home, Place::Guild);
        if self.place != place {
            self.place = place;
            if let Some(id) = guild_id {
                self.backend.subscribe_guild(id);
            }
        }
        self.select_channel(channel_id, cx);
        // Voice channels do not open, but the place changed.
        self.refresh(cx);
    }
}

/// Quick switcher.
impl ClientView {
    pub(crate) fn toggle_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay.is_open() {
            self.close_quick_switcher(window, cx);
        } else {
            self.open_quick_switcher(window, cx);
        }
    }

    pub(crate) fn open_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Where would you like to go?"));
        let subscription = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                this.quick_switcher_query_changed(cx);
            }
        });
        let mut sw = QuickSwitcher {
            input: input.clone(),
            items: Vec::new(),
            selected: None,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        };
        sw.set_items(self.switcher_results(""));
        self.overlay = Overlay::QuickSwitcher(sw);
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn close_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.overlay.is_open() {
            return;
        }
        self.overlay = Overlay::None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn switcher_results(&self, query: &str) -> Vec<SwitcherItem> {
        let ctx = SwitcherContext {
            recent_visits: self.recent_visits.clone(),
            current_channel: self.selected_channel,
            current_guild: place_key(self.place),
            now_ms: i64::try_from(now_ms()).unwrap_or(i64::MAX),
        };
        quick_switcher_results(&self.backend.state(), &ctx, query)
    }

    fn quick_switcher_query_changed(&mut self, cx: &mut Context<Self>) {
        let Overlay::QuickSwitcher(sw) = &self.overlay else { return };
        let query = sw.input.read(cx).value().to_string();
        let items = self.switcher_results(&query);
        if let Overlay::QuickSwitcher(sw) = &mut self.overlay {
            sw.set_items(items);
        }
        cx.notify();
    }

    pub(crate) fn quick_switcher_move(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Overlay::QuickSwitcher(sw) = &mut self.overlay {
            sw.move_selection(delta);
            cx.notify();
        }
    }

    pub(crate) fn quick_switcher_hover(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Overlay::QuickSwitcher(sw) = &mut self.overlay
            && sw.selected != Some(ix)
            && matches!(sw.items.get(ix), Some(SwitcherItem::Result(_)))
        {
            sw.selected = Some(ix);
            cx.notify();
        }
    }

    /// Opens row `ix` (or the selected row) and closes the switcher.
    pub(crate) fn quick_switcher_accept(&mut self, ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::QuickSwitcher(sw) = &mut self.overlay else { return };
        if ix.is_some() {
            sw.selected = ix;
        }
        let Some(target) = sw.selected_target().cloned() else { return };
        self.overlay = Overlay::None;
        self.open_target(target, cx);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Navigates to a switcher result. A person without a DM channel gets
    /// one from `POST /users/@me/channels`; `UiEvent::DmOpened` opens it.
    pub(crate) fn open_target(&mut self, target: SwitcherTarget, cx: &mut Context<Self>) {
        match target {
            SwitcherTarget::User { dm_channel_id: Some(channel_id), .. } | SwitcherTarget::GroupDm { channel_id } => {
                self.open_channel(None, channel_id, cx);
            }
            SwitcherTarget::User { user_id, dm_channel_id: None } => self.backend.open_dm(user_id),
            SwitcherTarget::TextChannel { channel_id, guild_id }
            | SwitcherTarget::VoiceChannel { channel_id, guild_id } => self.open_channel(guild_id, channel_id, cx),
            SwitcherTarget::Guild { guild_id } => self.select_place(Place::Guild(guild_id), cx),
        }
    }
}

/// Composer autocomplete.
impl ClientView {
    /// Recomputes the popup from the text up to the cursor (input changes).
    pub(super) fn update_autocomplete(&mut self, cx: &mut Context<Self>) {
        let next = self.selected_channel.and_then(|channel_id| {
            let text = composer_text::text_before_cursor(self.composer.read(cx));
            let state = self.backend.state();
            autocomplete::compute(&state, channel_id, &text, &self.frecency.data, now_ms())
        });
        if next != self.autocomplete {
            self.autocomplete = next;
            self.autocomplete_scroll.scroll_to_item(0);
            cx.notify();
        }
    }

    pub(crate) fn autocomplete_move(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Some(ac) = &mut self.autocomplete {
            ac.move_selection(delta);
            // Row `selected` is child `selected + 1` (after the heading).
            self.autocomplete_scroll.scroll_to_item(ac.selected + 1);
            cx.notify();
        }
    }

    pub(crate) fn autocomplete_hover(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(ac) = &mut self.autocomplete
            && ac.selected != ix
            && ix < ac.items.len()
        {
            ac.selected = ix;
            cx.notify();
        }
    }

    /// Replaces the trigger and query with item `ix` plus a space. A
    /// popup whose range no longer ends at the cursor (the text was
    /// replaced without a change event) is dropped instead.
    pub(crate) fn autocomplete_accept(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ac) = self.autocomplete.take() else { return };
        let Some(item) = ac.items.get(ix) else { return };
        let still_valid = {
            let input = self.composer.read(cx);
            ac.range.end == input.cursor() && input.value().get(ac.range.clone()).is_some()
        };
        if !still_valid {
            cx.notify();
            return;
        }
        composer_text::replace_range(&self.composer, ac.range.clone(), &item.insert, window, cx);
        if let Some(key) = &item.emoji_key {
            self.frecency.record(key, now_ms());
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Down moves in the popup; otherwise the input handles it.
    pub(super) fn on_down(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.autocomplete.is_none() || self.composing(window, cx) {
            return false;
        }
        self.autocomplete_move(1, cx);
        true
    }

    /// Tab / Enter pick the selected suggestion (Enter does not send then);
    /// during IME composition they only commit it, popup or not.
    pub(super) fn on_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.composing(window, cx) {
            // The key only ends the composition (`isComposing` keys are
            // ignored by the web composer): no pick, no send, and Tab does
            // not move focus out. The popup, recomputed from the committed
            // text, picks on the next press.
            self.composer.update(cx, |s, cx| s.unmark_text(window, cx));
            return true;
        }
        let Some(selected) = self.autocomplete.as_ref().map(|a| a.selected) else { return false };
        self.autocomplete_accept(selected, window, cx);
        true
    }
}
