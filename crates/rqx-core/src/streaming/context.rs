use crate::client::Client;
use crate::error::*;
use crate::request::RequestSpec;
use crate::response::PendingResponse;

/// A request built by `stream()` but not yet sent. Sent once, on enter.
pub struct Unsent {
    client: Client,
    request: RequestSpec,
    follow_redirects: Option<bool>,
}

impl Unsent {
    pub fn new(client: Client, request: RequestSpec, follow_redirects: Option<bool>) -> Self {
        Self {
            client,
            request,
            follow_redirects,
        }
    }

    pub fn take(slot: &mut Option<Unsent>) -> Result<Unsent, RqxError> {
        slot.take().ok_or_else(|| {
            StreamError::StreamError(
                "stream already started; call stream() again for a new request".to_string(),
            )
            .into()
        })
    }

    pub async fn send(self) -> Result<PendingResponse, RqxError> {
        self.client
            .stream(self.request, self.follow_redirects)
            .await
    }
}
