/// Granular per-phase HTTP timeout config.
///
/// Each phase is independent and may be `None` (no timeout on that phase).
/// Phases:
///   - `connect` — TCP/TLS connection establishment
///   - `read`    — receiving response data
///   - `write`   — sending request body (currently a no-op; reqwest doesn't
///                 expose a per-phase write timeout)
///   - `pool`    — connection pool idle timeout (maps to reqwest's
///                 `pool_idle_timeout`; semantics differ slightly from httpx's
///                 pool-acquisition timeout)
///
/// Construct with a single `all` value to set every phase, or pass per-phase
/// kwargs. Per-phase kwargs take precedence over `all` when both are given.
#[derive(Clone)]
pub struct Timeout {
    pub connect: Option<f64>,
    pub read: Option<f64>,
    pub write: Option<f64>,
    pub pool: Option<f64>,
}

impl Timeout {
    pub fn new(
        connect: Option<f64>,
        read: Option<f64>,
        write: Option<f64>,
        pool: Option<f64>,
    ) -> Self {
        Self {
            connect,
            read,
            write,
            pool,
        }
    }

    pub fn per_request_total(&self) -> Option<f64> {
        if let Some(r) = self.read {
            return Some(r);
        }
        let mut max: Option<f64> = None;
        for v in [self.connect, self.write, self.pool] {
            if let Some(x) = v {
                max = Some(max.map_or(x, |m| m.max(x)));
            }
        }
        max
    }
}
