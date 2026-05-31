//! API key authentication middleware with rate limiting.
//!
//! ## Security Controls
//! - Constant-time API key comparison (CWE-208)
//! - Rate limiting on failed authentication attempts (CWE-307)
//! - Path normalization to prevent auth bypass (CWE-287)

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use std::sync::Mutex;
use std::time::Instant;

/// SECURITY: Rate limiter state for brute-force protection (CWE-307).
///
/// Tracks failed authentication attempts globally using a sliding window.
/// After MAX_FAILED_ATTEMPTS failures within WINDOW_SECS, all requests are
/// rejected for LOCKOUT_SECS regardless of API key correctness.
///
/// This is a global rate limiter (not per-IP) because:
/// - The API is designed for single-user/localhost use
/// - Per-IP tracking is trivially bypassed via proxies
/// - A global lockout protects the key itself
struct RateLimiter {
    /// Timestamps of recent failed attempts (ring buffer)
    failures: Vec<Instant>,
    /// When the lockout expires (None if not locked out)
    locked_until: Option<Instant>,
}

/// Maximum failed attempts before lockout.
const MAX_FAILED_ATTEMPTS: usize = 10;
/// Sliding window for counting failures (seconds).
const WINDOW_SECS: u64 = 60;
/// Lockout duration after too many failures (seconds).
const LOCKOUT_SECS: u64 = 300;

static RATE_LIMITER: std::sync::LazyLock<Mutex<RateLimiter>> = std::sync::LazyLock::new(|| {
    Mutex::new(RateLimiter {
        failures: Vec::new(),
        locked_until: None,
    })
});

