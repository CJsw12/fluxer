//! Message search actions on [`Backend`]: `POST /search/messages`.

use fluxer_client_core::rest::MessageSearchRequest;

use super::{Backend, UiEvent};
#[cfg(test)]
use super::lock_sent;

impl Backend {
    /// Runs `POST /search/messages` in the background and notifies the UI via
    /// [`UiEvent::SearchLoaded`] tagged with `seq`.
    pub fn search_messages(&self, seq: u64, request: MessageSearchRequest) {
        #[cfg(test)]
        lock_sent(&self.sent_searches).push(request.clone());
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = rest.search_messages(&request).await.map_err(|err| err.to_string());
            let _ = events.send(UiEvent::SearchLoaded { seq, result });
        });
    }
}
