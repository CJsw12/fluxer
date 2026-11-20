//! Guild role and channel permission mutations.

use std::sync::Mutex;

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{RestClient, RolePatch};
use fluxer_client_core::state::{ClientState, PermissionOverwrite, Role, Snowflake};
use serde_json::{Value, json};

use super::{Backend, UiEvent, channels::access_was_denied, lock};

pub enum AccessChange {
    CreateRole {
        guild: Snowflake,
        name: String,
    },
    EditRole {
        guild: Snowflake,
        before: Role,
        patch: RolePatch,
    },
    DeleteRole {
        guild: Snowflake,
        before: Role,
    },
    MoveRole {
        guild: Snowflake,
        role: Snowflake,
        adjacent: Snowflake,
    },
    Overwrite {
        channel: Snowflake,
        target: Snowflake,
        before: Option<PermissionOverwrite>,
        after: Option<PermissionOverwrite>,
    },
}

const CHANGED: &str = "The permissions or target changed. Reopen the editor and try again.";

impl Backend {
    pub fn change_access(&self, seq: u64, change: AccessChange) {
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = run_change(&rest, &state, change).await;
            let _ = events.send(UiEvent::StateChanged);
            let _ = events.send(UiEvent::AccessChanged {
                seq,
                error: result.err(),
            });
        });
    }
}

async fn run_change(rest: &RestClient, state: &Mutex<ClientState>, change: AccessChange) -> Result<(), String> {
    let session = lock(state).session_id.clone();
    match change {
        AccessChange::CreateRole { guild, name } => {
            let before = {
                let state = lock(state);
                if !state.can_manage_guild_roles(guild) {
                    return Err(CHANGED.to_owned());
                }
                state.guilds[&guild].roles.clone()
            };
            let role = rest.create_guild_role(guild, &name).await.map_err(|e| e.to_string())?;
            let mut state = lock(state);
            if state.session_id == session
                && state
                    .guilds
                    .get(&guild)
                    .is_some_and(|g| g.roles == before && !g.roles.contains_key(&role.id))
            {
                dispatch(
                    &mut state,
                    "GUILD_ROLE_CREATE",
                    json!({ "guild_id": guild, "role": role }),
                );
            }
        }
        AccessChange::EditRole { guild, before, patch } => {
            {
                let state = lock(state);
                if !state.can_manage_role(guild, before.id)
                    || find_role(&state, guild, before.id) != Some(&before)
                    || patch
                        .permissions
                        .is_some_and(|p| !state.can_write_role_permissions(guild, p))
                {
                    return Err(CHANGED.to_owned());
                }
            }
            let role = rest
                .update_guild_role(guild, before.id, &patch)
                .await
                .map_err(|e| e.to_string())?;
            let mut state = lock(state);
            if state.session_id == session && find_role(&state, guild, before.id) == Some(&before) {
                dispatch(
                    &mut state,
                    "GUILD_ROLE_UPDATE",
                    json!({ "guild_id": guild, "role": role }),
                );
            }
        }
        AccessChange::DeleteRole { guild, before } => {
            {
                let state = lock(state);
                if before.id == guild
                    || !state.can_manage_role(guild, before.id)
                    || find_role(&state, guild, before.id) != Some(&before)
                {
                    return Err(CHANGED.to_owned());
                }
            }
            rest.delete_guild_role(guild, before.id)
                .await
                .map_err(|e| e.to_string())?;
            let mut state = lock(state);
            if state.session_id == session && find_role(&state, guild, before.id) == Some(&before) {
                dispatch(
                    &mut state,
                    "GUILD_ROLE_DELETE",
                    json!({ "guild_id": guild, "role_id": before.id }),
                );
            }
        }
        AccessChange::MoveRole { guild, role, adjacent } => {
            let (before, positions) = {
                let state = lock(state);
                let positions = role_move_positions(&state, guild, role, adjacent).ok_or(CHANGED)?;
                (state.guilds[&guild].roles.clone(), positions)
            };
            rest.reorder_guild_roles(guild, &positions)
                .await
                .map_err(|e| e.to_string())?;
            let roles = rest.list_guild_roles(guild).await.map_err(refresh_error)?;
            let mut state = lock(state);
            if state.session_id == session && state.guilds.get(&guild).is_some_and(|g| g.roles == before) {
                // Reordering changes positions. Keep the other fields from the gateway.
                let roles: Vec<_> = roles
                    .into_iter()
                    .filter_map(|role| {
                        let mut current = before.get(&role.id)?.clone();
                        current.position = role.position;
                        Some(current)
                    })
                    .collect();
                dispatch(
                    &mut state,
                    "GUILD_ROLE_UPDATE_BULK",
                    json!({ "guild_id": guild, "roles": roles }),
                );
            }
        }
        AccessChange::Overwrite {
            channel,
            target,
            before,
            after,
        } => {
            {
                let state = lock(state);
                if find_overwrite(&state, channel, target) != before.as_ref()
                    || !state.can_write_channel_overwrite(channel, before.as_ref(), after.as_ref())
                {
                    return Err(CHANGED.to_owned());
                }
            }
            match after {
                Some(overwrite) => rest.set_channel_overwrite(channel, &overwrite).await,
                None => rest.delete_channel_overwrite(channel, target).await,
            }
            .map_err(|e| e.to_string())?;
            let refreshed = fetch_overwrites(rest, channel).await?;
            let mut state = lock(state);
            if state.session_id == session {
                if let Some(overwrites) = refreshed {
                    apply_confirmed_overwrite(&mut state, channel, target, before.as_ref(), overwrites);
                } else if state.channels.contains_key(&channel)
                    && find_overwrite(&state, channel, target) == before.as_ref()
                {
                    state.set_denied_channel_access(channel, true);
                }
            }
        }
    }
    Ok(())
}

