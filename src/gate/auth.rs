use super::GateScope;

/// Generate a dynamic confirmation phrase based on the scope.
///
/// Examples: "PURGE 482 OBJECTS", "DELETE 3 OBJECTS"
pub fn generate_confirmation_phrase(scope: &GateScope) -> String {
    let action = scope.action.to_uppercase();
    format!("{} {} OBJECTS", action, scope.object_count)
}

/// Verify a confirmation phrase matches the expected value (exact match).
pub fn verify_phrase(input: &str, expected: &str) -> bool {
    input.trim() == expected
}

/// Verify a passphrase against a stored argon2 hash.
pub fn verify_passphrase(input: &str, stored_hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let parsed = match PasswordHash::new(stored_hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(input.as_bytes(), &parsed)
        .is_ok()
}

/// Hash a new passphrase for storage using argon2.
pub fn hash_passphrase(passphrase: &str) -> Result<String, String> {
    use argon2::{
        password_hash::{rand_core::OsRng, SaltString},
        Argon2, PasswordHasher,
    };

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(passphrase.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_generation() {
        let scope = GateScope {
            action: "purge".to_string(),
            object_count: 482,
            total_bytes: 0,
            protected_count: 0,
            examples: vec![],
        };
        assert_eq!(generate_confirmation_phrase(&scope), "PURGE 482 OBJECTS");
    }

    #[test]
    fn phrase_verification_exact() {
        assert!(verify_phrase("PURGE 482 OBJECTS", "PURGE 482 OBJECTS"));
    }

    #[test]
    fn phrase_verification_with_whitespace() {
        assert!(verify_phrase("  PURGE 482 OBJECTS  ", "PURGE 482 OBJECTS"));
    }

    #[test]
    fn phrase_verification_mismatch() {
        assert!(!verify_phrase("PURGE 481 OBJECTS", "PURGE 482 OBJECTS"));
    }

    #[test]
    fn phrase_verification_case_sensitive() {
        assert!(!verify_phrase("purge 482 objects", "PURGE 482 OBJECTS"));
    }

    #[test]
    fn passphrase_hash_and_verify() {
        let passphrase = "my-secret-gate-passphrase";
        let hash = hash_passphrase(passphrase).expect("hashing should succeed");

        assert!(verify_passphrase(passphrase, &hash));
        assert!(!verify_passphrase("wrong-passphrase", &hash));
    }

    #[test]
    fn verify_passphrase_invalid_hash() {
        assert!(!verify_passphrase("anything", "not-a-valid-hash"));
    }
}
