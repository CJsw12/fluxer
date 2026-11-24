//! Pure emoji picker layout: section order, row splitting and keyboard
//! movement. `F/emoji/components/popouts/EmojiPickerPopout.tsx`,
//! `pickers/shared/PickerRows.ts:44-97`, `shared/usePickerGridKeyboard.ts`.

use fluxer_client_core::search::emoji::{
    PICKER_FRECENT_LIMIT, EmojiCategory, EmojiChoice, EmojiFrecency, custom_choices_by_guild, frecency_choices,
    unicode_emojis_in,
};
use fluxer_client_core::state::{ClientState, Snowflake};

use crate::ui::l10n;

/// Grid width of the picker (`EmojiPickerConstants.ts:19-34`; min 5, default 9).
#[cfg(test)]
pub const COLUMNS: usize = 9;

/// Base emoji plus the five skin tones, in menu order.
pub const SKIN_TONES: [&str; 6] = ["👋", "👋🏻", "👋🏼", "👋🏽", "👋🏾", "👋🏿"];

/// Where a picked emoji goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerTarget {
    Composer,
    Reaction { channel_id: Snowflake, message_id: Snowflake },
}

/// What a picker section lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    FrequentlyUsed,
    Guild(Snowflake),
    Category(EmojiCategory),
}

/// One header plus its choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerSection {
    pub kind: SectionKind,
    pub title: String,
    pub choices: Vec<EmojiChoice>,
}

/// Localized category header.
pub fn category_title(category: EmojiCategory) -> String {
    match category {
        EmojiCategory::People => l10n::tr("People"),
        EmojiCategory::Nature => l10n::tr("Nature"),
        EmojiCategory::Food => l10n::tr("Food & drink"),
        EmojiCategory::Activity => l10n::tr("Activities"),
        EmojiCategory::Travel => l10n::tr("Travel & places"),
        EmojiCategory::Objects => l10n::tr("Objects"),
        EmojiCategory::Symbols => l10n::tr("Symbols"),
        EmojiCategory::Flags => l10n::tr("Flags"),
    }
}

/// Picker sections in display order: frequently used, custom emoji by guild
/// (current guild first), then the eight unicode categories.
pub fn build_sections(
    state: &ClientState,
    channel_id: Option<Snowflake>,
    frecency: &EmojiFrecency,
    now_ms: u64,
) -> Vec<PickerSection> {
    let mut sections = Vec::new();

    let frecent = frecency_choices(state, channel_id, frecency, now_ms, PICKER_FRECENT_LIMIT);
    if !frecent.is_empty() {
        sections.push(PickerSection {
            kind: SectionKind::FrequentlyUsed,
            title: l10n::tr("Frequently used"),
            choices: frecent,
        });
    }

    for (guild_id, guild_name, choices) in custom_choices_by_guild(state, channel_id) {
        sections.push(PickerSection { kind: SectionKind::Guild(guild_id), title: guild_name, choices });
    }

    for category in EmojiCategory::ALL {
        sections.push(PickerSection {
            kind: SectionKind::Category(category),
            title: category_title(category),
            choices: unicode_emojis_in(category).map(EmojiChoice::from_unicode).collect(),
        });
    }

    sections
}

/// Splits `choices` into rows of `columns` items.
pub fn chunk_rows(choices: &[EmojiChoice], columns: usize) -> Vec<Vec<EmojiChoice>> {
    choices.chunks(columns.max(1)).map(<[EmojiChoice]>::to_vec).collect()
}

/// Moves `selected` by `dr` rows and `dc` columns in a grid of `total` items,
/// clamping to the grid bounds. An empty grid always selects 0.
pub fn move_grid(selected: usize, total: usize, columns: usize, dr: i32, dc: i32) -> usize {
    if total == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let selected = selected.min(total - 1);
    let row = selected / columns;
    let column = selected % columns;
    let last_row = (total - 1) / columns;

    let row = offset_clamped(row, dr, last_row);
    let column = offset_clamped(column, dc, columns - 1);
    (row * columns + column).min(total - 1)
}

