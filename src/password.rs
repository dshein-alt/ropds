use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::RngExt;

/// Hash a plaintext password using Argon2id with a random salt.
pub fn hash(password: &str) -> String {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("Argon2 hashing should not fail")
        .to_string()
}

/// Generate a 16-character cryptographically random alphanumeric password
/// for use as an OAuth user's OPDS Basic Auth credential.
pub fn generate_opds_password() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..16)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
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

    #[test]
    fn test_generate_opds_password_length_and_charset() {
        let pw = generate_opds_password();
        assert_eq!(pw.len(), 16);
        assert!(pw.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_generate_opds_password_unique() {
        let a = generate_opds_password();
        let b = generate_opds_password();
        assert_ne!(a, b, "passwords should be random");
    }
}
