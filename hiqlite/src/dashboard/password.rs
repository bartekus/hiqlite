use crate::Error;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordVerifier, Version};
use std::sync::LazyLock;
use tokio::sync::RwLock;
use tokio::task;

// very simple way to rate-limit password hashing / login.
// only a single password hash at a time is allowed, the dashboard is just for debugging.
// prevents brute-fore effectively.
static IS_HASHING: LazyLock<RwLock<()>> = LazyLock::new(|| RwLock::new(()));

pub async fn verify_password(plain: String, hash: String) -> Result<(), Error> {
    // F-084: this was `let _ = IS_HASHING.write().await;`. A `_` pattern drops its value at
    // the end of the statement, so the guard was released immediately and the lock serialized
    // nothing. The comment above it described a rate limit that did not exist, and the
    // parameters below allocate 32 MiB and two threads per concurrent attempt, so N
    // unauthenticated logins allocated N times that.
    let _guard = IS_HASHING.write().await;

    task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash)?;
        build_hasher().verify_password(plain.as_bytes(), &parsed_hash)?;
        Ok::<(), Error>(())
    })
    .await??;

    Ok(())
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

    /// Replaces `the_single_flight_lock_is_released_before_any_hashing`, which pinned F-084 by
    /// asserting that the form `verify_password` used released its guard immediately. It does;
    /// `verify_password` no longer uses it.
    ///
    /// The distinction is the whole defect, so both forms stay asserted: a `_` pattern drops
    /// its value at the end of the statement, a named binding holds it to the end of the scope.
    #[tokio::test]
    async fn the_single_flight_lock_is_held_for_the_whole_hashing() {
        // The form the function used to open with, and what it does.
        {
            let _ = IS_HASHING.write().await;
            assert!(
                IS_HASHING.try_write().is_ok(),
                "`let _ = ...write().await` drops the guard at the end of the statement"
            );
        }

        // The form it opens with now.
        {
            let _guard = IS_HASHING.write().await;
            assert!(
                IS_HASHING.try_write().is_err(),
                "a named binding holds it, which is what makes this a single-flight lock"
            );
        }
        assert!(IS_HASHING.try_write().is_ok(), "and releases it at the end");
    }

    /// The function itself holds the lock while it works.
    ///
    /// The point of F-084 is the parameters below: 32 MiB and two threads per concurrent
    /// attempt, from an unauthenticated endpoint, with a lock that serialized nothing.
    #[tokio::test]
    async fn verify_password_holds_the_lock_while_it_runs() {
        // A hash that will fail to verify, which is all this needs: the lock is taken before
        // any of that.
        let hash = "$argon2id$v=19$m=32768,t=2,p=2$c29tZXNhbHQ$\
                    aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhcw"
            .to_string();

        let task = tokio::spawn(verify_password("wrong".to_string(), hash));

        // Give the task a moment to reach the lock, then assert it is held. Bounded, and the
        // assertion is on the lock rather than on timing: if the task has not started yet the
        // loop tries again, and if it never takes the lock the test fails at the end.
        let mut held = false;
        for _ in 0..200 {
            if IS_HASHING.try_write().is_err() {
                held = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let _ = task.await;
        assert!(held, "verify_password must hold the single-flight lock");
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
