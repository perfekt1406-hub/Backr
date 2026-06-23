/*
 * Pairing code session.
 *
 * The 6-digit gate that authorizes exactly one laptop to be trusted on this host
 * during a time-boxed pairing window. Hardening that makes a short numeric code
 * safe on a shared LAN: single-use (consumed on first success), TTL expiry, and a
 * fixed wrong-attempt budget. Validation is constant-time so it can't leak how many
 * leading digits matched. The host shows `code()`; the listener calls `validate()`.
 */

use chrono::{DateTime, Duration, Utc};
use rand::Rng;

/// Pairing window length (~3 minutes).
pub const PAIRING_TTL_SECS: i64 = 180;
/// Wrong-attempt budget before the session locks.
pub const MAX_ATTEMPTS: u32 = 5;

/// Outcome of validating a submitted pairing code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeValidation {
    /// Code matched within TTL and attempt budget; the session is now consumed.
    Valid,
    /// Code did not match; carries the remaining attempt budget.
    Invalid { attempts_left: u32 },
    /// The pairing window has elapsed.
    Expired,
    /// Too many wrong attempts — the session is locked.
    Locked,
    /// The code was already used successfully (single-use).
    AlreadyUsed,
}

/// One active pairing window: a single-use, expiring, rate-limited 6-digit code.
pub struct PairingSession {
    code: String,
    /// Wall-clock expiry, surfaced to the UI for a countdown.
    pub expires_at: DateTime<Utc>,
    attempts_left: u32,
    consumed: bool,
}

impl PairingSession {
    /// Creates a fresh session with a random 6-digit code valid for `PAIRING_TTL_SECS`.
    pub fn new() -> Self {
        Self::with_now(Utc::now())
    }

    /// Testable constructor that pins the issue time so TTL behavior is deterministic.
    fn with_now(now: DateTime<Utc>) -> Self {
        // External: rand 0.9 rng().random_range — uniform 6-digit value in [0, 1_000_000).
        let code = format!("{:06}", rand::rng().random_range(0..1_000_000u32));
        Self {
            code,
            expires_at: now + Duration::seconds(PAIRING_TTL_SECS),
            attempts_left: MAX_ATTEMPTS,
            consumed: false,
        }
    }

    /// The 6-digit code to display on the host.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// True once the pairing window has lapsed at `now`.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }

    /// Validates a submitted code at the current time, enforcing single-use, TTL,
    /// and the attempt budget. A correct code consumes the session.
    pub fn validate(&mut self, input: &str) -> CodeValidation {
        self.validate_at(input, Utc::now())
    }

    /// Time-injected core of [`validate`] for deterministic tests.
    fn validate_at(&mut self, input: &str, now: DateTime<Utc>) -> CodeValidation {
        if self.consumed {
            return CodeValidation::AlreadyUsed;
        }
        if self.is_expired(now) {
            return CodeValidation::Expired;
        }
        if self.attempts_left == 0 {
            return CodeValidation::Locked;
        }
        if constant_time_eq(input.as_bytes(), self.code.as_bytes()) {
            self.consumed = true;
            CodeValidation::Valid
        } else {
            self.attempts_left -= 1;
            CodeValidation::Invalid {
                attempts_left: self.attempts_left,
            }
        }
    }
}

/// Length-aware constant-time byte comparison — folds all bytes so timing does not
/// reveal how many leading digits matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns a 6-digit string guaranteed different from `code`.
    fn wrong_code(code: &str) -> String {
        let n: u32 = code.parse().unwrap();
        format!("{:06}", (n + 1) % 1_000_000)
    }

    #[test]
    fn code_is_six_ascii_digits() {
        let s = PairingSession::new();
        assert_eq!(s.code().len(), 6);
        assert!(s.code().bytes().all(|b| b.is_ascii_digit()));
    }

    #[test]
    fn correct_code_is_valid_then_single_use() {
        let mut s = PairingSession::new();
        let code = s.code().to_string();
        assert_eq!(s.validate(&code), CodeValidation::Valid);
        // Single-use: a second submission of the same correct code is rejected.
        assert_eq!(s.validate(&code), CodeValidation::AlreadyUsed);
    }

    #[test]
    fn wrong_code_decrements_attempts() {
        let mut s = PairingSession::new();
        let bad = wrong_code(s.code());
        assert_eq!(
            s.validate(&bad),
            CodeValidation::Invalid {
                attempts_left: MAX_ATTEMPTS - 1
            }
        );
    }

    #[test]
    fn locks_after_max_attempts_even_with_correct_code() {
        let mut s = PairingSession::new();
        let code = s.code().to_string();
        let bad = wrong_code(&code);
        for _ in 0..MAX_ATTEMPTS {
            let _ = s.validate(&bad);
        }
        // Budget exhausted: even the correct code is now locked out.
        assert_eq!(s.validate(&code), CodeValidation::Locked);
    }

    #[test]
    fn expires_after_ttl() {
        let t0 = Utc::now();
        let mut s = PairingSession::with_now(t0);
        let code = s.code().to_string();
        let after = t0 + Duration::seconds(PAIRING_TTL_SECS + 1);
        assert_eq!(s.validate_at(&code, after), CodeValidation::Expired);
    }

    #[test]
    fn valid_just_before_expiry() {
        let t0 = Utc::now();
        let mut s = PairingSession::with_now(t0);
        let code = s.code().to_string();
        let before = t0 + Duration::seconds(PAIRING_TTL_SECS - 1);
        assert_eq!(s.validate_at(&code, before), CodeValidation::Valid);
    }

    #[test]
    fn constant_time_eq_matches_only_on_full_equality() {
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        assert!(!constant_time_eq(b"023456", b"123456"));
        assert!(!constant_time_eq(b"12345", b"123456")); // length mismatch
    }
}
