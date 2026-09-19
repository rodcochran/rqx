#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::redundant_field_names
)]

mod http;
mod query_params;
mod request;
mod request_headers;
mod response;
mod transport;
mod url;

pub mod client;
pub mod error;
pub mod retry;
pub mod timeout;
