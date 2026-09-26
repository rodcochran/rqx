const DEFAULT_MAX_REDIRECTS: u32 = 20;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;

#[derive(Clone, Copy)]
pub struct RedirectPolicy {
    pub follow: bool,
    pub max_redirects: u32,
    pub raise_on_exceeded: bool,
}

impl Default for RedirectPolicy {
    fn default() -> Self {
        RedirectPolicy {
            follow: DEFAULT_FOLLOW_REDIRECTS,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            raise_on_exceeded: false,
        }
    }
}
