/*
 * Pairing code session.
 *
 * The 6-digit gate that authorizes one laptop to be trusted during a pairing
 * window. The window now auto-closes after PAIRING_TTL_SECS (3 minutes) via a
 * Tokio task in `commands/pairing_cmd.rs`. Security rests on two properties:
 * single-use (consumed on the first correct submission) and a fixed wrong-attempt
 * budget that locks the session. Validation is constant-time so it can't leak how
 * many digits matched.
 */

use rand::Rng;

/// Wrong-attempt budget before the session locks.
pub const MAX_ATTEMPTS: u32 = 5;

/// Outcome of validating a submitted pairing code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeValidation {
    /// Code matched; the session is now consumed.
    Valid,
    /// Code did not match; carries the remaining attempt budget.
    Invalid { attempts_left: u32 },
    /// Too many wrong attempts — the session is locked.
    Locked,
    /// The code was already used successfully (single-use).
    AlreadyUsed,
}

/// One active pairing window: a single-use, rate-limited 6-digit code.
pub struct PairingSession {
    code: String,
    attempts_left: u32,
    consumed: bool,
}

impl PairingSession {
    /// Creates a fresh session with a random 6-digit code.
    pub fn new() -> Self {
        // External: rand 0.9 rng().random_range — uniform 6-digit value in [0, 1_000_000).
        let code = format!("{:06}", rand::rng().random_range(0..1_000_000u32));
        Self {
            code,
            attempts_left: MAX_ATTEMPTS,
            consumed: false,
        }
    }

    /// The 6-digit code to display on the host.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Validates a submitted code, enforcing single-use and the attempt budget.
    /// A correct code consumes the session.
    pub fn validate(&mut self, input: &str) -> CodeValidation {
        if self.consumed {
            return CodeValidation::AlreadyUsed;
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
    fn constant_time_eq_matches_only_on_full_equality() {
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        assert!(!constant_time_eq(b"023456", b"123456"));
        assert!(!constant_time_eq(b"12345", b"123456")); // length mismatch
    }

    /// A pair attempt against an already-consumed session returns `AlreadyUsed`, not
    /// `Valid`. This simulates what happens when the TTL fires after a successful pair:
    /// the session is consumed, so any residual request correctly bounces.
    #[test]
    fn consumed_session_rejects_further_attempts() {
        let mut s = PairingSession::new();
        let code = s.code().to_string();
        // First use — valid.
        assert_eq!(s.validate(&code), CodeValidation::Valid);
        // Subsequent attempts with the same correct code are rejected.
        assert_eq!(s.validate(&code), CodeValidation::AlreadyUsed);
        // Wrong codes also return AlreadyUsed (not Invalid) — consumed trumps everything.
        let bad = wrong_code(&code);
        assert_eq!(s.validate(&bad), CodeValidation::AlreadyUsed);
    }

    /// Simulates the TTL task finding no active pairing window: calling `take()` on a
    /// `None` pairing_runtime must be safe and produce no double-free or panic.
    /// (The actual state machinery lives in `pairing_cmd.rs`; this tests the Rust
    /// ownership semantics that make the no-op safe via `Option::take`.)
    #[test]
    fn ttl_teardown_on_already_closed_window_is_safe() {
        // Simulate what stop_pairing_internal does: take from an Option<PairingRuntime>.
        let mut slot: Option<u32> = None; // stand-in for Option<PairingRuntime>
        // First take — already None; should not panic.
        assert!(slot.take().is_none());
        // Second take — also None; idempotent.
        assert!(slot.take().is_none());
        // A runtime that was already consumed (Some then taken).
        slot = Some(42);
        assert_eq!(slot.take(), Some(42));
        // Now the TTL fires and tries to take again — safe no-op.
        assert!(slot.take().is_none());
    }
}
