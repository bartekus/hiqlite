use crate::Error;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordVerifier, Version};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task;

// Very simple rate limiting, as upstream 0.15.0 has it (`044` B-7). The dashboard is meant for
// debugging only, so a single password hash at a time is enough: one permit bounds the argon2
// work (32 MiB and two threads per hash) whatever the number of concurrent attempts, and a
// failed attempt is answered only after a delay, with the permit already released.
//
// This replaces two mechanisms that did not agree. F-084: the `RwLock` guard here was bound to
// `_` and released before any hashing (repaired in `030` by naming the binding). F-087: a
// process-global cooldown in `session.rs` rejected **every** login for five seconds after any
// failure, so one unauthenticated request every five seconds locked the operator out. A
// per-failure delay costs the failing caller its own time and nobody else's.
static HASH_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));

/// How long a caller waits for the hashing permit before being told to retry.
const PERMIT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a failed attempt waits, after releasing the permit, before it is answered.
const FAILED_LOGIN_DELAY: Duration = Duration::from_secs(3);

pub async fn verify_password(plain: String, hash: String) -> Result<(), Error> {
    verify_with(&HASH_PERMITS, plain, hash, FAILED_LOGIN_DELAY).await
}

async fn verify_with(
    permits: &Semaphore,
    plain: String,
    hash: String,
    failure_delay: Duration,
) -> Result<(), Error> {
    // F-084 stays repaired: the permit is a named binding, held until the hash is verified.
    let permit = tokio::time::timeout(PERMIT_TIMEOUT, permits.acquire())
        .await
        .map_err(|_| {
            Error::RateLimit("timeout while waiting for a password hashing permit".into())
        })?
        .map_err(|err| {
            Error::RateLimit(
                format!("error while waiting for a password hashing permit: {err}").into(),
            )
        })?;

    let res = task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash)?;
        build_hasher().verify_password(plain.as_bytes(), &parsed_hash)?;
        Ok::<(), Error>(())
    })
    .await;
    drop(permit);

    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => {
            // The permit is released first, so the next caller's hash is not held behind this
            // caller's penalty; this caller alone waits.
            tokio::time::sleep(failure_delay).await;
            Err(Error::Unauthorized("unauthorized".into()))
        }
        Err(err) => Err(err.into()),
    }
}

pub fn build_hasher<'a>() -> Argon2<'a> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(32_768, 2, 2, Some(32)).unwrap(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::PasswordHasher;
    use argon2::password_hash::SaltString;
    use std::sync::Arc;
    use std::time::Instant;

    // Every test uses its own semaphore, so none of them shares state with another or with a
    // parallel run: F-095 was three tests racing over one process-global.

    fn hash_of(password: &str) -> String {
        let salt = SaltString::from_b64("c29tZXNhbHRzb21lc2FsdA").unwrap();
        build_hasher()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    /// Replaces `the_single_flight_lock_is_held_for_the_whole_hashing` and
    /// `verify_password_holds_the_lock_while_it_runs`, which asserted the `RwLock` guard this
    /// replaced. F-084 in its new form: the permit is held while the hash runs.
    #[tokio::test]
    async fn the_hashing_permit_is_held_while_the_hash_runs() {
        let permits = Arc::new(Semaphore::new(1));
        let hash = hash_of("correct horse battery");

        let p = permits.clone();
        let task = tokio::spawn(async move {
            verify_with(&p, "correct horse battery".into(), hash, Duration::ZERO).await
        });

        let mut held = false;
        for _ in 0..400 {
            if permits.available_permits() == 0 {
                held = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        task.await.unwrap().expect("the right password verifies");
        assert!(held, "verify_password must hold the hashing permit");
        assert_eq!(permits.available_permits(), 1, "and release it afterwards");
    }

    /// F-087 (`044` B-7): a failed attempt is delayed, and the delay is the failing caller's
    /// alone. While a wrong password waits out its penalty, the permit is free and the right
    /// password verifies at once. Under the process-global cooldown this replaced, the second
    /// login was answered `429` without being checked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_attempt_is_delayed_without_locking_anyone_else_out() {
        let permits = Arc::new(Semaphore::new(1));
        let hash = hash_of("correct horse battery");
        let delay = Duration::from_secs(3);

        let (p, h) = (permits.clone(), hash.clone());
        let started = Instant::now();
        let wrong = tokio::spawn(async move { verify_with(&p, "wrong".into(), h, delay).await });

        // Wait until the wrong attempt has taken the permit and then given it back, which is
        // when it is serving its delay.
        let mut taken = false;
        let mut released = false;
        for _ in 0..2000 {
            if permits.available_permits() == 0 {
                taken = true;
            } else if taken {
                released = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            taken && released,
            "the failing attempt never released its permit"
        );
        assert!(
            !wrong.is_finished(),
            "the failing attempt is still serving its delay"
        );

        verify_with(&permits, "correct horse battery".into(), hash, delay)
            .await
            .expect("the operator is not locked out by someone else's failure");
        assert!(
            !wrong.is_finished(),
            "and got in before the failing attempt was answered"
        );

        let err = wrong
            .await
            .unwrap()
            .expect_err("the wrong password is refused");
        assert!(matches!(err, Error::Unauthorized(_)), "got: {err:?}");
        assert!(
            started.elapsed() >= delay,
            "the failure was answered only after the delay"
        );
    }

    /// No process-global cooldown remains for a test to race over, and the session handler no
    /// longer answers a login before checking it.
    #[test]
    fn there_is_no_global_login_cooldown() {
        let session = include_str!("session.rs");
        for gone in [
            "NEXT_LOGIN_ALLOWED",
            "LOGIN_COOLDOWN",
            "fn lock_logins",
            "TOO_MANY_REQUESTS",
        ] {
            assert!(!session.contains(gone), "session.rs still has {gone}");
        }
    }

    /// The hashing parameters are part of the contract and are pinned here so a
    /// change to them is a deliberate one.
    #[test]
    fn the_hasher_is_argon2id_with_recorded_parameters() {
        let hasher = build_hasher();
        let params = hasher.params();
        assert_eq!(params.m_cost(), 32_768);
        assert_eq!(params.t_cost(), 2);
        assert_eq!(params.p_cost(), 2);
        assert_eq!(params.output_len(), Some(32));
    }
}
