//! Capability extractor — parses `CAPABILITY.toml` from repos.

use crate::github::RepoInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single capability declared by a repo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    /// Machine-readable capability name, e.g. `si-auth`.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Semantic version string.
    #[serde(default)]
    pub version: String,
    /// Capabilities this repo *provides* to others.
    #[serde(default)]
    pub provides: Vec<String>,
    /// Capabilities this repo *depends on*.
    #[serde(default)]
    pub depends: Vec<String>,
    /// Optional tags / keywords.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The top-level structure of a `CAPABILITY.toml` file.
#[derive(Debug, Deserialize)]
pub struct CapabilityToml {
    #[serde(default)]
    pub capability: Vec<Capability>,
    /// Legacy single-capability format.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub provides: Option<Vec<String>>,
    #[serde(default)]
    pub depends: Option<Vec<String>>,
}

/// Extract capabilities from a repo's `CAPABILITY.toml` content.
///
/// If the repo has no `CAPABILITY.toml`, returns an empty vec.
pub fn extract_capabilities_from_content(content: &str) -> Vec<Capability> {
    let parsed: Result<CapabilityToml, _> = toml::from_str(content);
    match parsed {
        Ok(toml) => {
            if !toml.capability.is_empty() {
                toml.capability
            } else if let Some(name) = toml.name {
                vec![Capability {
                    name,
                    description: toml.description.unwrap_or_default(),
                    version: toml.version.unwrap_or_default(),
                    provides: toml.provides.unwrap_or_default(),
                    depends: toml.depends.unwrap_or_default(),
                    tags: vec![],
                }]
            } else {
                vec![]
            }
        }
        Err(_) => vec![],
    }
}

/// Extract capabilities from a [`RepoInfo`].
///
/// This is a convenience wrapper that reads `has_capability_toml` and returns
/// an empty vec if the repo has no capability file. In a real scenario the
/// caller would first fetch the file content via `GitHubScanner::fetch_file_content`.
pub fn extract_capabilities(_repo: &RepoInfo, content: Option<&str>) -> Vec<Capability> {
    match content {
        Some(c) => extract_capabilities_from_content(c),
        None => vec![],
    }
}

/// Extract capabilities for many repos at once.
pub fn extract_all(
    repos: &[RepoInfo],
    contents: &HashMap<String, String>,
) -> HashMap<String, Vec<Capability>> {
    let mut map = HashMap::new();
    for repo in repos {
        let caps = match contents.get(&repo.name) {
            Some(c) => extract_capabilities_from_content(c),
            None => vec![],
        };
        map.insert(repo.name.clone(), caps);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::RepoInfo;
    use chrono::Utc;

    fn mock_repo(name: &str) -> RepoInfo {
        RepoInfo {
            name: name.to_owned(),
            description: Some("test".into()),
            language: Some("Rust".into()),
            topics: vec![],
            updated_at: Utc::now(),
            has_readme: true,
            has_capability_toml: true,
            default_branch: Some("main".into()),
            archived: false,
            fork: false,
            stars: 0,
            html_url: None,
        }
    }

    #[test]
    fn test_extract_multi_capability() {
        let content = r#"
[[capability]]
name = "si-auth"
description = "Authentication service"
version = "1.0.0"
provides = ["auth", "oauth2"]
depends = ["si-config"]

[[capability]]
name = "si-metrics"
description = "Metrics collector"
version = "0.2.0"
provides = ["metrics"]
depends = []
"#;
        let caps = extract_capabilities_from_content(content);
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0].name, "si-auth");
        assert_eq!(caps[0].provides, vec!["auth", "oauth2"]);
        assert_eq!(caps[0].depends, vec!["si-config"]);
        assert_eq!(caps[1].name, "si-metrics");
    }

    #[test]
    fn test_extract_legacy_single() {
        let content = r#"
name = "si-gateway"
description = "API Gateway"
version = "2.0.0"
provides = ["gateway", "routing"]
depends = ["si-auth"]
"#;
        let caps = extract_capabilities_from_content(content);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].name, "si-gateway");
        assert_eq!(caps[0].provides, vec!["gateway", "routing"]);
    }

    #[test]
    fn test_extract_empty() {
        let caps = extract_capabilities_from_content("");
        assert!(caps.is_empty());
    }

    #[test]
    fn test_extract_invalid_toml() {
        let caps = extract_capabilities_from_content("this is not valid [[[");
        assert!(caps.is_empty());
    }

    #[test]
    fn test_extract_none_content() {
        let repo = mock_repo("empty");
        let caps = extract_capabilities(&repo, None);
        assert!(caps.is_empty());
    }

    #[test]
    fn test_extract_with_content() {
        let repo = mock_repo("has-cap");
        let content = r#"
[[capability]]
name = "si-test"
description = "Test"
version = "0.1.0"
provides = ["testing"]
depends = []
"#;
        let caps = extract_capabilities(&repo, Some(content));
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].name, "si-test");
    }

    #[test]
    fn test_extract_all() {
        let r1 = mock_repo("repo-a");
        let r2 = mock_repo("repo-b");
        let mut contents = HashMap::new();
        contents.insert(
            "repo-a".to_owned(),
            r#"[[capability]]
name = "cap-a"
provides = ["a"]
"#
            .to_owned(),
        );
        // repo-b has no content entry
        let map = extract_all(&[r1, r2], &contents);
        assert_eq!(map.len(), 2);
        assert_eq!(map["repo-a"].len(), 1);
        assert!(map["repo-b"].is_empty());
    }

    #[test]
    fn test_capability_tags() {
        let content = r#"
[[capability]]
name = "si-tagged"
tags = ["infra", "core"]
"#;
        let caps = extract_capabilities_from_content(content);
        assert_eq!(caps[0].tags, vec!["infra", "core"]);
    }

    #[test]
    fn test_capability_serde_roundtrip() {
        let cap = Capability {
            name: "si-test".into(),
            description: "desc".into(),
            version: "1.0.0".into(),
            provides: vec!["a".into()],
            depends: vec!["b".into()],
            tags: vec![],
        };
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }
}
