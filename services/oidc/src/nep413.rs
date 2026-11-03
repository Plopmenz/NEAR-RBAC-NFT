//! NEP-413 message construction and signature verification.
//!
//! A NEP-413 message is `tag || borsh(payload)` where `tag = 2^31 + 413` and the payload is
//! `{ message, nonce, recipient, callback_url }`. Wallets sign the SHA-256 hash of those bytes.
//! See <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>.

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use borsh::BorshSerialize;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// NEP-413 message tag: `2^31 + 413`.
pub const NEP413_TAG: u32 = (1 << 31) + 413;

#[derive(BorshSerialize)]
struct Payload<'a> {
    message: &'a str,
    nonce: [u8; 32],
    recipient: &'a str,
    callback_url: Option<&'a str>,
}

/// Errors produced while verifying a NEP-413 signature.
#[derive(Debug, Error)]
pub enum Nep413Error {
    #[error("public key must use the ed25519: prefix")]
    UnsupportedKey,
    #[error("public key is not valid base58")]
    InvalidKeyEncoding,
    #[error("public key is not a valid ed25519 key")]
    InvalidPublicKey,
    #[error("signature is not valid base64")]
    InvalidSignatureEncoding,
    #[error("signature is not a valid ed25519 signature")]
    InvalidSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

/// Serializes the bytes a NEAR wallet signs for a NEP-413 message.
pub fn signing_message(
    message: &str,
    nonce: [u8; 32],
    recipient: &str,
    callback_url: Option<&str>,
) -> Vec<u8> {
    let mut bytes = NEP413_TAG.to_le_bytes().to_vec();
    bytes.extend(
        borsh::to_vec(&Payload {
            message,
            nonce,
            recipient,
            callback_url,
        })
        .expect("NEP-413 payload serialization cannot fail"),
    );
    bytes
}

/// Returns the SHA-256 hash that a wallet signs for a NEP-413 payload.
pub fn signing_hash(
    message: &str,
    nonce: [u8; 32],
    recipient: &str,
    callback_url: Option<&str>,
) -> [u8; 32] {
    Sha256::digest(signing_message(message, nonce, recipient, callback_url)).into()
}

/// Verifies a NEP-413 signature.
///
/// `public_key` uses NEAR's `ed25519:<base58>` encoding and `signature` is base64. The account id
/// is not part of the signature; callers must separately verify that the public key belongs to
/// the claimed account (see [`crate::roles::RoleSource::has_access_key`]).
pub fn verify(
    public_key: &str,
    signature: &str,
    message: &str,
    nonce: [u8; 32],
    recipient: &str,
    callback_url: Option<&str>,
) -> Result<(), Nep413Error> {
    let encoded_key = public_key
        .strip_prefix("ed25519:")
        .ok_or(Nep413Error::UnsupportedKey)?;
    let key_bytes = bs58::decode(encoded_key)
        .into_vec()
        .map_err(|_| Nep413Error::InvalidKeyEncoding)?;
    let key_bytes: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| Nep413Error::InvalidPublicKey)?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|_| Nep413Error::InvalidPublicKey)?;

    let signature_bytes = decode_base64(signature).ok_or(Nep413Error::InvalidSignatureEncoding)?;
    let signature_bytes: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| Nep413Error::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature_bytes);

    let digest = signing_hash(message, nonce, recipient, callback_url);
    verifying_key
        .verify_strict(&digest, &signature)
        .map_err(|_| Nep413Error::VerificationFailed)
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(value)
        .or_else(|_| STANDARD_NO_PAD.decode(value))
        .or_else(|_| URL_SAFE.decode(value))
        .or_else(|_| URL_SAFE_NO_PAD.decode(value))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn keypair() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn public_key(key: &SigningKey) -> String {
        format!(
            "ed25519:{}",
            bs58::encode(key.verifying_key().to_bytes()).into_string()
        )
    }

    fn sign(key: &SigningKey, message: &str, nonce: [u8; 32], recipient: &str) -> String {
        let digest = signing_hash(message, nonce, recipient, None);
        STANDARD.encode(key.sign(&digest).to_bytes())
    }

    #[test]
    fn signing_message_has_the_nep413_tag() {
        let bytes = signing_message("hello", [1; 32], "auth.example.com", None);
        assert_eq!(&bytes[..4], &NEP413_TAG.to_le_bytes());
        assert!(bytes.len() > 4);
    }

    #[test]
    fn verifies_a_valid_signature() {
        let key = keypair();
        let nonce = [7u8; 32];
        let signature = sign(&key, "hello", nonce, "auth.example.com");
        verify(
            &public_key(&key),
            &signature,
            "hello",
            nonce,
            "auth.example.com",
            None,
        )
        .unwrap();
    }

    #[test]
    fn accepts_unpadded_base64_signatures() {
        let key = keypair();
        let nonce = [7u8; 32];
        let signature = sign(&key, "hello", nonce, "auth.example.com")
            .trim_end_matches('=')
            .to_owned();
        verify(
            &public_key(&key),
            &signature,
            "hello",
            nonce,
            "auth.example.com",
            None,
        )
        .unwrap();
    }

    #[test]
    fn rejects_a_message_mismatch() {
        let key = keypair();
        let nonce = [7u8; 32];
        let signature = sign(&key, "hello", nonce, "auth.example.com");
        assert!(matches!(
            verify(
                &public_key(&key),
                &signature,
                "goodbye",
                nonce,
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::VerificationFailed)
        ));
    }

    #[test]
    fn rejects_a_nonce_mismatch() {
        let key = keypair();
        let signature = sign(&key, "hello", [1; 32], "auth.example.com");
        assert!(matches!(
            verify(
                &public_key(&key),
                &signature,
                "hello",
                [2; 32],
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::VerificationFailed)
        ));
    }

    #[test]
    fn rejects_a_recipient_mismatch() {
        let key = keypair();
        let nonce = [7u8; 32];
        let signature = sign(&key, "hello", nonce, "auth.example.com");
        assert!(matches!(
            verify(
                &public_key(&key),
                &signature,
                "hello",
                nonce,
                "evil.example.com",
                None,
            ),
            Err(Nep413Error::VerificationFailed)
        ));
    }

    #[test]
    fn rejects_a_callback_url_mismatch() {
        let key = keypair();
        let nonce = [7u8; 32];
        let digest = signing_hash("hello", nonce, "auth.example.com", None);
        let signature = STANDARD.encode(key.sign(&digest).to_bytes());
        assert!(matches!(
            verify(
                &public_key(&key),
                &signature,
                "hello",
                nonce,
                "auth.example.com",
                Some("https://auth.example.com/callback"),
            ),
            Err(Nep413Error::VerificationFailed)
        ));
    }

    #[test]
    fn allows_callback_url_in_the_payload() {
        let key = keypair();
        let nonce = [7u8; 32];
        let digest = signing_hash(
            "hello",
            nonce,
            "auth.example.com",
            Some("https://auth.example.com/callback"),
        );
        let signature = STANDARD.encode(key.sign(&digest).to_bytes());
        verify(
            &public_key(&key),
            &signature,
            "hello",
            nonce,
            "auth.example.com",
            Some("https://auth.example.com/callback"),
        )
        .unwrap();
    }

    #[test]
    fn rejects_non_ed25519_keys() {
        assert!(matches!(
            verify(
                "secp256k1:abc",
                "AAAA",
                "hello",
                [0; 32],
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::UnsupportedKey)
        ));
    }

    #[test]
    fn rejects_malformed_keys_and_signatures() {
        assert!(matches!(
            verify(
                "ed25519:not-base58!",
                "AAAA",
                "hello",
                [0; 32],
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::InvalidKeyEncoding)
        ));
        assert!(matches!(
            verify(
                "ed25519:1111",
                "AAAA",
                "hello",
                [0; 32],
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::InvalidPublicKey)
        ));

        let key = keypair();
        assert!(matches!(
            verify(
                &public_key(&key),
                "not base64!",
                "hello",
                [0; 32],
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::InvalidSignatureEncoding)
        ));
        assert!(matches!(
            verify(
                &public_key(&key),
                "AAAA",
                "hello",
                [0; 32],
                "auth.example.com",
                None,
            ),
            Err(Nep413Error::InvalidSignature)
        ));
    }
}
