//! RSA signing keys and JWKS generation.

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header};
use rand::rngs::OsRng;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePublicKey};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// A JWK set containing the provider's RS256 signing key.
#[derive(Clone, Debug, Serialize)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

/// A single RSA public JWK.
#[derive(Clone, Debug, Serialize)]
pub struct Jwk {
    pub kty: &'static str,
    #[serde(rename = "use")]
    pub use_: &'static str,
    pub alg: &'static str,
    pub kid: String,
    pub n: String,
    pub e: String,
}

/// Signing material used to issue and validate JWTs.
#[derive(Clone)]
pub struct KeyMaterial {
    kid: String,
    encoding: EncodingKey,
    decoding: DecodingKey,
    jwks: Jwks,
}

impl KeyMaterial {
    /// Generates a fresh 2048-bit RSA key. Intended for development and tests.
    pub fn generate() -> Result<Self> {
        let private = RsaPrivateKey::new(&mut OsRng, 2048)
            .context("failed to generate an RSA signing key")?;
        Self::from_private_key(private)
    }

    /// Loads a PKCS#8 PEM private key.
    pub fn from_pem(pem: &str) -> Result<Self> {
        let private = RsaPrivateKey::from_pkcs8_pem(pem)
            .context("failed to parse the RSA private key; expected PKCS#8 PEM")?;
        Self::from_private_key(private)
    }

    fn from_private_key(private: RsaPrivateKey) -> Result<Self> {
        let public = RsaPublicKey::from(&private);
        let n = public.n().to_bytes_be();
        let e = public.e().to_bytes_be();

        let public_der = public
            .to_public_key_der()
            .context("failed to encode the RSA public key")?;
        let digest = Sha256::digest(public_der.as_bytes());
        let kid = hex::encode(&digest[..8]);

        let private_der = private
            .to_pkcs1_der()
            .context("failed to encode the RSA private key")?;

        Ok(Self {
            kid: kid.clone(),
            encoding: EncodingKey::from_rsa_der(private_der.as_bytes()),
            decoding: DecodingKey::from_rsa_raw_components(&n, &e),
            jwks: Jwks {
                keys: vec![Jwk {
                    kty: "RSA",
                    use_: "sig",
                    alg: "RS256",
                    kid,
                    n: URL_SAFE_NO_PAD.encode(n),
                    e: URL_SAFE_NO_PAD.encode(e),
                }],
            },
        })
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn jwks(&self) -> &Jwks {
        &self.jwks
    }

    pub fn header(&self) -> Header {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.kid.clone());
        header
    }

    pub fn encode<T: serde::Serialize>(&self, claims: &T) -> Result<String> {
        jsonwebtoken::encode(&self.header(), claims, &self.encoding).context("failed to sign JWT")
    }

    /// Decodes and validates a token issued by this provider.
    ///
    /// The audience is checked by the caller because different token types use different
    /// audiences. Issuer and expiry are validated here.
    pub fn decode<T: serde::de::DeserializeOwned>(&self, token: &str, issuer: &str) -> Result<T> {
        let mut validation = jsonwebtoken::Validation::new(Algorithm::RS256);
        validation.set_issuer(&[issuer]);
        validation.validate_aud = false;
        validation.leeway = 30;
        let data = jsonwebtoken::decode::<T>(token, &self.decoding, &validation)
            .context("invalid or expired JWT")?;
        Ok(data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::OnceLock;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestClaims {
        iss: String,
        sub: String,
        exp: u64,
    }

    pub(crate) fn test_keys() -> KeyMaterial {
        static KEYS: OnceLock<KeyMaterial> = OnceLock::new();
        KEYS.get_or_init(|| KeyMaterial::generate().unwrap())
            .clone()
    }

    #[test]
    fn signs_and_verifies_tokens() {
        let keys = test_keys();
        let claims = TestClaims {
            iss: "https://auth.example.com".to_owned(),
            sub: "alice.test.near".to_owned(),
            exp: u64::MAX / 2,
        };
        let token = keys.encode(&claims).unwrap();
        let decoded: TestClaims = keys.decode(&token, "https://auth.example.com").unwrap();
        assert_eq!(decoded, claims);
    }

    #[test]
    fn rejects_tokens_from_another_issuer() {
        let keys = test_keys();
        let claims = TestClaims {
            iss: "https://evil.example.com".to_owned(),
            sub: "alice.test.near".to_owned(),
            exp: u64::MAX / 2,
        };
        let token = keys.encode(&claims).unwrap();
        assert!(keys
            .decode::<TestClaims>(&token, "https://auth.example.com")
            .is_err());
    }

    #[test]
    fn jwks_contains_rsa_components() {
        let keys = test_keys();
        let jwks = keys.jwks();
        assert_eq!(jwks.keys.len(), 1);
        let jwk = &jwks.keys[0];
        assert_eq!(jwk.kty, "RSA");
        assert_eq!(jwk.alg, "RS256");
        assert_eq!(jwk.kid, keys.kid());
        assert!(!jwk.n.is_empty());
        assert!(!jwk.e.is_empty());
        assert!(!jwk.n.contains('='));
    }
}
