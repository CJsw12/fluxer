use super::*;

pub(super) fn render_keybind_chip_list(
    chips: &[String],
    overridden: bool,
    disabled: bool,
    theme: &Theme,
) -> AnyElement {
    let dimmed = overridden || disabled;
    let chip_bg = theme.background;
    let chip_border = theme.background_modifier_accent;

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .flex_none()
        .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips")
        .when(chips.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .italic()
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips-empty")
                    .child(l10n::tr("Unassigned")),
            )
        })
        .when(!chips.is_empty(), |d| {
            d.children(chips.iter().map(|chip| {
                div()
                    .min_w(px(24.))
                    .h(px(22.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(chip_border)
                    .bg(chip_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .when(dimmed, |c| c.opacity(0.5).line_through())
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chip")
                    .child(chip.clone())
            }))
        })
        .when(overridden, |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .italic()
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips-empty--2")
                    .child(l10n::tr("Overridden")),
            )
        })
        .when(!overridden && disabled, |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .italic()
                    .flx("user.keybinds-tab.default-shortcut-chip-list.default-chips-empty--3")
                    .child(l10n::tr("Disabled")),
            )
        })
        .into_any_element()
}

pub(super) fn render_custom_keybind_row(
    entry: &CustomKeybindEntry,
    conflict_label: Option<&str>,
    is_recording: bool,
    action_menu_open: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let id = entry.id.clone();
    let id_sel = id.clone();
    let id_action_btn = id.clone();
    let id_record_btn = id.clone();
    let id_clear_btn = id.clone();
    let id_switch = id.clone();
    let id_delete = id.clone();

    let action_display = entry
        .action
        .as_deref()
        .and_then(shortcuts::action_label)
        .or(entry.action.as_deref())
        .map(l10n::tr)
        .unwrap_or_else(|| l10n::tr("Select an action…"));

    let combo_display = if is_recording {
        l10n::tr("Press a keybind… (Esc to cancel)")
    } else if entry.combo.is_empty() {
        l10n::tr("No keybind set")
    } else {
        shortcuts::format_combo_display(&entry.combo)
    };

    let has_combo = !entry.combo.is_empty();
    let border_col = theme.background_modifier_accent;
    let recorder_border = if is_recording { theme.brand } else { border_col };

    let action_dropdown_menu = action_menu_open.then(|| {
        let id_unassigned = id.clone();
        div()
            .id(SharedString::from(format!("kb-action-menu-{id}")))
            .mt(px(4.))
            .p(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(border_col)
            .bg(theme.background)
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(200.))
            .overflow_y_scroll()
            .debug_selector({
                let id = id.clone();
                move || format!("settings-keybind-action-menu-{id}")
            })
            .child(
                div()
                    .id(SharedString::from(format!("kb-action-opt-{id_unassigned}-none")))
                    .debug_selector({
                        let id = id_unassigned.clone();
                        move || format!("settings-keybind-action-option-{id}-unassigned")
                    })
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.hover_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_set_custom_keybind_action(&id_unassigned, None, cx);
                    }))
                    .child(l10n::tr("(Unassigned)")),
            )
            .children(shortcuts::ASSIGNABLE_ACTIONS.iter().map(|&(act_id, act_label)| {
                let id_opt = id.clone();
                let selected = entry.action.as_deref() == Some(act_id);
                div()
                    .id(SharedString::from(format!("kb-action-opt-{id_opt}-{act_id}")))
                    .debug_selector({
                        let id = id_opt.clone();
                        move || format!("settings-keybind-action-option-{id}-{act_id}")
                    })
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .text_size(px(13.))
                    .text_color(theme.foreground)
                    .when(selected, |d| d.bg(theme.accent).font_weight(FontWeight::MEDIUM))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.hover_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_set_custom_keybind_action(&id_opt, Some(act_id.to_owned()), cx);
                    }))
                    .child(l10n::tr(act_label))
                    .when(selected, |d| {
                        d.child(Icon::new(AppIcon::Check).size(px(14.)).text_color(theme.foreground))
                    })
            }))
    });

    div()
        .debug_selector(move || format!("settings-keybind-custom-row-{id_sel}"))
        .flex()
        .flex_col()
        .gap(px(8.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(border_col)
        .bg(settings_card_bg(theme))
        .when(!entry.enabled, |d| d.opacity(0.62))
        .flx("user.keybinds-tab.custom-keybind-row.custom-row")
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap(px(12.))
                .flx("user.keybinds-tab.custom-keybind-row.custom-row-main")
                .child(
                    div()
                        .flex_1()
                        .min_w(px(150.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("user.keybinds-tab.custom-keybind-row.custom-field")
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .flx("user.keybinds-tab.custom-keybind-row.custom-field-label")
                                .child(l10n::tr("Action")),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("kb-action-btn-{id_action_btn}")))
                                .debug_selector({
                                    let id = id_action_btn.clone();
                                    move || format!("settings-keybind-action-{id}")
                                })
                                .h(px(34.))
                                .px(px(10.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(border_col)
                                .bg(settings_sidebar_bg(theme))
                                .cursor_pointer()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .text_size(px(13.))
                                .text_color(if entry.action.is_some() {
                                    theme.foreground
                                } else {
                                    theme.muted_foreground
                                })
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx("user.keybinds-tab.custom-keybind-row.action-dropdown")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_toggle_keybind_action_menu(id_action_btn.clone(), cx);
                                }))
                                .child(div().truncate().child(action_display))
                                .child(
                                    Icon::new(AppIcon::CaretDown)
                                        .size(px(14.))
                                        .text_color(theme.muted_foreground),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(180.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("user.keybinds-tab.custom-keybind-row.keybind-recorder.combo")
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .flx("input.keybind-recorder.field-label")
                                .child(l10n::tr("Keybind")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .id(SharedString::from(format!("kb-record-btn-{id_record_btn}")))
                                        .debug_selector({
                                            let id = id_record_btn.clone();
                                            move || format!("settings-keybind-record-{id}")
                                        })
                                        .flex_1()
                                        .min_w_0()
                                        .h(px(34.))
                                        .px(px(10.))
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(recorder_border)
                                        .bg(settings_sidebar_bg(theme))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_size(px(13.))
                                        .text_color(if is_recording {
                                            theme.brand
                                        } else if has_combo {
                                            theme.foreground
                                        } else {
                                            theme.muted_foreground
                                        })
                                        .hover(|s| s.bg(theme.hover_bg))
                                        .flx("input.keybind-recorder.recorder.button")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.settings_toggle_record_keybind(id_record_btn.clone(), window, cx);
                                        }))
                                        .child(
                                            Icon::new(if is_recording {
                                                AppIcon::Keyboard
                                            } else {
                                                AppIcon::PencilSimple
                                            })
                                            .size(px(14.))
                                            .text_color(if is_recording {
                                                theme.brand
                                            } else {
                                                theme.muted_foreground
                                            }),
                                        )
                                        .child(div().truncate().child(combo_display)),
                                )
                                .when(has_combo && !is_recording, |d| {
                                    d.child(
                                        div()
                                            .id(SharedString::from(format!("kb-clear-btn-{id_clear_btn}")))
                                            .debug_selector({
                                                let id = id_clear_btn.clone();
                                                move || format!("settings-keybind-clear-{id}")
                                            })
                                            .size(px(28.))
                                            .flex_none()
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .hover(|s| s.bg(theme.hover_bg))
                                            .flx("input.keybind-recorder.keybind-editor-popout.button.clear")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.settings_set_custom_keybind_combo(
                                                    &id_clear_btn,
                                                    String::new(),
                                                    cx,
                                                );
                                            }))
                                            .child(
                                                Icon::new(AppIcon::X)
                                                    .size(px(14.))
                                                    .text_color(theme.muted_foreground),
                                            ),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .h(px(34.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flx("user.keybinds-tab.custom-keybind-row.custom-controls")
                        .child(
                            div()
                                .debug_selector({
                                    let id = id_switch.clone();
                                    move || format!("settings-keybind-toggle-{id}")
                                })
                                .flx("user.keybinds-tab.custom-keybind-row.switch.handle-enabled")
                                .child(
                                    Switch::new(SharedString::from(format!("kb-switch-{id_switch}")))
                                        .checked(entry.enabled)
                                        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                            this.settings_set_custom_keybind_enabled(&id_switch, *checked, cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("kb-delete-btn-{id_delete}")))
                                .debug_selector({
                                    let id = id_delete.clone();
                                    move || format!("settings-keybind-delete-{id}")
                                })
                                .size(px(28.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx("user.keybinds-tab.custom-keybind-row.dots-button.open-row-menu")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_remove_custom_keybind(&id_delete, cx);
                                }))
                                .child(Icon::new(AppIcon::Trash).size(px(16.)).text_color(theme.danger)),
                        ),
                ),
        )
        .children(action_dropdown_menu)
        .when(entry.action.is_none(), |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .pl(px(2.))
                    .flx("user.keybinds-tab.custom-keybind-row.custom-hint")
                    .child(l10n::tr("Choose an action.")),
            )
        })
        .when_some(conflict_label, |d, other| {
            let id_conflict = entry.id.clone();
            d.child(
                div()
                    .debug_selector(move || format!("settings-keybind-conflict-{id_conflict}"))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .mt(px(2.))
                    .text_size(px(12.))
                    .text_color(theme.status_idle)
                    .flx("user.keybinds-tab.custom-keybind-row.conflict-warning")
                    .child(
                        Icon::new(AppIcon::WarningFill)
                            .size(px(14.))
                            .text_color(theme.status_idle),
                    )
                    .child(format!("{} {}", l10n::tr("Conflicts with"), other)),
            )
        })
        .into_any_element()
}

