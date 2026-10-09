use bytes::Bytes;
use std::collections::HashMap;

use crate::error::*;

#[derive(Clone)]
pub enum RequestBody {
    Empty,
    Content(Bytes),
    Form(HashMap<String, String>),
    Json(serde_json::Value),
}

impl RequestBody {
    pub fn new(
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
    ) -> Result<Self, RqxCoreError> {
        match (content, data, json) {
            (None, None, None) => Ok(Self::Empty),
            (Some(content), None, None) => Ok(Self::Content(Bytes::copy_from_slice(content))),
            (None, Some(data), None) => Ok(Self::Form(data)),
            (None, None, Some(json)) => Ok(Self::Json(json)),
            _ => Err(RqxCoreError::InvalidArgument(
                "Only one of content, data, or json may be set".to_string(),
            )),
        }
    }
}
