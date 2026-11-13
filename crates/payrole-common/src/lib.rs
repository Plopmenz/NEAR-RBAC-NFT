//! Stable role identifiers shared by the on-chain PayRole contract and off-chain services.
//!
//! NEP-245 identifies tokens with a string `token_id` and groups them with a base-metadata
//! identifier. This project uses one token type per role: the decimal representation of the
//! 64-bit role id is both the token id and the base-metadata id.
//!
//! The role id is the first eight bytes (big-endian) of `SHA-256(role_name)`. It is deliberately
//! stable and documented: the contract, OIDC service, indexers, and clients must all derive the
//! same role id. The contract still detects collisions when a role is first configured and
//! refuses to merge two different role names.

use sha2::{Digest, Sha256};

/// Maximum accepted length of a role name, in bytes.
pub const MAX_ROLE_NAME_LEN: usize = 64;

/// Errors returned when validating a role name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleNameError {
    /// The role name is empty.
    Empty,
    /// The role name is longer than [`MAX_ROLE_NAME_LEN`] bytes.
    TooLong,
    /// The role name has leading or trailing whitespace.
    SurroundingWhitespace,
    /// The role name contains an ASCII control character.
    ControlCharacter,
}

impl core::fmt::Display for RoleNameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("role name must not be empty"),
            Self::TooLong => write!(f, "role name must be at most {MAX_ROLE_NAME_LEN} bytes"),
            Self::SurroundingWhitespace => {
                f.write_str("role name must not have leading or trailing whitespace")
            }
            Self::ControlCharacter => f.write_str("role name must not contain control characters"),
        }
    }
}

impl std::error::Error for RoleNameError {}

/// Validates a human-readable role name.
///
/// Role names are case-sensitive and are hashed byte-for-byte, so `Admin` and `admin` are
/// different roles.
pub fn validate_role_name(role: &str) -> Result<(), RoleNameError> {
    if role.is_empty() {
        return Err(RoleNameError::Empty);
    }
    if role.len() > MAX_ROLE_NAME_LEN {
        return Err(RoleNameError::TooLong);
    }
    if role.trim() != role {
        return Err(RoleNameError::SurroundingWhitespace);
    }
    if role.chars().any(|c| c.is_control()) {
        return Err(RoleNameError::ControlCharacter);
    }
    Ok(())
}

/// Returns the stable 64-bit id of a role name.
///
/// This is `u64::from_be_bytes(SHA-256(role_name)[0..8])`.
pub fn role_id(role: &str) -> u64 {
    let digest = Sha256::digest(role.as_bytes());
    u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}

/// Returns the NEP-245 token id for a role.
///
/// The token id is the decimal representation of the role id. An account's membership in a role
/// is a balance of `1` on this token type, not a separate token.
pub fn token_id(role_id: u64) -> String {
    role_id.to_string()
}

/// Returns the base-metadata id used for a role by the NEP-245 metadata extension.
///
/// It is the same value as the token id.
pub fn base_metadata_id(role_id: u64) -> String {
    token_id(role_id)
}

/// Parses the decimal role id out of a token id.
///
/// Returns `None` for malformed token ids.
pub fn parse_token_id(token_id: &str) -> Option<u64> {
    token_id.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ids_match_documented_test_vectors() {
        assert_eq!(role_id("admin"), 10_117_748_766_825_055_253);
        assert_eq!(role_id("employee"), 3_448_633_025_363_786_332);
        assert_eq!(role_id("manager"), 7_990_692_413_185_102_085);
        assert_eq!(role_id("hr.manager"), 6_339_347_599_350_065_209);
        assert_eq!(role_id("Treasury"), 7_701_258_499_861_874_806);
    }

    #[test]
    fn role_ids_are_case_sensitive() {
        assert_ne!(role_id("Admin"), role_id("admin"));
    }

    #[test]
    fn token_id_round_trips() {
        let id = token_id(role_id("admin"));
        assert_eq!(id, "10117748766825055253");
        assert_eq!(parse_token_id(&id), Some(role_id("admin")));
    }

    #[test]
    fn token_id_rejects_malformed_values() {
        assert_eq!(parse_token_id(""), None);
        assert_eq!(parse_token_id("admin"), None);
        assert_eq!(parse_token_id("1:alice.near"), None);
        assert_eq!(parse_token_id("-1"), None);
        assert_eq!(parse_token_id("18446744073709551616"), None);
    }

    #[test]
    fn base_metadata_id_is_the_token_id() {
        assert_eq!(
            base_metadata_id(role_id("admin")),
            token_id(role_id("admin"))
        );
        assert_eq!(base_metadata_id(role_id("admin")), "10117748766825055253");
    }

    #[test]
    fn role_name_validation() {
        assert_eq!(validate_role_name("admin"), Ok(()));
        assert_eq!(validate_role_name("hr.manager"), Ok(()));
        assert_eq!(validate_role_name("line manager"), Ok(()));
        assert_eq!(validate_role_name(""), Err(RoleNameError::Empty));
        assert_eq!(
            validate_role_name(" admin"),
            Err(RoleNameError::SurroundingWhitespace)
        );
        assert_eq!(
            validate_role_name("admin\n"),
            Err(RoleNameError::SurroundingWhitespace)
        );
        assert_eq!(
            validate_role_name("adm\u{0000}in"),
            Err(RoleNameError::ControlCharacter)
        );
        assert_eq!(
            validate_role_name(&"a".repeat(MAX_ROLE_NAME_LEN + 1)),
            Err(RoleNameError::TooLong)
        );
    }
}
