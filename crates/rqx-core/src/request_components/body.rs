use std::collections::HashMap;

use crate::error::*;

#[derive(Clone)]
pub enum RequestBody {
    Empty,
    Content(Vec<u8>),
    Form(HashMap<String, String>),
    Json(serde_json::Value),
}

impl RequestBody {
    pub fn new(
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
    ) -> Result<Self, RqxError> {
        match (content, data, json) {
            (None, None, None) => Ok(Self::Empty),
            (Some(content), None, None) => Ok(Self::Content(content.to_vec())),
            (None, Some(data), None) => Ok(Self::Form(data)),
            (None, None, Some(json)) => Ok(Self::Json(json)),
            _ => Err(RqxError::InvalidArgument(
                "Only one of content, data, or json may be set".to_string(),
            )),
        }
    }
}
