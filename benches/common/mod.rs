//! Common benchmark infrastructure for ryg-rans-rs.
//!
//! Provides deterministic corpus generation, model construction, and
//! backend verification helpers shared across all Criterion benchmark tiers.

pub mod corpus;
pub mod metadata;
pub mod models;
pub mod verification;
