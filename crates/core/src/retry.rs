//! Tries a network call again after a short wait when it failed for a reason that usually passes.

use std::time::Duration;

/// Waits between attempts for a live call: a line of transcript is not worth a long delay.
pub const LIVE_DELAYS: &[Duration] = &[Duration::from_secs(1), Duration::from_secs(3)];
/// Waits for notes and catch-ups, which may sit in a busy provider's queue.
pub const PATIENT_DELAYS: &[Duration] = &[Duration::from_secs(1), Duration::from_secs(3), Duration::from_secs(8)];

/// Runs `operation`, and again after each wait in `delays` while it fails with an error `is_transient` accepts.
pub fn with_retries<T, E>(
    delays: &[Duration],
    is_transient: impl Fn(&E) -> bool,
    mut operation: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut waits = delays.iter();
    loop {
        match operation() {
            Err(error) if is_transient(&error) => match waits.next() {
                Some(wait) => std::thread::sleep(*wait),
                None => return Err(error),
            },
            result => return result,
        }
    }
}

/// Statuses worth another try: timeouts, rate limits and server trouble.
pub fn is_transient_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429) || (500..=599).contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_WAIT: &[Duration] = &[Duration::ZERO, Duration::ZERO];

    #[test]
    fn a_transient_failure_is_tried_again_until_it_works() {
        let mut calls = 0;
        let result = with_retries(NO_WAIT, |_: &&str| true, || {
            calls += 1;
            if calls < 3 {
                Err("busy")
            } else {
                Ok(calls)
            }
        });
        assert_eq!(result, Ok(3));
    }

    #[test]
    fn it_gives_up_after_the_last_wait() {
        let mut calls = 0;
        let result: Result<(), _> = with_retries(NO_WAIT, |_: &&str| true, || {
            calls += 1;
            Err("down")
        });
        assert_eq!((result, calls), (Err("down"), 3));
    }

    #[test]
    fn a_permanent_failure_is_not_repeated() {
        let mut calls = 0;
        let result: Result<(), _> = with_retries(NO_WAIT, |_: &&str| false, || {
            calls += 1;
            Err("bad key")
        });
        assert_eq!((result, calls), (Err("bad key"), 1));
    }

    #[test]
    fn statuses() {
        assert!(is_transient_status(429) && is_transient_status(503));
        assert!(!is_transient_status(401) && !is_transient_status(400));
    }
}
