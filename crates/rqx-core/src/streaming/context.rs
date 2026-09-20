use crate::client::Client;
use crate::error::*;
use crate::request::RequestSpec;

/// A request built by `stream()` but not yet sent. Sent once, on enter.
struct Unsent {
    client: Client,
    request: RequestSpec,
    follow_redirects: Option<bool>,
}

impl Unsent {
    fn take(slot: &mut Option<Unsent>) -> Result<Unsent, RqxError> {
        slot.take().ok_or_else(|| {
            StreamError::StreamError(
                "stream already started; call stream() again for a new request".to_string(),
            )
            .into()
        })
    }
}
