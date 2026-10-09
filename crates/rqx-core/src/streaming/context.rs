use crate::client::Client;
use crate::error::*;
use crate::response::PendingResponse;

/// A request built by `stream()` but not yet sent. Sent once, on enter.
pub struct Unsent {
    client: Client,
    request: reqwest::Request,
    follow_redirects: bool,
}

impl Unsent {
    pub(crate) fn new(client: Client, request: reqwest::Request, follow_redirects: bool) -> Self {
        Self {
            client,
            request,
            follow_redirects,
        }
    }

    pub fn take(slot: &mut Option<Unsent>) -> Result<Unsent, RqxCoreError> {
        slot.take().ok_or_else(|| {
            RqxCoreError::StreamError(
                "stream already started; call stream() again for a new request".to_string(),
            )
        })
    }

    pub async fn send(self) -> Result<PendingResponse, RqxCoreError> {
        self.client.send(self.request, self.follow_redirects).await
    }
}
