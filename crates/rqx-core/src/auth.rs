use crate::error::RqxCoreError;

#[derive(Clone, Default)]
pub enum Auth {
    #[default]
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer(String),
}

impl Auth {
    pub fn new(
        basic: Option<(String, String)>,
        bearer: Option<String>,
    ) -> Result<Self, RqxCoreError> {
        match (basic, bearer) {
            (None, None) => Ok(Self::None),
            (Some((username, password)), None) => Ok(Self::Basic { username, password }),
            (None, Some(token)) => Ok(Self::Bearer(token)),
            (Some(_), Some(_)) => Err(RqxCoreError::InvalidArgument(
                "Cannot specify both basic auth and a bearer token.".to_string(),
            )),
        }
    }
}
