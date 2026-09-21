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
    let _ = IS_HASHING.write().await;

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

    /// F-084. `verify_password` opens with `let _ = IS_HASHING.write().await;`.
    /// A `_` pattern drops its value at the end of the statement, so the write
    /// guard is released immediately and the lock serializes nothing. This
    /// asserts both halves against the same static: the form the function uses,
    /// and the binding form that would hold it.
    #[tokio::test]
    async fn the_single_flight_lock_is_released_before_any_hashing() {
        // exactly the line `verify_password` opens with
        let _ = IS_HASHING.write().await;
        assert!(
            IS_HASHING.try_write().is_ok(),
            "F-084: `let _ = ...write().await` drops the guard at the end of the \
             statement, so nothing is serialized"
        );

        // the form that does hold it
        let guard = IS_HASHING.write().await;
        assert!(IS_HASHING.try_write().is_err());
        drop(guard);
        assert!(IS_HASHING.try_write().is_ok());
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
