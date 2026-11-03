//! NEP-245 metadata types.
//!
//! The role name is exposed as the base-token name. Individual tokens are the memberships of an
//! account in a role, so their token metadata describes the membership (issue time, salary, role).

use near_sdk::near;

/// Contract-level NEP-245 metadata.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MTContractMetadata {
    pub spec: String,
    pub name: String,
}

/// NEP-245 base-token metadata (one per role).
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MTBaseTokenMetadata {
    pub name: String,
    pub id: String,
    pub symbol: Option<String>,
    pub icon: Option<String>,
    pub decimals: Option<String>,
    pub base_uri: Option<String>,
    pub reference: Option<String>,
    pub copies: Option<u64>,
    pub reference_hash: Option<String>,
}

/// NEP-245 instance token metadata (one per membership).
#[near(serializers = [json])]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MTTokenMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub media: Option<String>,
    pub media_hash: Option<String>,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub starts_at: Option<String>,
    pub updated_at: Option<String>,
    pub extra: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

/// Combined base and instance metadata.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MTTokenMetadataAll {
    pub base: MTBaseTokenMetadata,
    pub token: MTTokenMetadata,
}

/// A token in NEP-245's core/enumeration shape.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MTRoleToken {
    pub token_id: String,
    pub owner_id: Option<near_sdk::AccountId>,
    pub base_metadata_id: String,
    pub token_metadata: Option<MTTokenMetadata>,
}
