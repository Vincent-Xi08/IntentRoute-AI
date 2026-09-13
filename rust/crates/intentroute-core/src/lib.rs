//! IntentRoute AI deterministic core (Rust migration, phase 1).
//!
//! Ports the pure decision logic of the WPF product so both the future Rust
//! GUI and tooling share one implementation:
//!
//! - [`ProxyMode`] / [`ProxyRule`] — the persisted rule model, JSON-compatible
//!   with the C# `AppConfigStore` format (numeric enums, required `Id`).
//! - [`constraint`] — host / IP-CIDR / port list validation with the same
//!   semantics as `RuleConstraintValidator`.
//! - [`identity`] — the full rule identity key shared by AI dedupe and import.
//! - [`import_plan`] — import classification (add / already-present skip /
//!   in-file duplicate skip) identical to `RuleImportPlanner`.
//! - [`runtime_order`] — the Canonical Runtime Order (priority, creation
//!   timestamp, persisted order).
//! - [`builder`] — the sing-box 1.13+ configuration builder (phase 2),
//!   including loopback-only upstream validation and password redaction.

pub mod builder;
pub mod config;
pub mod constraint;
pub mod identity;
pub mod import_plan;
pub mod local_endpoint;
pub mod runtime_order;
pub mod rule;

pub use builder::{build as build_sing_box_config, BuildResult, DIRECT_TAG};
pub use config::{AppConfig, GlobalMode, ProxyServer, ProxyType};
pub use identity::rule_identity_key;
pub use import_plan::{ImportDisposition, ImportPlan, ImportPreviewRow, plan_import};
pub use local_endpoint::try_normalize as try_normalize_local_endpoint;
pub use rule::{ProxyMode, ProxyRule};
pub use runtime_order::canonical_order;
