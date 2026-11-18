//! Member list LAZY_REQUEST (op 14 with `member_list_channels`) and the
//! own presence (op 3). The per-session bookkeeping (synced guilds, last
//! sent ranges) lives in `ui::member_list::MemberListSubscriptions`, which
//! calls [`Backend::send_member_list`]; `backend` declares this module
//! private, so only `impl Backend` methods with core types cross it.

use fluxer_client_core::gateway::PresenceUpdate;
use fluxer_client_core::state::Status;

use super::*;

impl Backend {
    /// LAZY_REQUEST for one channel's member list. Returns whether it was
    /// queued; false without a gateway, for visiting guilds (no member
    /// list, `applyChannelSubscription`) or when the queue rejected it.
    pub fn send_member_list(&self, guild_id: Snowflake, channel_id: Snowflake, ranges: &[(u32, u32)], sync: bool) -> bool {
        let Some(gateway) = &self.gateway else { return false };
        if self.state().guilds.get(&guild_id).is_some_and(|g| g.visiting) {
            return false;
        }
        match gateway.subscribe_member_list(&guild_id.to_string(), &channel_id.to_string(), ranges, sync) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, "member list LAZY_REQUEST not queued");
                false
            }
        }
    }

    /// The own status: `user_settings.status`, online when unset.
    pub fn own_status(&self) -> Status {
        match self.state().user_settings.status.as_deref() {
            None => Status::Online,
            Some(status) => Status::parse(Some(status)),
        }
    }

    /// Sets the own status: local `user_settings.status` now (the user
    /// panel dot), then PRESENCE_UPDATE (op 3). `Offline` is not settable.
    pub fn set_presence(&self, status: Status) {
        if status == Status::Offline {
            return;
        }
        self.state().user_settings.status = Some(status.as_str().to_owned());
        let _ = self.events.send(UiEvent::StateChanged);
        let Some(gateway) = &self.gateway else { return };
        let presence =
            PresenceUpdate { status: status.as_str().to_owned(), afk: false, mobile: false, custom_status: None };
        if let Err(err) = gateway.set_presence(&presence) {
            tracing::warn!(%err, "PRESENCE_UPDATE not queued");
        }
    }
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::rest::{ClientIdentity, Endpoints, RestConfig};

    use super::*;

    fn offline_backend(rt: &tokio::runtime::Runtime) -> (Backend, mpsc::UnboundedReceiver<UiEvent>) {
        let endpoints = Endpoints {
            api_code_version: 1,
            api: "https://fluxer.invalid/api".to_owned(),
            gateway: "wss://fluxer.invalid/gateway".to_owned(),
            media: None,
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: None,
            discovery_url: "https://fluxer.invalid/.well-known/fluxer".to_owned(),
            document: json!({}),
        };
        let rest = RestClient::new(RestConfig::new(&endpoints, ClientIdentity::default())).expect("rest client");
        Backend::new(rt.handle().clone(), Arc::new(rest), None)
    }

    #[test]
    fn presence_is_set_locally_and_member_list_needs_a_gateway() {
        let rt = tokio::runtime::Builder::new_current_thread().build().expect("runtime");
        let (backend, mut events) = offline_backend(&rt);
        assert_eq!(backend.own_status(), Status::Online);
        backend.set_presence(Status::Dnd);
        assert_eq!(backend.own_status(), Status::Dnd);
        assert_eq!(events.try_recv().ok(), Some(UiEvent::StateChanged));
        backend.set_presence(Status::Offline);
        assert_eq!(backend.own_status(), Status::Dnd, "offline is not a settable status");
        assert!(!backend.send_member_list(Snowflake(1), Snowflake(2), &[(0, 99)], true));
    }
}
