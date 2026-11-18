//! ALTCHA v2 proof-of-work solver for `CAPTCHA_REQUIRED` / `INVALID_CAPTCHA`.
//!
//! Port of `fluxer_app/src/features/auth/altcha/AltchaSolver.ts` and
//! `altcha-lib@2.8.0` `solveChallenge` (v2, PBKDF2 key derivation). The
//! server (`fluxer_api/pkgs/captcha/src/providers/AltchaProvider.ts`) issues
//! `{parameters: {algorithm, nonce, salt, cost, keyLength, keyPrefix,
//! keySignature, expiresAt}, signature}`. The solver finds the counter whose
//! `PBKDF2(password = nonce || u32_be(counter), salt, cost, keyLength)`
//! starts with `keyPrefix` and sends base64 JSON
//! `{challenge: {parameters, signature}, solution: {counter, derivedKey, time}}`
//! in `X-Captcha-Token`. `parameters` are echoed verbatim because the server
//! verifies its HMAC over them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Sha256, Sha384, Sha512};
use tokio::sync::Semaphore;

use super::client::base64_encode;
use super::error::RestError;
use super::secret::Secret;

/// `MAX_SOLVES` in `CaptchaInterceptor.ts`.
pub(crate) const MAX_CAPTCHA_SOLVES: u32 = 2;
/// `SOLVE_TIMEOUT_MS` in `AltchaSolver.ts`.
pub const ALTCHA_SOLVE_TIMEOUT: Duration = Duration::from_secs(60);
/// Highest counter tried. The server picks the counter in
/// `[max_counter / 2, max_counter]` with `max_counter <= 20000`
/// (`CAPTCHA_MAX_MAX_COUNTER`, `CaptchaSchemas.ts`); 5x margin.
pub const ALTCHA_MAX_COUNTER: u32 = 100_000;
/// `MAX_SOLVER_WORKERS` in `AltchaSolver.ts`.
const MAX_SOLVER_THREADS: usize = 8;
/// Server cost tops out at 20000 * 2^6 (`CAPTCHA_MAX_COST`,
/// `MAX_CAPTCHA_ESCALATION_STEPS`) = 1,280,000.
const MAX_COST: u64 = 20_000 * 64;
const MAX_KEY_LENGTH: u64 = 32;
/// Longest accepted `keyPrefix` in hex characters.
const MAX_KEY_PREFIX_HEX: usize = 32;
/// Longest accepted `nonce` / `salt` in hex characters.
const MAX_NONCE_SALT_HEX: usize = 128;
/// The server rejects longer tokens (`ALTCHA_MAX_TOKEN_LENGTH`).
const MAX_TOKEN_LENGTH: usize = 4096;
/// Bytes of the token payload besides the echoed parameters and signature
/// (`challenge` / `solution` keys, counter, derived key hex, time).
const TOKEN_PAYLOAD_OVERHEAD: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    Sha256,
    Sha384,
    Sha512,
}

/// Validated challenge parameters.
#[derive(Debug)]
struct Challenge {
    algorithm: Algorithm,
    nonce: Vec<u8>,
    salt: Vec<u8>,
    cost: u32,
    key_length: usize,
    /// Lowercase hex.
    key_prefix: String,
    /// Raw `parameters` object, echoed back unchanged.
    parameters: Value,
    signature: String,
}

/// `readAltchaChallenge`: the `altcha_challenge` of an error body when
/// `captcha_provider` is `altcha` and it has `parameters` and `signature`.
pub(crate) fn read_altcha_challenge(body: &Value) -> Option<&Value> {
    if body.get("captcha_provider").and_then(Value::as_str) != Some("altcha") {
        return None;
    }
    let challenge = body.get("altcha_challenge")?;
    let has_parameters = challenge.get("parameters").is_some_and(Value::is_object);
    let has_signature = challenge.get("signature").is_some_and(Value::is_string);
    (has_parameters && has_signature).then_some(challenge)
}

/// Solves an `altcha_challenge` (`{parameters, signature}`) and returns the
/// `X-Captcha-Token` value. CPU-bound and blocking for up to
/// [`ALTCHA_SOLVE_TIMEOUT`]: call it from a blocking thread, not an async
/// executor. Useful with [`super::RestClient::login_with_captcha`].
pub fn solve_altcha_challenge(challenge: &Value) -> Result<Secret, RestError> {
    solve_bounded(challenge, ALTCHA_MAX_COUNTER, ALTCHA_SOLVE_TIMEOUT)
}

