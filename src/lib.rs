//! # si-scanner
//!
//! Fleet-wide discovery and cataloging for SuperInstance.
//!
//! Scan a GitHub organization or user, extract capabilities from `CAPABILITY.toml`
//! files, build a dependency graph, and generate reports in Markdown, JSON, or
//! Graphviz DOT format.

pub mod catalog;
pub mod extractor;
pub mod github;
pub mod graph;
pub mod report;

pub use catalog::{CatalogStats, FleetCatalog};
pub use extractor::Capability;
pub use github::RepoInfo;
pub use graph::DependencyGraph;
