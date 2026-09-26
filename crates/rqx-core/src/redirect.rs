const DEFAULT_MAX_REDIRECTS: u32 = 20;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;
const DEFAULT_RAISE_ON_REDIRECT: bool = true;

#[derive(Clone, Copy)]
pub struct RedirectPolicy {
    pub follow: bool,
    pub max_redirects: u32,
    pub raise_on_exceeded: bool,
}

impl RedirectPolicy {
    pub fn with_defaults(
        follow: Option<bool>,
        max_redirects: Option<u32>,
        raise_on_exceeded: Option<bool>,
    ) -> Self {
        let defaults = RedirectPolicy::default();
        Self {
            follow: follow.unwrap_or(defaults.follow),
            max_redirects: max_redirects.unwrap_or(defaults.max_redirects),
            raise_on_exceeded: raise_on_exceeded.unwrap_or(defaults.raise_on_exceeded),
        }
    }
}

impl Default for RedirectPolicy {
    fn default() -> Self {
        RedirectPolicy {
            follow: DEFAULT_FOLLOW_REDIRECTS,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            raise_on_exceeded: DEFAULT_RAISE_ON_REDIRECT,
        }
    }
}