/// Sets the shared cancel flag when dropped, so the blocking solver stops
/// when the awaiting future is dropped.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Runs the solver on tokio's blocking pool, at most one solve at a time per
/// `limiter`. The permit is held until the blocking work actually ends.
pub(crate) async fn solve_off_executor(challenge: Value, limiter: Arc<Semaphore>) -> Result<Secret, RestError> {
    let permit = limiter
        .acquire_owned()
        .await
        .map_err(|_| RestError::Captcha("solver limiter closed".to_owned()))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let _guard = CancelOnDrop(cancel.clone());
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        solve_cancellable(&challenge, ALTCHA_MAX_COUNTER, ALTCHA_SOLVE_TIMEOUT, &cancel)
    })
    .await
    .map_err(|_| RestError::Captcha("solver thread failed".to_owned()))?
}

pub(crate) fn solve_bounded(challenge: &Value, max_counter: u32, timeout: Duration) -> Result<Secret, RestError> {
    solve_cancellable(challenge, max_counter, timeout, &AtomicBool::new(false))
}

fn solve_cancellable(
    challenge: &Value,
    max_counter: u32,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Secret, RestError> {
    let challenge = parse_challenge(challenge)?;
    let started = Instant::now();
    let deadline = started + timeout;
    let threads = std::thread::available_parallelism()
        .map_or(2, usize::from)
        .clamp(1, MAX_SOLVER_THREADS);
    let found = search(&challenge, max_counter, deadline, threads, cancel)?;
    let (counter, derived_key) =
        found.ok_or_else(|| RestError::Captcha("no solution within the counter and time bounds".to_owned()))?;
    let elapsed_ms = (started.elapsed().as_secs_f64() * 10_000.0).floor() / 10.0;
    let payload = json!({
        "challenge": { "parameters": challenge.parameters, "signature": challenge.signature },
        "solution": { "counter": counter, "derivedKey": to_hex(&derived_key), "time": elapsed_ms },
    });
    let token = base64_encode(payload.to_string().as_bytes());
    if token.len() > MAX_TOKEN_LENGTH {
        return Err(RestError::Captcha("solution token exceeds the server limit".to_owned()));
    }
    Ok(Secret::new(token))
}

/// Interleaved search over `0..=max_counter` (thread `i` tries
/// `i, i + n, ...`, like `solveChallengeWorkers`). Returns the lowest
/// counter found by any thread once all threads stop. Every thread checks
/// the deadline, `cancel` and the shared stop flag before each derivation.
fn search(
    challenge: &Challenge,
    max_counter: u32,
    deadline: Instant,
    threads: usize,
    cancel: &AtomicBool,
) -> Result<Option<(u32, Vec<u8>)>, RestError> {
    let stop = AtomicBool::new(false);
    let best: Mutex<Option<(u32, Vec<u8>)>> = Mutex::new(None);
    let step = u32::try_from(threads).unwrap_or(1).max(1);
    let mut failure: Option<RestError> = None;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for start in 0..step {
            let stop = &stop;
            let best = &best;
            let spawned = std::thread::Builder::new()
                .name("altcha-solver".to_owned())
                .spawn_scoped(scope, move || {
                    let mut key = vec![0u8; challenge.key_length];
                    let mut password = Vec::with_capacity(challenge.nonce.len() + 4);
                    password.extend_from_slice(&challenge.nonce);
                    password.extend_from_slice(&[0; 4]);
                    let counter_at = challenge.nonce.len();
                    let mut counter = start;
                    while counter <= max_counter {
                        if stop.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        if Instant::now() >= deadline {
                            stop.store(true, Ordering::Relaxed);
                            return;
                        }
                        password[counter_at..].copy_from_slice(&counter.to_be_bytes());
                        derive_key(challenge, &password, &mut key);
                        if matches_prefix(&key, &challenge.key_prefix) {
                            if let Ok(mut slot) = best.lock() {
                                if slot.as_ref().is_none_or(|(c, _)| counter < *c) {
                                    *slot = Some((counter, key.clone()));
                                }
                            }
                            stop.store(true, Ordering::Relaxed);
                            return;
                        }
                        match counter.checked_add(step) {
                            Some(next) => counter = next,
                            None => return,
                        }
                    }
                });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    // Counters are interleaved, so a missing thread would
                    // leave gaps: give up instead of searching partially.
                    stop.store(true, Ordering::Relaxed);
                    failure = Some(RestError::Captcha(format!("could not start solver thread: {error}")));
                    break;
                }
            }
        }
        for handle in handles {
            if handle.join().is_err() && failure.is_none() {
                failure = Some(RestError::Captcha("solver thread panicked".to_owned()));
            }
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(RestError::Captcha("solver cancelled".to_owned()));
    }
    best.into_inner()
        .map_err(|_| RestError::Captcha("solver thread panicked".to_owned()))
}

