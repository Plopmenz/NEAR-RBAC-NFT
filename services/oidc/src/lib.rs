//! OIDC provider that authenticates NEAR accounts with NEP-413 and authorizes them using the
//! role NFTs held in the `rbac-nft` contract.
//!
//! The crate is split into small modules so that the protocol logic can be tested without a
//! network:
//!
//! - [`config`] parses the TOML configuration.
//! - [`keys`] owns the RSA signing key and JWKS.
//! - [`nep413`] builds and verifies NEP-413 messages.
//! - [`roles`] queries the RBAC contract over NEAR JSON-RPC.
//! - [`store`] holds pending logins, codes and sessions in memory.
//! - [`handlers`] implements the HTTP endpoints.
//!
//! See the repository README for a complete flow example and deployment notes.

pub mod clock;
pub mod config;
pub mod error;
pub mod handlers;
pub mod html;
pub mod keys;
pub mod nep413;
pub mod roles;
pub mod state;
pub mod store;
pub mod tokens;

pub use handlers::router as build_router;
pub use state::AppState;
