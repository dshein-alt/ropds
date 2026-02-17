use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

/// Hash a plaintext password using Argon2id with a random salt.
pub fn hash(password: &str) -> String {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("Argon2 hashing should not fail")
        .to_string()
}

/// Verify a plaintext password against a stored hash.
pub fn verify(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let pw = "correct-horse-battery-staple";
        let hashed = hash(pw);
        assert!(hashed.starts_with("$argon2"));
        assert!(verify(pw, &hashed));
    }

    #[test]
    fn test_wrong_password() {
        let hashed = hash("real-password");
        assert!(!verify("wrong-password", &hashed));
    }

    #[test]
    fn test_garbage_hash() {
        assert!(!verify("anything", "not-a-valid-hash"));
        assert!(!verify("anything", ""));
    }

    #[test]
    fn test_different_hashes_for_same_password() {
        let h1 = hash("same");
        let h2 = hash("same");
        assert_ne!(h1, h2); // different salts
        assert!(verify("same", &h1));
        assert!(verify("same", &h2));
    }
}