fn derive_key(challenge: &Challenge, password: &[u8], out: &mut [u8]) {
    match challenge.algorithm {
        Algorithm::Sha256 => pbkdf2::pbkdf2_hmac::<Sha256>(password, &challenge.salt, challenge.cost, out),
        Algorithm::Sha384 => pbkdf2::pbkdf2_hmac::<Sha384>(password, &challenge.salt, challenge.cost, out),
        Algorithm::Sha512 => pbkdf2::pbkdf2_hmac::<Sha512>(password, &challenge.salt, challenge.cost, out),
    }
}

/// Even-length prefixes compare bytes, odd-length ones compare hex text
/// (`solveChallenge`); both reduce to a lowercase hex prefix check.
fn matches_prefix(key: &[u8], prefix_hex: &str) -> bool {
    let needed = prefix_hex.len().div_ceil(2);
    if needed > key.len() {
        return false;
    }
    to_hex(&key[..needed]).starts_with(prefix_hex)
}

fn parse_challenge(challenge: &Value) -> Result<Challenge, RestError> {
    let invalid = |what: &str| RestError::Captcha(format!("invalid ALTCHA challenge: {what}"));
    let parameters = challenge
        .get("parameters")
        .filter(|v| v.is_object())
        .ok_or_else(|| invalid("parameters missing"))?;
    let signature = challenge
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("signature missing"))?;
    let algorithm = match parameters.get("algorithm").and_then(Value::as_str) {
        Some("PBKDF2/SHA-256") => Algorithm::Sha256,
        Some("PBKDF2/SHA-384") => Algorithm::Sha384,
        Some("PBKDF2/SHA-512") => Algorithm::Sha512,
        _ => return Err(invalid("unsupported algorithm")),
    };
    // Bound the echoed payload before any work, so the final token cannot
    // exceed the server limit.
    if parameters.to_string().len() + signature.len() + TOKEN_PAYLOAD_OVERHEAD > MAX_TOKEN_LENGTH / 4 * 3 {
        return Err(invalid("challenge too large"));
    }
    match parameters.get("expiresAt") {
        None | Some(Value::Null) => {}
        Some(v) => {
            // Seconds since the epoch (`pow.js` compares to `Date.now() / 1000`);
            // 0 counts as unset there.
            let expires = v.as_f64().filter(|e| e.is_finite() && *e >= 0.0).ok_or_else(|| invalid("expiresAt"))?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
            if expires != 0.0 && expires < now {
                return Err(invalid("challenge expired"));
            }
        }
    }
    let hex_field = |key: &str| {
        parameters
            .get(key)
            .and_then(Value::as_str)
            .filter(|t| t.len() <= MAX_NONCE_SALT_HEX)
            .and_then(from_hex)
            .ok_or_else(|| invalid(key))
    };
    let nonce = hex_field("nonce")?;
    let salt = hex_field("salt")?;
    let cost = parameters
        .get("cost")
        .and_then(Value::as_u64)
        .filter(|c| (1..=MAX_COST).contains(c))
        .and_then(|c| u32::try_from(c).ok())
        .ok_or_else(|| invalid("cost"))?;
    let key_length = match parameters.get("keyLength") {
        None | Some(Value::Null) => 32,
        Some(v) => v
            .as_u64()
            .filter(|l| (1..=MAX_KEY_LENGTH).contains(l))
            .and_then(|l| usize::try_from(l).ok())
            .ok_or_else(|| invalid("keyLength"))?,
    };
    let key_prefix = parameters
        .get("keyPrefix")
        .and_then(Value::as_str)
        .filter(|p| {
            // `keyPrefixLength >= derivedKey.length` is refused by altcha-lib.
            !p.is_empty()
                && p.len() <= MAX_KEY_PREFIX_HEX
                && p.len() < key_length * 2
                && p.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| invalid("keyPrefix"))?;
    Ok(Challenge {
        algorithm,
        nonce,
        salt,
        cost,
        key_length,
        key_prefix,
        parameters: parameters.clone(),
        signature: signature.to_owned(),
    })
}

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(overrides: &[(&str, Value)]) -> Value {
        let mut parameters = json!({
            "algorithm": "PBKDF2/SHA-256", "nonce": "00112233", "salt": "44556677",
            "cost": 1, "keyLength": 32, "keyPrefix": "a",
        });
        for (key, value) in overrides {
            parameters[*key] = value.clone();
        }
        json!({ "parameters": parameters, "signature": "sig" })
    }

    fn rejected(overrides: &[(&str, Value)]) -> bool {
        parse_challenge(&params(overrides)).is_err()
    }

    #[test]
    fn key_prefix_must_be_shorter_than_the_key() {
        assert!(!rejected(&[("keyLength", json!(4)), ("keyPrefix", json!("abcdef0"))]));
        assert!(rejected(&[("keyLength", json!(4)), ("keyPrefix", json!("abcdef01"))]));
        assert!(rejected(&[("keyLength", json!(32)), ("keyPrefix", json!("a".repeat(33)))]));
        assert!(!rejected(&[("keyLength", json!(32)), ("keyPrefix", json!("a".repeat(32)))]));
    }

    #[test]
    fn cost_and_key_length_follow_the_server_maximum() {
        assert!(!rejected(&[("cost", json!(1_280_000))]));
        assert!(rejected(&[("cost", json!(1_280_001))]));
        assert!(!rejected(&[("keyLength", json!(32))]));
        assert!(rejected(&[("keyLength", json!(33))]));
    }

    #[test]
    fn nonce_salt_and_token_size_are_capped() {
        assert!(!rejected(&[("nonce", json!("ab".repeat(64)))]));
        assert!(rejected(&[("nonce", json!("ab".repeat(65)))]));
        assert!(rejected(&[("salt", json!("ab".repeat(65)))]));
        assert!(rejected(&[("extra", json!("x".repeat(5000)))]));
    }

    #[test]
    fn expired_challenges_are_rejected() {
        assert!(rejected(&[("expiresAt", json!(1_000))]));
        assert!(rejected(&[("expiresAt", json!("soon"))]));
        assert!(!rejected(&[("expiresAt", json!(4_000_000_000u64))]));
        assert!(!rejected(&[("expiresAt", Value::Null)]));
    }

    #[test]
    fn cancel_flag_stops_the_search_promptly() {
        // Unreachable 8-hex prefix at a heavy cost: only cancel can end it.
        let challenge = params(&[("cost", json!(200_000)), ("keyPrefix", json!("ffffffff"))]);
        let cancel = AtomicBool::new(true);
        let started = Instant::now();
        let result = solve_cancellable(&challenge, u32::MAX, Duration::from_secs(60), &cancel);
        assert!(matches!(result, Err(RestError::Captcha(_))));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn deadline_is_checked_every_iteration() {
        let challenge = params(&[("cost", json!(1_000_000)), ("keyPrefix", json!("ffffffff"))]);
        let started = Instant::now();
        let result = solve_bounded(&challenge, u32::MAX, Duration::ZERO);
        assert!(matches!(result, Err(RestError::Captcha(_))));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn one_thread_finding_the_answer_stops_the_others() {
        // A one-hex-digit prefix is found almost immediately; the long
        // timeout and huge counter range must not matter.
        let challenge = params(&[("keyPrefix", json!("0"))]);
        let started = Instant::now();
        assert!(solve_bounded(&challenge, u32::MAX, Duration::from_secs(60)).is_ok());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn dropping_the_future_cancels_and_releases_the_limiter() {
        let limiter = Arc::new(Semaphore::new(1));
        let challenge = params(&[("cost", json!(100_000)), ("keyPrefix", json!("ffffffff"))]);
        let future = solve_off_executor(challenge, limiter.clone());
        let _ = tokio::time::timeout(Duration::from_millis(100), future).await;
        let permit = tokio::time::timeout(Duration::from_secs(10), limiter.acquire_owned()).await;
        assert!(matches!(permit, Ok(Ok(_))), "permit not released after cancel");
    }

    #[tokio::test]
    async fn only_one_solve_runs_at_a_time() {
        let limiter = Arc::new(Semaphore::new(1));
        let held = limiter.clone().acquire_owned().await;
        assert!(held.is_ok());
        let waiting = tokio::time::timeout(
            Duration::from_millis(100),
            solve_off_executor(params(&[("keyPrefix", json!("0"))]), limiter.clone()),
        )
        .await;
        assert!(waiting.is_err(), "second solve must wait for the permit");
    }
}
