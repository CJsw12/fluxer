//! Member list, status, read-ack timers, update checks and row syncing.

use super::*;

impl ClientView {
    /// Whether the member panel is shown (guild channel and toggled on).
    pub(crate) fn shows_member_list(&self) -> bool {
        self.member_list_open && self.member_target.is_some()
    }

    pub(crate) fn toggle_member_list(&mut self, cx: &mut Context<Self>) {
        if self.group_dm.is_some() {
            self.group_dm_panel_open = !self.group_dm_panel_open;
            if !self.group_dm_panel_open {
                self.group_dm_adding = false;
            }
            cx.notify();
            return;
        }
        self.member_list_open = !self.member_list_open;
        self.member_visible.set(None);
        self.subscribe_member_list();
        cx.notify();
    }

    /// Subscribes the shown channel's member list around the visible rows
    /// (the first page before the list reported any). The tracker skips
    /// unchanged requests, so this runs on every refresh.
    pub(super) fn subscribe_member_list(&mut self) {
        let Some((guild_id, channel_id)) = self.member_target.filter(|_| self.member_list_open) else { return };
        let (first, last) = self.member_visible.get().unwrap_or((0, 0));
        let ranges = member_list::page_ranges(first, last, &self.member_panel.borrow());
        self.member_subscriptions.subscribe(guild_id, channel_id, &ranges);
    }

    /// The member list showed rows `first..=last`.
    pub(crate) fn member_list_scrolled(&mut self, first: usize, last: usize, cx: &mut Context<Self>) {
        self.member_visible.set(Some((first, last)));
        self.subscribe_member_list();
        cx.notify();
    }

    /// User panel status dot: online -> idle -> dnd -> invisible.
    pub(crate) fn cycle_status(&mut self, cx: &mut Context<Self>) {
        let next = member_list::next_status(self.own_status);
        self.backend.set_presence(next);
        self.own_status = next;
        cx.notify();
    }

    /// Window title: "Fluxer", or "Fluxer - #channel" with a channel open;
    /// "(N) " in front while there are N unread mentions.
    pub(crate) fn title_text(&self) -> String {
        let base = if self.selected_channel.is_none() || self.channel_title.is_empty() {
            "Fluxer".to_owned()
        } else {
            format!("Fluxer - {}{}", self.channel_glyph, self.channel_title)
        };
        match self.mention_total {
            0 => base,
            n => format!("({n}) {base}"),
        }
    }

    /// Schedules a read ack when the open channel is unread and the list is
    /// at the bottom; it is sent after `ACK_DELAY` if that still holds.
    pub(crate) fn maybe_ack(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel else {
            self.cancel_ack();
            return;
        };
        if self.manual_unread_channel == Some(channel_id)
            || self.loading_history
            || self.pending_jump.is_some()
            || self.has_more_after
            || self.scroller.read(cx).is_scrolled_up()
        {
            self.cancel_ack();
            return;
        }
        let newest = self.rows.borrow().iter().rev().find(|r| r.state == MessageState::Sent).map(|r| r.id);
        let Some(newest) = newest else { return };
        let unread = self.backend.state().channel_unread(channel_id);
        if !unread.unread && unread.mention_count == 0 {
            return;
        }
        let now = cx.background_executor().now();
        if let Some(delay) = self.acks.offer(channel_id, newest, now) {
            self.start_ack_timer(delay, cx);
        }
    }

    fn start_ack_timer(&mut self, delay: std::time::Duration, cx: &mut Context<Self>) {
        self._ack_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |view, cx| view.flush_ack(cx));
        }));
    }

    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.update_status,
            crate::updater::UpdateStatus::Checking | crate::updater::UpdateStatus::Downloading { .. }
        ) {
            return;
        }
        self.update_status = crate::updater::UpdateStatus::Checking;
        cx.notify();
        let bg = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |this, cx| {
            let status = bg
                .spawn(async move {
                    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
                        return crate::updater::UpdateStatus::Error("Could not start update runtime".to_owned());
                    };
                    rt.block_on(crate::updater::check_for_updates(
                        crate::updater::DEFAULT_RELEASES_API_URL,
                        crate::updater::CURRENT_VERSION,
                    ))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.update_status = status;
                view._update_task = None;
                cx.notify();
            });
        }));
    }

    pub(crate) fn download_available_update(&mut self, cx: &mut Context<Self>) {
        let crate::updater::UpdateStatus::Available {
            latest,
            asset_name: Some(asset_name),
            asset_url: Some(asset_url),
            checksum_url,
            ..
        } = &self.update_status
        else {
            return;
        };
        let latest = latest.clone();
        let asset_name = asset_name.clone();
        let asset_url = asset_url.clone();
        let checksum_url = checksum_url.clone();
        let dest_dir = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("FluxerClient")
            .join("updates");

        self.update_status = crate::updater::UpdateStatus::Downloading {
            latest: latest.clone(),
        };
        cx.notify();
        let bg = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |this, cx| {
            let status = bg
                .spawn(async move {
                    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
                        return crate::updater::UpdateStatus::Error(
                            "Could not start update download runtime".to_owned(),
                        );
                    };
                    rt.block_on(crate::updater::download_update(
                        &latest,
                        &asset_name,
                        &asset_url,
                        checksum_url.as_deref(),
                        &dest_dir,
                    ))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.update_status = status;
                view._update_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn cancel_ack(&mut self) {
        self.acks.cancel();
        self._ack_timer = None;
    }

    fn flush_ack(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match self.acks.take_due(now) {
            Some((channel_id, message_id)) if Some(channel_id) == self.selected_channel => {
                self.backend.ack(channel_id, message_id);
                self.refresh(cx);
            }
            Some(_) => {}
            None => {
                if let Some(left) = self.acks.remaining(now) {
                    self.start_ack_timer(left, cx);
                }
            }
        }
    }

    /// Splices only the changed middle of the grouped row list so the scroll anchor
    /// survives prepends (older pages) and appends (new messages).
    pub(super) fn sync_rows(&mut self, new_rows: Vec<MessageRow>, cx: &mut Context<Self>) {
        let new_groups = ui::message_list::compute_row_groups(&new_rows);
        let (start, old_end, new_count) = {
            let old = self.rows.borrow();
            let old_groups = self.row_groups.borrow();
            let group_eq = |a: &std::ops::Range<usize>, b: &std::ops::Range<usize>| {
                old.get(a.clone()) == new_rows.get(b.clone())
            };
            let prefix = old_groups.iter().zip(&new_groups).take_while(|(a, b)| group_eq(a, b)).count();
            let max_suffix = old_groups.len().min(new_groups.len()) - prefix;
            let suffix = old_groups
                .iter()
                .rev()
                .zip(new_groups.iter().rev())
                .take(max_suffix)
                .take_while(|(a, b)| group_eq(a, b))
                .count();
            (prefix, old_groups.len() - suffix, new_groups.len() - prefix - suffix)
        };
        *self.rows.borrow_mut() = new_rows;
        *self.row_groups.borrow_mut() = new_groups;
        if start == old_end && new_count == 0 {
            return;
        }
        self.scroller.update(cx, |s, cx| {
            s.splice(start..old_end, new_count, cx);
        });
    }

    pub(super) fn set_can_send(&mut self, can_send: bool, cx: &mut Context<Self>) {
        self.can_send = can_send;
        self.composer.update(cx, |s, cx| s.set_disabled(!can_send, cx));
    }
}
