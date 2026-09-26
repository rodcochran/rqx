use crate::{auth::Auth::Basic, error::RqxError};

#[derive(Clone)]
pub enum Auth {
    None,
    Basic { username: String, password: String },
    Bearer(String),
}

impl Auth {
    pub fn new(basic: Option<(String, String)>, bearer: Option<String>) -> Result<Self, RqxError> {
        match (basic, bearer) {
            (None, None) => Ok(Self::None),
            (Some((username, password)), None) => Ok(Self::Basic { username, password }),
            (None, Some(token)) => Ok(Self::Bearer(token)),
            (Some(_), Some(_)) => Err(RqxError::InvalidArgument(
                "Cannot specify both basic auth and a bearer token.".to_string(),
            )),
        }
    }

    pub fn validate_coexistence(&self, other: &Auth) -> Result<(), RqxError> {
        match (self, other) {
            (Self::Basic { .. }, Self::Bearer(_)) | (Self::Bearer(_), Self::Basic { .. }) => {
                Err(RqxError::InvalidArgument(
                    "Cannot specify both basic auth and a bearer token.".to_string(),
                ))
            }
            _ => Ok(()),
        }
    }
}

impl Default for Auth {
    fn default() -> Self {
        Auth::None
    }
}