fn offset_clamped(value: usize, delta: i32, max: usize) -> usize {
    let moved = if delta >= 0 {
        value.saturating_add(delta as usize)
    } else {
        value.saturating_sub(delta.unsigned_abs() as usize)
    };
    moved.min(max)
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::Dispatch;
    use fluxer_client_core::search::emoji::unicode_emojis;
    use fluxer_client_core::state::bits;
    use serde_json::json;

    use super::*;

    fn ready_state() -> ClientState {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s",
                "user": {"id": "100", "username": "me"},
                "guilds": [
                    {
                        "id": "1",
                        "properties": {"name": "Alpha"},
                        "roles": [{"id": "1", "name": "@everyone", "permissions": bits::DEFAULT.to_string()}],
                        "channels": [{"id": "11", "type": 0, "name": "general"}],
                        "members": [],
                        "emojis": [
                            {"id": "9001", "name": "partyparrot", "animated": true},
                            {"id": "9002", "name": "pepe", "animated": false}
                        ]
                    },
                    {
                        "id": "2",
                        "properties": {"name": "Beta Club"},
                        "roles": [{"id": "2", "name": "@everyone", "permissions": bits::DEFAULT.to_string()}],
                        "channels": [],
                        "members": [],
                        "emojis": [{"id": "9003", "name": "betaemoji", "animated": false}]
                    }
                ],
                "private_channels": [],
                "read_states": []
            }),
        });
        state
    }

    #[test]
    fn sections_follow_picker_order() {
        let state = ready_state();
        let mut frecency = EmojiFrecency::default();
        frecency.record("custom:9001", 1_000);

        let sections = build_sections(&state, Some(Snowflake(11)), &frecency, 2_000);
        let mut expected = vec![
            SectionKind::FrequentlyUsed,
            SectionKind::Guild(Snowflake(1)),
            SectionKind::Guild(Snowflake(2)),
        ];
        expected.extend(EmojiCategory::ALL.map(SectionKind::Category));
        assert_eq!(sections.iter().map(|s| s.kind).collect::<Vec<_>>(), expected);

        assert_eq!(sections[0].title, "Frequently used");
        assert_eq!(sections[0].choices.first().and_then(|c| c.custom_id), Some(Snowflake(9001)));
        assert_eq!(sections[1].title, "Alpha");
        assert_eq!(sections[2].title, "Beta Club");
        assert_eq!(sections[2].choices.first().and_then(|c| c.custom_id), Some(Snowflake(9003)));

        let titles: Vec<&str> = sections[3..].iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["People", "Nature", "Food & drink", "Activities", "Travel & places", "Objects", "Symbols", "Flags"]
        );
        assert_eq!(sections[3].choices, unicode_emojis_in(EmojiCategory::People).map(EmojiChoice::from_unicode).collect::<Vec<_>>());

        let no_frecent = build_sections(&state, Some(Snowflake(11)), &EmojiFrecency::default(), 2_000);
        assert_eq!(no_frecent.first().map(|s| s.kind), Some(SectionKind::Guild(Snowflake(1))));
        assert_eq!(no_frecent.len(), 2 + EmojiCategory::ALL.len());
    }

    #[test]
    fn chunk_rows_splits_into_grid_rows() {
        let choices: Vec<EmojiChoice> =
            unicode_emojis().iter().take(19).map(EmojiChoice::from_unicode).collect();
        let lengths: Vec<usize> = chunk_rows(&choices, COLUMNS).iter().map(Vec::len).collect();
        assert_eq!(lengths, [9, 9, 1]);

        assert!(chunk_rows(&[], COLUMNS).is_empty());
        assert_eq!(chunk_rows(&choices[..2], 0).len(), 2, "zero columns acts as one");
    }

    #[test]
    fn move_grid_clamps_to_bounds() {
        assert_eq!(move_grid(0, 0, COLUMNS, 1, 1), 0);
        assert_eq!(move_grid(3, 0, COLUMNS, -1, -1), 0);

        // total 19, 9 columns: rows of 9, 9, 1
        assert_eq!(move_grid(0, 19, COLUMNS, 0, 1), 1);
        assert_eq!(move_grid(0, 19, COLUMNS, 0, -1), 0, "left edge");
        assert_eq!(move_grid(8, 19, COLUMNS, 0, 1), 8, "right edge");
        assert_eq!(move_grid(0, 19, COLUMNS, 1, 0), 9);
        assert_eq!(move_grid(0, 19, COLUMNS, -1, 0), 0, "top edge");
        assert_eq!(move_grid(9, 19, COLUMNS, -1, 0), 0);
        assert_eq!(move_grid(10, 19, COLUMNS, 1, 0), 18, "short last row clamps");
        assert_eq!(move_grid(17, 19, COLUMNS, 1, 0), 18, "short last row clamps");
        assert_eq!(move_grid(18, 19, COLUMNS, 1, 0), 18, "bottom edge");
        assert_eq!(move_grid(18, 19, COLUMNS, 0, 1), 18, "last item stays");
        assert_eq!(move_grid(18, 19, COLUMNS, 0, -1), 18, "last row starts at column 0");
        assert_eq!(move_grid(99, 19, COLUMNS, 0, 0), 18, "selected is clamped");
        assert_eq!(move_grid(5, 10, 0, 1, 0), 6, "zero columns acts as one");
    }
}
