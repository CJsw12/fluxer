//! Guild custom emojis: the guild payload's `emojis` and
//! GUILD_EMOJIS_UPDATE (`Emoji.handleGuildEmojiUpdated`, list replaced).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{id_field, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;
use crate::state::model::null_default;

/// `GuildEmojiResponse` without the uploader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuildEmoji {
    pub id: Snowflake,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub animated: bool,
}

impl ClientState {
    pub(in crate::state) fn load_guild_emojis(&mut self, guild_id: Snowflake, g: &Value) {
        let emojis = parse_list::<GuildEmoji>(g.get("emojis"), "guild.emojis");
        self.emojis.insert(guild_id, emojis);
    }

    pub(in crate::state) fn apply_guild_emojis_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(guild_id) = id_field(d, "guild_id") else { return };
        if !self.guilds.contains_key(&guild_id) {
            return;
        }
        self.load_guild_emojis(guild_id, d);
        out.push(StateChange::EmojisChanged { guild_id });
    }

    /// The guild's custom emojis (empty if unknown).
    pub fn guild_emojis(&self, guild_id: Snowflake) -> &[GuildEmoji] {
        self.emojis.get(&guild_id).map_or(&[], Vec::as_slice)
    }

    /// A custom emoji by id in any loaded guild, with its guild.
    pub fn find_emoji(&self, emoji_id: Snowflake) -> Option<(Snowflake, &GuildEmoji)> {
        self.emojis
            .iter()
            .find_map(|(guild_id, list)| list.iter().find(|e| e.id == emoji_id).map(|e| (*guild_id, e)))
    }
}