fn apply_confirmed_overwrite(
    state: &mut ClientState,
    channel: Snowflake,
    target: Snowflake,
    before: Option<&PermissionOverwrite>,
    overwrites: Vec<PermissionOverwrite>,
) {
    if find_overwrite(state, channel, target) != before {
        return;
    }
    let Some(current) = state.channels.get(&channel) else {
        return;
    };
    let mut merged = current.permission_overwrites.clone();
    merged.retain(|item| item.id != target);
    merged.extend(overwrites.into_iter().filter(|item| item.id == target));
    dispatch(
        state,
        "CHANNEL_UPDATE",
        json!({ "id": channel, "permission_overwrites": merged }),
    );
}

fn role_move_positions(
    state: &ClientState,
    guild: Snowflake,
    role: Snowflake,
    adjacent: Snowflake,
) -> Option<Vec<(Snowflake, i32)>> {
    if !state.can_manage_role(guild, role)
        || !state.can_manage_role(guild, adjacent)
        || role == guild
        || adjacent == guild
    {
        return None;
    }
    let mut roles: Vec<_> = state
        .guilds
        .get(&guild)?
        .roles
        .values()
        .filter(|r| r.id != guild)
        .collect();
    roles.sort_by_key(|role| (std::cmp::Reverse(role.position), role.id));
    let from = roles.iter().position(|r| r.id == role)?;
    let to = roles.iter().position(|r| r.id == adjacent)?;
    if from.abs_diff(to) != 1 {
        return None;
    }
    roles.swap(from, to);
    Some(
        roles
            .iter()
            .rev()
            .filter(|r| state.can_manage_role(guild, r.id))
            .enumerate()
            .map(|(index, r)| (r.id, index as i32 + 1))
            .collect(),
    )
}

async fn fetch_overwrites(
    rest: &RestClient,
    channel: Snowflake,
) -> Result<Option<Vec<PermissionOverwrite>>, String> {
    let response = match rest.get_guild_channel(channel).await {
        Ok(response) => response,
        // The requested overwrite can remove the actor's own access.
        Err(error) if access_was_denied(&error) => return Ok(None),
        Err(error) => return Err(refresh_error(error)),
    };
    if response.get("id").and_then(Value::as_str).and_then(Snowflake::parse) != Some(channel) {
        return Err(refresh_error("channel did not match the request"));
    }
    let overwrites = response
        .get("permission_overwrites")
        .cloned()
        .ok_or_else(|| refresh_error("channel permissions missing from response"))?;
    serde_json::from_value(overwrites).map(Some).map_err(refresh_error)
}

fn find_role(state: &ClientState, guild: Snowflake, role: Snowflake) -> Option<&Role> {
    state.guilds.get(&guild)?.roles.get(&role)
}

pub(crate) fn find_overwrite(
    state: &ClientState,
    channel: Snowflake,
    target: Snowflake,
) -> Option<&PermissionOverwrite> {
    state
        .channels
        .get(&channel)?
        .permission_overwrites
        .iter()
        .find(|item| item.id == target)
}

fn dispatch(state: &mut ClientState, t: &str, d: Value) {
    state.apply(&Dispatch {
        t: t.to_owned(),
        s: 0,
        d,
    });
}

fn refresh_error(error: impl std::fmt::Display) -> String {
    format!("Saved, but the updated permissions could not be loaded: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_response_preserves_newer_target_and_sibling() {
        let channel = Snowflake(11);
        let target = Snowflake(2);
        let sibling = Snowflake(3);
        let item = |id, allow| PermissionOverwrite {
            id,
            kind: 0,
            allow,
            deny: 0,
        };
        let before = item(target, 0);
        let mut state = ClientState::new();
        dispatch(
            &mut state,
            "CHANNEL_CREATE",
            json!({ "id": channel, "type": 0, "permission_overwrites": [before] }),
        );
        dispatch(
            &mut state,
            "CHANNEL_UPDATE",
            json!({
                "id": channel, "permission_overwrites": [before, item(sibling, 8)],
            }),
        );
        apply_confirmed_overwrite(
            &mut state,
            channel,
            target,
            Some(&before),
            vec![item(target, 4), item(sibling, 0)],
        );
        assert_eq!(find_overwrite(&state, channel, sibling), Some(&item(sibling, 8)));
        assert_eq!(find_overwrite(&state, channel, target), Some(&item(target, 4)));
        apply_confirmed_overwrite(&mut state, channel, target, Some(&before), vec![item(target, 16)]);
        assert_eq!(find_overwrite(&state, channel, target), Some(&item(target, 4)));
    }
}