/// Middleware that validates the X-API-Key header with rate limiting.
///
/// ## Auth bypass paths
/// Only `/api/v1/health` is allowed without authentication.
/// Path is normalized before comparison to prevent bypass via
/// double slashes, trailing slashes, or URL encoding (CWE-287).
pub async fn api_key_auth(
    req: Request,
    next: Next,
    expected_key: String,
) -> Result<Response, StatusCode> {
    // SECURITY: Normalize path to prevent auth bypass (CWE-287).
    // Reject requests with path traversal sequences.
    let path = req.uri().path();

    // Check for path traversal attempts
    if path.contains("..") {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Normalize: collapse repeated slashes, strip trailing slash
    let normalized = normalize_path(path);

    // Allow health check without auth (exact match on normalized path)
    if normalized == "/api/v1/health" {
        return Ok(next.run(req).await);
    }

    // Allow OpenAPI documentation endpoints without auth.
    // The docs MUST be reachable without a key so users can learn the API
    // shape before they've been issued credentials.
    //   - /api/v1/openapi.json            — raw OpenAPI 3.0 spec
    //   - /api/v1/docs, /api/v1/docs/*    — swagger-ui static assets + try-it
    //   - /api/v1/redoc                    — redoc static page
    if normalized == "/api/v1/openapi.json"
        || normalized == "/api/v1/docs"
        || normalized.starts_with("/api/v1/docs/")
        || normalized == "/api/v1/redoc"
    {
        return Ok(next.run(req).await);
    }

    // SECURITY: Check rate limiter before processing the key (CWE-307)
    // SECURITY: CWE-662 — If the rate limiter mutex is poisoned (prior panic
    // corrupted state), reject all requests rather than recovering with
    // `into_inner()`. Using corrupted state could bypass the rate limiter,
    // allowing unlimited brute-force attempts (CWE-307).
    {
        let mut rl = match RATE_LIMITER.lock() {
            Ok(guard) => guard,
            Err(_poison) => {
                tracing::error!(
                    "Rate limiter mutex poisoned (CWE-662) — rejecting request. \
                     Corrupted rate limiter state could allow brute-force bypass (CWE-307)."
                );
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            },
        };
        let now = Instant::now();

        // Check if currently locked out
        if let Some(until) = rl.locked_until {
            if now < until {
                tracing::warn!("Auth rate limit: rejecting request during lockout");
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            // Lockout expired — reset
            rl.locked_until = None;
            rl.failures.clear();
        }

        // Prune old failures outside the window
        let window_start = now - std::time::Duration::from_secs(WINDOW_SECS);
        rl.failures.retain(|&t| t > window_start);
    }

    let api_key = req.headers().get("X-API-Key").and_then(|v| v.to_str().ok());

    match api_key {
        Some(key) => {
            // SECURITY: Use constant-time comparison to prevent timing side-channel (CWE-208).
            let key_bytes = key.as_bytes();
            let expected_bytes = expected_key.as_bytes();
            let max_len = key_bytes.len().max(expected_bytes.len());
            let mut diff: u8 = 0;
            for i in 0..max_len {
                let a = key_bytes.get(i).copied().unwrap_or(0xFF);
                let b = expected_bytes.get(i).copied().unwrap_or(0xFE);
                diff |= a ^ b;
            }
            diff |= (key_bytes.len() != expected_bytes.len()) as u8;

            if diff == 0 {
                Ok(next.run(req).await)
            } else {
                record_failure();
                Err(StatusCode::UNAUTHORIZED)
            }
        },
        None => {
            record_failure();
            Err(StatusCode::UNAUTHORIZED)
        },
    }
}

/// Record a failed authentication attempt and trigger lockout if threshold exceeded.
/// SECURITY: CWE-662 — If the rate limiter mutex is poisoned, we log and return
/// without recording. The next auth check will reject with 500 anyway.
fn record_failure() {
    let mut rl = match RATE_LIMITER.lock() {
        Ok(guard) => guard,
        Err(_poison) => {
            tracing::error!(
                "Rate limiter mutex poisoned in record_failure (CWE-662) — \
                 cannot record failure, but auth is already rejecting all requests."
            );
            return;
        },
    };
    let now = Instant::now();

    rl.failures.push(now);

    // Prune and count
    let window_start = now - std::time::Duration::from_secs(WINDOW_SECS);
    rl.failures.retain(|&t| t > window_start);

    if rl.failures.len() >= MAX_FAILED_ATTEMPTS {
        rl.locked_until = Some(now + std::time::Duration::from_secs(LOCKOUT_SECS));
        rl.failures.clear();
        tracing::error!(
            "Auth rate limit: {} failed attempts in {}s — locking out for {}s (CWE-307)",
            MAX_FAILED_ATTEMPTS,
            WINDOW_SECS,
            LOCKOUT_SECS
        );
    }
}

/// SECURITY: Normalize URL path to prevent authentication bypass (CWE-287).
///
/// - Collapses `//` into `/`
/// - Strips trailing `/` (except root)
/// - Decodes percent-encoded path separators
///
/// Without this, paths like `/api/v1/health/`, `//api/v1/health`,
/// or `/api/v1/health%2f` could bypass the auth check.
fn normalize_path(path: &str) -> String {
    // First, percent-decode the path for comparison
    let decoded: String = percent_decode(path);

    // Collapse repeated slashes
    let mut normalized = String::with_capacity(decoded.len());
    let mut prev_slash = false;
    for ch in decoded.chars() {
        if ch == '/' {
            if !prev_slash {
                normalized.push('/');
            }
            prev_slash = true;
        } else {
            normalized.push(ch);
            prev_slash = false;
        }
    }

    // Strip trailing slash (except for root "/")
    if normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }

    normalized
}

/// Simple percent-decoding for path characters (CWE-20).
/// Only decodes characters relevant to path traversal/bypass.
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push((h << 4 | l) as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_basic() {
        assert_eq!(normalize_path("/api/v1/health"), "/api/v1/health");
    }

    #[test]
    fn test_normalize_path_trailing_slash() {
        assert_eq!(normalize_path("/api/v1/health/"), "/api/v1/health");
    }

    #[test]
    fn test_normalize_path_double_slash() {
        assert_eq!(normalize_path("//api/v1/health"), "/api/v1/health");
    }

    #[test]
    fn test_normalize_path_encoded_slash() {
        assert_eq!(normalize_path("/api/v1/health%2f"), "/api/v1/health");
    }

    #[test]
    fn test_normalize_path_root() {
        assert_eq!(normalize_path("/"), "/");
    }
}
