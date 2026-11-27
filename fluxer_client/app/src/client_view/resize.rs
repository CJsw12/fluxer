//! Voice stage layout and the sidebar/voice/emoji-picker resize handles.

use super::*;

impl ClientView {
    pub(crate) fn open_voice_stage(&mut self, cx: &mut Context<Self>) {
        if self.voice.channel_id.is_some()
            && self.voice.state != crate::backend::voice::VoiceConnectionState::Disconnected
        {
            self.voice_stage_open = true;
            cx.notify();
        }
    }

    pub(crate) fn close_voice_stage(&mut self, cx: &mut Context<Self>) {
        if self.voice_stage_open {
            self.voice_stage_open = false;
            self.voice_popout = None;
            cx.notify();
        }
    }

    pub(crate) fn toggle_voice_stage_expanded(&mut self, cx: &mut Context<Self>) {
        self.voice_stage_expanded = !self.voice_stage_expanded;
        cx.notify();
    }

    pub(crate) fn effective_sidebar_width(&self) -> f32 {
        self.sidebar_width.clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32)
    }

    pub(crate) fn max_voice_call_height(&self, window: &Window) -> f32 {
        let vh = f32::from(window.viewport_size().height);
        // Reserve 32px desktop title bar + ~178px minimum split chat/composer space, capped at official 1049px.
        ((vh - 32.0 - 178.0).min(VOICE_CALL_HEIGHT_MAX as f32)).max(VOICE_CALL_HEIGHT_MIN as f32)
    }

    pub(crate) fn effective_voice_call_height(&self, window: &Window) -> f32 {
        self.voice_call_height.clamp(VOICE_CALL_HEIGHT_MIN as f32, self.max_voice_call_height(window))
    }

    pub(super) fn max_emoji_picker_bounds(&self, window: &Window) -> (f32, f32) {
        let viewport = window.viewport_size();
        let vw = f32::from(viewport.width);
        let vh = f32::from(viewport.height);
        let min_top = if vh >= 700. { 44. } else { 16. };
        let anchor = match &self.overlay {
            crate::ui::overlay::Overlay::EmojiPicker(picker) => picker.anchor,
            _ => gpui_kit::point(px((vw - 16.).max(0.)), px((vh - 16.).max(0.))),
        };
        let min_right = (EMOJI_PICKER_WIDTH_MIN as f32 + 16.).min(vw - 16.);
        let max_right = (vw - 16.).max(EMOJI_PICKER_WIDTH_MIN as f32 + 16.);
        let eff_right = f32::from(anchor.x).clamp(min_right, max_right);
        let min_bottom = (min_top + EMOJI_PICKER_HEIGHT_MIN as f32).min(vh - 16.);
        let max_bottom = (vh - 16.).max(min_top + EMOJI_PICKER_HEIGHT_MIN as f32);
        let eff_bottom = f32::from(anchor.y).clamp(min_bottom, max_bottom);
        let max_w = (eff_right - 16.).clamp(EMOJI_PICKER_WIDTH_MIN as f32, EMOJI_PICKER_WIDTH_MAX as f32);
        let max_h = (eff_bottom - min_top).clamp(EMOJI_PICKER_HEIGHT_MIN as f32, EMOJI_PICKER_HEIGHT_MAX as f32);
        (max_w, max_h)
    }

    pub(super) fn apply_emoji_picker_size(&mut self, width: u16, height: u16, persist: bool, _cx: &mut Context<Self>) {
        self.emoji_picker_width = width as f32;
        self.emoji_picker_height = height as f32;
        if let crate::ui::overlay::Overlay::EmojiPicker(picker) = &mut self.overlay {
            picker.set_width(width as f32);
        }
        if persist {
            self.prefs.set_emoji_picker_size(width, height);
        }
    }

    pub(crate) fn set_sidebar_resize_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.sidebar_resize_hovered != hovered {
            self.sidebar_resize_hovered = hovered;
            cx.notify();
        }
    }

    pub(crate) fn set_voice_resize_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.voice_resize_hovered != hovered {
            self.voice_resize_hovered = hovered;
            cx.notify();
        }
    }

    pub(crate) fn begin_sidebar_resize(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        window.focus(&self.sidebar_resize_focus, cx);
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.sidebar_width = SIDEBAR_WIDTH_DEFAULT as f32;
            self.prefs.set_sidebar_width(SIDEBAR_WIDTH_DEFAULT);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::Sidebar {
            start_x: f32::from(ev.position.x),
            start_width: self.effective_sidebar_width(),
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn begin_voice_height_resize(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        window.focus(&self.voice_resize_focus, cx);
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.voice_call_height = VOICE_CALL_HEIGHT_DEFAULT as f32;
            self.prefs.set_voice_call_height(VOICE_CALL_HEIGHT_DEFAULT);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::VoiceCallHeight {
            start_y: f32::from(ev.position.y),
            start_height: self.effective_voice_call_height(window),
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn begin_emoji_picker_resize(
        &mut self,
        edge: ResizeEdge,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev.button != MouseButton::Left {
            return;
        }
        window.focus(&self.emoji_resize_focus, cx);
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.apply_emoji_picker_size(EMOJI_PICKER_WIDTH_DEFAULT, EMOJI_PICKER_HEIGHT_DEFAULT, true, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::EmojiPicker {
            edge,
            start_x: f32::from(ev.position.x),
            start_y: f32::from(ev.position.y),
            start_w: self.emoji_picker_width,
            start_h: self.emoji_picker_height,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn begin_pins_popout_resize(
        &mut self,
        edge: ResizeEdge,
        ev: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if ev.button != MouseButton::Left {
            return;
        }
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.pins_popout_width = crate::ui::channel_popouts::PINS_POPOUT_WIDTH_DEFAULT;
            self.pins_popout_height = crate::ui::channel_popouts::PINS_POPOUT_HEIGHT_DEFAULT;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::PinsPopout {
            edge,
            start_x: f32::from(ev.position.x),
            start_y: f32::from(ev.position.y),
            start_w: self.pins_popout_width,
            start_h: self.pins_popout_height,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn begin_inbox_popout_resize(
        &mut self,
        edge: ResizeEdge,
        ev: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if ev.button != MouseButton::Left {
            return;
        }
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.inbox_popout_width = crate::ui::channel_popouts::INBOX_POPOUT_WIDTH_DEFAULT;
            self.inbox_popout_height = crate::ui::channel_popouts::INBOX_POPOUT_HEIGHT_DEFAULT;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::InboxPopout {
            edge,
            start_x: f32::from(ev.position.x),
            start_y: f32::from(ev.position.y),
            start_w: self.inbox_popout_width,
            start_h: self.inbox_popout_height,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn handle_resize_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active_resize else {
            return;
        };
        match active {
            ActiveResize::Sidebar { start_x, start_width } => {
                let next = (start_width + (f32::from(ev.position.x) - start_x))
                    .round()
                    .clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32);
                if (next - self.sidebar_width).abs() > 0.5 {
                    self.sidebar_width = next;
                    cx.notify();
                }
            }
            ActiveResize::VoiceCallHeight { start_y, start_height } => {
                let max_h = self.max_voice_call_height(window);
                let next = (start_height + (f32::from(ev.position.y) - start_y))
                    .round()
                    .clamp(VOICE_CALL_HEIGHT_MIN as f32, max_h);
                if (next - self.voice_call_height).abs() > 0.5 {
                    self.voice_call_height = next;
                    cx.notify();
                }
            }
            ActiveResize::EmojiPicker {
                edge,
                start_x,
                start_y,
                start_w,
                start_h,
            } => {
                let (max_w, max_h) = self.max_emoji_picker_bounds(window);
                let next_w = if matches!(edge, ResizeEdge::Left | ResizeEdge::TopLeft) {
                    let dx = start_x - f32::from(ev.position.x);
                    snap_emoji_picker_width(start_w + dx, max_w)
                } else {
                    self.emoji_picker_width.round() as u16
                };
                let next_h = if matches!(edge, ResizeEdge::Top | ResizeEdge::TopLeft) {
                    let dy = start_y - f32::from(ev.position.y);
                    (start_h + dy).round().clamp(EMOJI_PICKER_HEIGHT_MIN as f32, max_h) as u16
                } else {
                    self.emoji_picker_height.round() as u16
                };
                if (next_w as f32 - self.emoji_picker_width).abs() > 0.5
                    || (next_h as f32 - self.emoji_picker_height).abs() > 0.5
                {
                    self.apply_emoji_picker_size(next_w, next_h, false, cx);
                    cx.notify();
                }
            }
            ActiveResize::PinsPopout {
                edge,
                start_x,
                start_y,
                start_w,
                start_h,
            } => {
                let viewport = window.viewport_size();
                let max_w = (f32::from(viewport.width) - 32.).max(crate::ui::channel_popouts::PINS_POPOUT_WIDTH_MIN);
                let max_h = (f32::from(viewport.height) - 60.).max(crate::ui::channel_popouts::PINS_POPOUT_HEIGHT_MIN);
                let next_w = if matches!(edge, ResizeEdge::Left | ResizeEdge::BottomLeft) {
                    (start_w + (start_x - f32::from(ev.position.x)))
                        .round()
                        .clamp(crate::ui::channel_popouts::PINS_POPOUT_WIDTH_MIN, max_w)
                } else {
                    self.pins_popout_width
                };
                let next_h = if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomLeft) {
                    (start_h + (f32::from(ev.position.y) - start_y))
                        .round()
                        .clamp(crate::ui::channel_popouts::PINS_POPOUT_HEIGHT_MIN, max_h)
                } else {
                    self.pins_popout_height
                };
                if (next_w - self.pins_popout_width).abs() > 0.5
                    || (next_h - self.pins_popout_height).abs() > 0.5
                {
                    self.pins_popout_width = next_w;
                    self.pins_popout_height = next_h;
                    cx.notify();
                }
            }
            ActiveResize::InboxPopout {
                edge,
                start_x,
                start_y,
                start_w,
                start_h,
            } => {
                let viewport = window.viewport_size();
                let max_w = (f32::from(viewport.width) - 32.).max(crate::ui::channel_popouts::INBOX_POPOUT_WIDTH_MIN);
                let max_h = (f32::from(viewport.height) - 60.).max(crate::ui::channel_popouts::INBOX_POPOUT_HEIGHT_MIN);
                let next_w = if matches!(edge, ResizeEdge::Left | ResizeEdge::BottomLeft) {
                    (start_w + (start_x - f32::from(ev.position.x)))
                        .round()
                        .clamp(crate::ui::channel_popouts::INBOX_POPOUT_WIDTH_MIN, max_w)
                } else {
                    self.inbox_popout_width
                };
                let next_h = if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomLeft) {
                    (start_h + (f32::from(ev.position.y) - start_y))
                        .round()
                        .clamp(crate::ui::channel_popouts::INBOX_POPOUT_HEIGHT_MIN, max_h)
                } else {
                    self.inbox_popout_height
                };
                if (next_w - self.inbox_popout_width).abs() > 0.5
                    || (next_h - self.inbox_popout_height).abs() > 0.5
                {
                    self.inbox_popout_width = next_w;
                    self.inbox_popout_height = next_h;
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.active_resize.take() else {
            return;
        };
        match active {
            ActiveResize::Sidebar { .. } => {
                self.prefs.set_sidebar_width(self.effective_sidebar_width().round() as u16);
            }
            ActiveResize::VoiceCallHeight { .. } => {
                self.prefs.set_voice_call_height(self.voice_call_height.round() as u16);
            }
            ActiveResize::EmojiPicker { .. } => {
                self.prefs.set_emoji_picker_size(
                    self.emoji_picker_width.round() as u16,
                    self.emoji_picker_height.round() as u16,
                );
            }
            ActiveResize::PinsPopout { .. } | ActiveResize::InboxPopout { .. } => {}
        }
        cx.notify();
    }

    pub(super) fn render_sidebar_resize_handle(&self, sidebar_w: f32, rail_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let dragging = matches!(self.active_resize, Some(ActiveResize::Sidebar { .. }));
        let handle_bottom = if !matches!(self.voice.state, crate::backend::voice::VoiceConnectionState::Disconnected) {
            261.0
        } else {
            66.0
        };
        let (indicator_w, indicator_opacity) = if dragging {
            (6.0, 1.0)
        } else if self.sidebar_resize_hovered {
            (4.0, 0.92)
        } else {
            (2.0, 0.0)
        };
        div()
            .id("sidebar-resize-handle")
            .debug_selector(|| "sidebar-resize-handle".to_owned())
            .aria_label(l10n::tr("Resize sidebar"))
            .flx("app.sidebar-resize-handle")
            .track_focus(&self.sidebar_resize_focus)
            .absolute()
            .left(px(rail_width + 1. + sidebar_w - 3.))
            .top(px(1.))
            .bottom(px(handle_bottom))
            .w(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .on_hover(cx.listener(|v, hovered, _, cx| v.set_sidebar_resize_hover(*hovered, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_sidebar_resize))
            .on_key_down(cx.listener(Self::handle_sidebar_resize_key))
            .child(
                div()
                    .debug_selector(|| "sidebar-resize-indicator".to_owned())
                    .flx("app.sidebar-resize-handle.indicator")
                    .w(px(indicator_w))
                    .h_full()
                    .rounded_full()
                    .bg(rgb(0x69686d))
                    .opacity(indicator_opacity),
            )
            .into_any_element()
    }
}