pub(super) fn render_default_shortcut_row(
    row: &DefaultShortcutRowSpec,
    is_last: bool,
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let row_key = row.primary_action();
    let menu_open = modal.keybinds_open_default_menu.as_deref() == Some(row_key);
    let specs = row.specs();
    let has_custom = specs.iter().any(|s| {
        view.prefs
            .data
            .custom_keybinds
            .iter()
            .any(|e| e.action.as_deref() == Some(s.action))
    });
    let any_disabled = specs
        .iter()
        .any(|s| view.prefs.is_builtin_action_disabled(s.action));
    let all_disabled = specs
        .iter()
        .all(|s| view.prefs.is_builtin_action_disabled(s.action));

    let border_col = theme.background_modifier_accent;
    let is_pair = matches!(row, DefaultShortcutRowSpec::Pair(_, _));
    let row_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.default-row"
    } else {
        "user.keybinds-tab.default-shortcut-row.default-row--2"
    };
    let label_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.default-label"
    } else {
        "user.keybinds-tab.default-shortcut-row.default-label--2"
    };
    let actions_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.default-row-actions"
    } else {
        "user.keybinds-tab.default-shortcut-row.default-row-actions--2"
    };
    let dots_flx = if is_pair {
        "user.keybinds-tab.default-shortcut-row.dots-button.open-row-menu"
    } else {
        "user.keybinds-tab.default-shortcut-row.dots-button.open-row-menu--2"
    };

    let chips_el: AnyElement = match row {
        DefaultShortcutRowSpec::Single(s) => {
            let overridden = shortcuts::is_builtin_action_overridden(&view.prefs.data, s.action);
            let disabled = view.prefs.data.disable_builtin_keybinds
                || view.prefs.is_builtin_action_disabled(s.action);
            render_keybind_chip_list(&shortcuts::chips_for_default_spec(s), overridden, disabled, theme)
        }
        DefaultShortcutRowSpec::Pair(a, b) => {
            let a_over = shortcuts::is_builtin_action_overridden(&view.prefs.data, a.action);
            let a_dis = view.prefs.data.disable_builtin_keybinds
                || view.prefs.is_builtin_action_disabled(a.action);
            let b_over = shortcuts::is_builtin_action_overridden(&view.prefs.data, b.action);
            let b_dis = view.prefs.data.disable_builtin_keybinds
                || view.prefs.is_builtin_action_disabled(b.action);
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .flex_none()
                .flx("user.keybinds-tab.default-shortcut-row.default-chips-merged")
                .child(render_keybind_chip_list(
                    &shortcuts::chips_for_default_spec(a),
                    a_over,
                    a_dis,
                    theme,
                ))
                .child(render_keybind_chip_list(
                    &shortcuts::chips_for_default_spec(b),
                    b_over,
                    b_dis,
                    theme,
                ))
                .into_any_element()
        }
    };

    let row_menu = menu_open.then(|| {
        let actions_add: Vec<&'static str> = specs.iter().map(|s| s.action).collect();
        let actions_disable: Vec<&'static str> = specs.iter().map(|s| s.action).collect();
        let actions_enable: Vec<&'static str> = specs.iter().map(|s| s.action).collect();
        let actions_reset: Vec<&'static str> = specs.iter().map(|s| s.action).collect();

        div()
            .px(px(12.))
            .pb(px(10.))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .debug_selector(move || format!("settings-keybind-default-actions-{row_key}"))
            .child(
                div()
                    .debug_selector(move || format!("settings-keybind-default-add-{row_key}"))
                    .child(
                        Button::new(SharedString::from(format!("kb-def-add-{row_key}")))
                            .ghost()
                            .small()
                            .icon(AppIcon::PencilSimple)
                            .label(if has_custom {
                                l10n::tr("Add another custom keybind")
                            } else {
                                l10n::tr("Set custom keybind")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                for act in &actions_add {
                                    this.settings_add_custom_keybind(Some((*act).to_owned()), cx);
                                }
                            })),
                    ),
            )
            .when(has_custom, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("settings-keybind-default-reset-{row_key}"))
                        .child(
                            Button::new(SharedString::from(format!("kb-def-reset-{row_key}")))
                                .danger()
                                .small()
                                .icon(AppIcon::ArrowClockwise)
                                .label(l10n::tr("Reset to built-in"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for act in &actions_reset {
                                        this.settings_remove_custom_keybinds_for_action(act, cx);
                                    }
                                })),
                        ),
                )
            })
            .when(any_disabled, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("settings-keybind-default-enable-{row_key}"))
                        .child(
                            Button::new(SharedString::from(format!("kb-def-enable-{row_key}")))
                                .ghost()
                                .small()
                                .icon(AppIcon::ArrowClockwise)
                                .label(l10n::tr("Re-enable built-in keybind"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for act in &actions_enable {
                                        this.settings_enable_builtin_action(act, cx);
                                    }
                                })),
                        ),
                )
            })
            .when(!has_custom && !all_disabled, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("settings-keybind-default-disable-{row_key}"))
                        .child(
                            Button::new(SharedString::from(format!("kb-def-disable-{row_key}")))
                                .ghost()
                                .small()
                                .icon(AppIcon::EyeSlash)
                                .label(l10n::tr("Disable built-in keybind"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for act in &actions_disable {
                                        this.settings_disable_builtin_action(act, cx);
                                    }
                                })),
                        ),
                )
            })
    });

    div()
        .flex()
        .flex_col()
        .when(!is_last, |d| d.border_b_1().border_color(border_col))
        .when(is_last, |d| d.border_b_0())
        .flx(row_flx)
        .child(
            div()
                .min_h(px(44.))
                .p(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(theme.foreground)
                        .flx(label_flx)
                        .child(l10n::tr(row.label())),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flex_none()
                        .flx(actions_flx)
                        .child(chips_el)
                        .child(
                            div()
                                .id(SharedString::from(format!("kb-def-menu-{row_key}")))
                                .debug_selector(move || format!("settings-keybind-default-menu-{row_key}"))
                                .size(px(28.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(menu_open, |d| d.bg(theme.hover_bg))
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx(dots_flx)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_toggle_keybind_default_menu(row_key.to_owned(), cx);
                                }))
                                .child(
                                    Icon::new(AppIcon::DotsThreeVerticalBold)
                                        .size(px(16.))
                                        .text_color(theme.muted_foreground),
                                ),
                        ),
                ),
        )
        .children(row_menu)
        .into_any_element()
}

pub(super) fn render_keybinds_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let raw_query = modal.keybinds_search.read(cx).value().trim().to_owned();
    let query = raw_query.to_lowercase();
    let border_col = theme.background_modifier_accent;

    let custom_keybinds = &view.prefs.data.custom_keybinds;
    let conflicts = shortcuts::find_custom_conflicts(custom_keybinds);
    let filtered_custom: Vec<&CustomKeybindEntry> = custom_keybinds
        .iter()
        .filter(|entry| {
            if query.is_empty() {
                return true;
            }
            let label = entry
                .action
                .as_deref()
                .and_then(shortcuts::action_label)
                .or(entry.action.as_deref())
                .unwrap_or_default()
                .to_lowercase();
            if label.contains(&query) {
                return true;
            }
            if entry.combo.to_lowercase().contains(&query) {
                return true;
            }
            shortcuts::format_combo_display(&entry.combo)
                .to_lowercase()
                .contains(&query)
        })
        .collect();

    let has_any_overrides = !custom_keybinds.is_empty()
        || view.prefs.data.disable_builtin_keybinds
        || !view.prefs.data.disabled_builtin_actions.is_empty();

    let search_row = div()
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .flx("user.keybinds-tab.search-row")
        .child(
            div()
                .w_full()
                .h(px(44.))
                .pl(px(36.))
                .pr(px(16.))
                .rounded(px(6.))
                .border_1()
                .border_color(border_col)
                .bg(settings_sidebar_bg(theme))
                .flex()
                .flex_row()
                .items_center()
                .debug_selector(|| "settings-keybinds-search".to_owned())
                .flx("user.keybinds-tab.input.set-search-query")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&modal.keybinds_search).appearance(false)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(12.))
                .top(px(14.))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .flx("user.keybinds-tab.magnifying-glass-icon")
                .child(
                    Icon::new(AppIcon::MagnifyingGlass)
                        .size(px(16.))
                        .text_color(theme.muted_foreground),
                ),
        );

    let custom_header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .flx("user.keybinds-tab.custom-keybinds-list.custom-header")
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w_0()
                .flx("user.keybinds-tab.custom-keybinds-list.custom-header-text")
                .child(
                    div()
                        .text_size(px(15.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx("user.keybinds-tab.custom-keybinds-list.custom-title")
                        .child(l10n::tr("Custom keybinds")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(theme.muted_foreground)
                        .flx("user.keybinds-tab.custom-keybinds-list.custom-subtitle")
                        .child(l10n::tr("Keybinds are paused while recording.")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flex_none()
                .flx("user.keybinds-tab.custom-keybinds-list.custom-header-actions")
                .when(has_any_overrides, |d| {
                    d.child(
                        div()
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .debug_selector(|| "settings-keybinds-reset-all".to_owned())
                            .flx("user.keybinds-tab.custom-keybinds-list.button.reset-all")
                            .child(
                                Button::new("settings-keybinds-reset-all")
                                    .ghost()
                                    .small()
                                    .h(px(34.))
                                    .icon(AppIcon::ArrowClockwise)
                                    .label(l10n::tr("Reset to defaults"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_set_keybinds_confirm_reset(true, cx);
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .id("settings-keybinds-add")
                        .debug_selector(|| "settings-keybinds-add".to_owned())
                        .h(px(34.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.brand_button_border)
                        .bg(theme.brand)
                        .hover(|s| s.opacity(0.9))
                        .cursor_pointer()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .flx("user.keybinds-tab.custom-keybinds-list.button.add")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings_add_custom_keybind(None, cx);
                        }))
                        .child(
                            div()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flx("user.keybinds-tab.custom-keybinds-list.plus-icon")
                                .child(
                                    Icon::new(AppIcon::Plus)
                                        .size(px(14.))
                                        .text_color(theme.on_fill),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.on_fill)
                                .child(l10n::tr("Add keybind")),
                        ),
                ),
        );

    let reset_confirm_banner = modal.keybinds_confirm_reset.then(|| {
        div()
            .debug_selector(|| "settings-keybinds-reset-confirm".to_owned())
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.danger)
            .bg(settings_card_bg(theme))
            .flex()
            .flex_col()
            .gap(px(8.))
            .flx("user.keybinds-tab.handle-reset-all.confirm-modal")
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(l10n::tr("Reset all keybinds?")),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr(
                        "This removes every custom keybind and re-enables all built-in keybinds. This cannot be undone.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .debug_selector(|| "settings-keybinds-reset-cancel".to_owned())
                            .child(
                                Button::new("settings-keybinds-reset-cancel")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_set_keybinds_confirm_reset(false, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-keybinds-reset-confirm-btn".to_owned())
                            .child(
                                Button::new("settings-keybinds-reset-confirm-btn")
                                    .danger()
                                    .small()
                                    .label(l10n::tr("Reset to defaults"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_reset_all_keybinds(cx);
                                    })),
                            ),
                    ),
            )
    });

    let shortcut_options = div()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(border_col)
        .bg(settings_card_bg(theme))
        .flex()
        .flex_col()
        .gap(px(8.))
        .flx("user.keybinds-tab.custom-keybinds-list.shortcut-options")
        .child(
            div()
                .h(px(24.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .debug_selector(|| "settings-keybinds-disable-builtins".to_owned())
                .flx("user.keybinds-tab.custom-keybinds-list.switch.set-disable-builtin-keybinds")
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(l10n::tr("Disable built-ins")),
                )
                .child(
                    Switch::new("settings-keybinds-disable-builtins")
                        .checked(view.prefs.data.disable_builtin_keybinds)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.settings_set_disable_builtin_keybinds(*checked, cx);
                        })),
                ),
        )
        .child(
            div()
                .h(px(24.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .debug_selector(|| "settings-keybinds-sync".to_owned())
                .flx("user.keybinds-tab.custom-keybinds-list.switch.set-sync-across-devices")
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(l10n::tr("Sync keybinds")),
                )
                .child(
                    Switch::new("settings-keybinds-sync")
                        .checked(modal.keybinds_sync_across_devices)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.settings_set_sync_keybinds(*checked, cx);
                        })),
                ),
        );

    let custom_list = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .flx("user.keybinds-tab.custom-keybinds-list.custom-list")
        .when(custom_keybinds.is_empty(), |d| {
            d.child(
                div()
                    .py(px(8.))
                    .text_center()
                    .text_size(px(13.))
                    .line_height(px(19.5))
                    .text_color(theme.muted_foreground)
                    .debug_selector(|| "settings-keybinds-custom-empty".to_owned())
                    .flx("user.keybinds-tab.custom-keybinds-list.empty-state")
                    .child(l10n::tr("No custom keybinds yet.")),
            )
        })
        .when(!custom_keybinds.is_empty() && filtered_custom.is_empty(), |d| {
            d.child(
                div()
                    .py(px(8.))
                    .text_center()
                    .text_size(px(13.))
                    .line_height(px(19.5))
                    .text_color(theme.muted_foreground)
                    .debug_selector(|| "settings-keybinds-custom-no-match".to_owned())
                    .flx("user.keybinds-tab.custom-keybinds-list.empty-state--2")
                    .child(l10n::tr("No custom keybinds match that search.")),
            )
        })
        .when(!filtered_custom.is_empty(), |d| {
            d.children(filtered_custom.into_iter().map(|entry| {
                let conflict = conflicts.get(&entry.id).map(String::as_str);
                let is_rec = modal.keybinds_recording_id.as_deref() == Some(entry.id.as_str());
                let menu_open = modal.keybinds_open_action_menu.as_deref() == Some(entry.id.as_str());
                render_custom_keybind_row(entry, conflict, is_rec, menu_open, theme, cx)
            }))
        });

    let custom_section = div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .flx("user.keybinds-tab.custom-keybinds-list.custom-section")
        .child(custom_header)
        .children(reset_confirm_banner)
        .child(shortcut_options)
        .child(custom_list);

    let filtered_sections: Vec<(KeybindSection, Vec<DefaultShortcutRowSpec>)> = KeybindSection::ALL
        .iter()
        .filter_map(|&section| {
            let rows: Vec<DefaultShortcutRowSpec> = shortcuts::default_shortcut_rows(section)
                .into_iter()
                .filter(|row| {
                    if query.is_empty() {
                        return true;
                    }
                    if row.label().to_lowercase().contains(&query) {
                        return true;
                    }
                    row.specs().iter().any(|s| {
                        s.combo.to_lowercase().contains(&query)
                            || shortcuts::chips_for_default_spec(s)
                                .iter()
                                .any(|c| c.to_lowercase().contains(&query))
                    })
                })
                .collect();
            (!rows.is_empty()).then_some((section, rows))
        })
        .collect();

    let defaults_list = if filtered_sections.is_empty() && !query.is_empty() {
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .flx("user.keybinds-tab.default-keybinds-list.defaults-list")
            .child(
                div()
                    .py(px(8.))
                    .text_center()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .debug_selector(|| "settings-keybinds-defaults-no-match".to_owned())
                    .flx("user.keybinds-tab.default-keybinds-list.empty-state")
                    .child(l10n::tr("No keybinds match that search.")),
            )
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .mb(px(8.))
            .flx("user.keybinds-tab.default-keybinds-list.defaults-list--2")
            .children(filtered_sections.into_iter().map(|(section, rows)| {
                let count = rows.len();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .flx("user.keybinds-tab.default-keybinds-list.defaults-section")
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .flx("user.keybinds-tab.default-keybinds-list.defaults-section-title")
                            .child(l10n::tr(section.label())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(border_col)
                            .bg(theme.background_secondary_lighter)
                            .overflow_hidden()
                            .flx("user.keybinds-tab.default-keybinds-list.defaults-section-rows")
                            .children(rows.iter().enumerate().map(|(idx, row)| {
                                render_default_shortcut_row(row, idx + 1 == count, view, modal, theme, cx)
                            })),
                    )
            }))
    };

    div()
        .debug_selector(|| "settings-keybinds-list".to_owned())
        .flex()
        .flex_col()
        .gap(px(16.))
        .flx("user.keybinds-tab.container")
        .child(search_row)
        .child(custom_section)
        .child(defaults_list)
        .into_any_element()
}
