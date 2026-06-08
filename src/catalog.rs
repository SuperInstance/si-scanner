//! Fleet catalog — aggregates repos, capabilities, and the dependency graph.

use crate::extractor::{extract_all, Capability};
use crate::github::{GitHubScanner, RepoInfo};
use crate::graph::DependencyGraph;
use std::collections::HashMap;

/// Summary statistics about the fleet.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CatalogStats {
    pub total_repos: usize,
    pub total_capabilities: usize,
    pub languages: HashMap<String, usize>,
    pub repos_with_capability_toml: usize,
    pub repos_without_capability_toml: usize,
    pub total_stars: u32,
    pub top_starred: Vec<(String, u32)>,
}

/// The fleet-wide catalog — the main aggregation point.
pub struct FleetCatalog {
    pub repos: Vec<RepoInfo>,
    pub capabilities: HashMap<String, Vec<Capability>>,
    pub graph: DependencyGraph,
}

impl FleetCatalog {
    /// Build a catalog from scratch by scanning a GitHub org or user.
    ///
    /// `target` is parsed: if it starts with `org:` the rest is treated as an
    /// org name; if it starts with `user:` it is treated as a user. Otherwise
    /// it defaults to an org scan.
    pub fn from_scan(
        scanner: &GitHubScanner,
        target: &str,
        capability_contents: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let repos = if let Some(org) = target.strip_prefix("org:") {
            scanner.scan_org(org)?
        } else if let Some(user) = target.strip_prefix("user:") {
            scanner.scan_user(user)?
        } else {
            scanner.scan_org(target)?
        };

        Ok(Self::from_repos(repos, capability_contents))
    }

    /// Build a catalog from an existing list of repos and their CAPABILITY.toml contents.
    pub fn from_repos(repos: Vec<RepoInfo>, contents: &HashMap<String, String>) -> Self {
        let capabilities = extract_all(&repos, contents);

        let mut graph = DependencyGraph::new();
        for repo in &repos {
            let caps = capabilities.get(&repo.name).cloned().unwrap_or_default();
            graph.add_repo(repo, &caps);
        }

        Self {
            repos,
            capabilities,
            graph,
        }
    }

    /// Search repos by name or description (case-insensitive substring match).
    pub fn search(&self, query: &str) -> Vec<&RepoInfo> {
        let q = query.to_lowercase();
        self.repos
            .iter()
            .filter(|r| {
                r.name.to_lowercase().contains(&q)
                    || r
                        .description
                        .as_ref()
                        .map_or(false, |d| d.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// Get repos written in a specific language.
    pub fn by_language(&self, lang: &str) -> Vec<&RepoInfo> {
        self.repos
            .iter()
            .filter(|r| r.language.as_deref() == Some(lang))
            .collect()
    }

    /// Get repos that declare a specific capability.
    pub fn by_capability(&self, cap: &str) -> Vec<&RepoInfo> {
        let providers = self.graph.find_providers(cap);
        self.repos
            .iter()
            .filter(|r| providers.contains(&r.name))
            .collect()
    }

    /// Compute summary statistics.
    pub fn stats(&self) -> CatalogStats {
        let mut languages: HashMap<String, usize> = HashMap::new();
        let mut total_stars = 0u32;
        let mut with_cap = 0usize;
        let mut without_cap = 0usize;
        let mut star_list: Vec<(String, u32)> = Vec::new();

        for repo in &self.repos {
            let lang = repo.language.clone().unwrap_or_else(|| "Unknown".to_owned());
            *languages.entry(lang).or_insert(0) += 1;
            total_stars += repo.stars;
            star_list.push((repo.name.clone(), repo.stars));
            if repo.has_capability_toml {
                with_cap += 1;
            } else {
                without_cap += 1;
            }
        }

        star_list.sort_by(|a, b| b.1.cmp(&a.1));
        let top_starred = star_list.into_iter().take(10).collect();

        CatalogStats {
            total_repos: self.repos.len(),
            total_capabilities: self.graph.capability_count(),
            languages,
            repos_with_capability_toml: with_cap,
            repos_without_capability_toml: without_cap,
            total_stars,
            top_starred,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn repo(name: &str, lang: &str, desc: &str, stars: u32, has_cap: bool) -> RepoInfo {
        RepoInfo {
            name: name.to_owned(),
            description: if desc.is_empty() {
                None
            } else {
                Some(desc.to_owned())
            },
            language: if lang.is_empty() {
                None
            } else {
                Some(lang.to_owned())
            },
            topics: vec![],
            updated_at: Utc::now(),
            has_readme: true,
            has_capability_toml: has_cap,
            default_branch: Some("main".into()),
            archived: false,
            fork: false,
            stars,
            html_url: None,
        }
    }

    fn cap_content(name: &str, provides: &[&str], depends: &[&str]) -> String {
        format!(
            r#"
[[capability]]
name = "{}"
provides = {:?}
depends = {:?}
"#,
            name,
            provides,
            depends
        )
    }

    #[test]
    fn test_from_repos_basic() {
        let repos = vec![
            repo("si-core", "Rust", "Core library", 50, true),
            repo("si-web", "TypeScript", "Web frontend", 30, false),
        ];
        let contents = HashMap::new();
        let cat = FleetCatalog::from_repos(repos, &contents);
        assert_eq!(cat.repos.len(), 2);
    }

    #[test]
    fn test_search_by_name() {
        let repos = vec![
            repo("si-auth", "Rust", "Auth service", 10, true),
            repo("si-gateway", "Go", "API Gateway", 20, true),
        ];
        let cat = FleetCatalog::from_repos(repos, &HashMap::new());
        let results = cat.search("auth");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "si-auth");
    }

    #[test]
    fn test_search_by_description() {
        let repos = vec![
            repo("r1", "Rust", "Authentication module", 0, false),
            repo("r2", "Go", "Networking stack", 0, false),
        ];
        let cat = FleetCatalog::from_repos(repos, &HashMap::new());
        let results = cat.search("auth");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "r1");
    }

    #[test]
    fn test_search_case_insensitive() {
        let repos = vec![repo("Rust-Toolkit", "Rust", "A TOOL", 0, false)];
        let cat = FleetCatalog::from_repos(repos, &HashMap::new());
        assert_eq!(cat.search("tool").len(), 1);
        assert_eq!(cat.search("rust-toolkit").len(), 1);
    }

    #[test]
    fn test_by_language() {
        let repos = vec![
            repo("r1", "Rust", "", 0, false),
            repo("r2", "Rust", "", 0, false),
            repo("r3", "Go", "", 0, false),
        ];
        let cat = FleetCatalog::from_repos(repos, &HashMap::new());
        assert_eq!(cat.by_language("Rust").len(), 2);
        assert_eq!(cat.by_language("Go").len(), 1);
        assert_eq!(cat.by_language("Python").len(), 0);
    }

    #[test]
    fn test_by_capability() {
        let repos = vec![
            repo("si-core", "Rust", "", 0, true),
            repo("si-auth", "Rust", "", 0, true),
        ];
        let mut contents = HashMap::new();
        contents.insert(
            "si-core".to_owned(),
            cap_content("si-core", &["config"], &[]),
        );
        contents.insert(
            "si-auth".to_owned(),
            cap_content("si-auth", &["auth"], &["config"]),
        );
        let cat = FleetCatalog::from_repos(repos, &contents);
        let providers = cat.by_capability("config");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "si-core");
    }

    #[test]
    fn test_stats() {
        let repos = vec![
            repo("popular", "Rust", "Big project", 500, true),
            repo("small", "TypeScript", "Tiny", 5, false),
            repo("medium", "Rust", "Medium", 50, true),
        ];
        let cat = FleetCatalog::from_repos(repos, &HashMap::new());
        let stats = cat.stats();
        assert_eq!(stats.total_repos, 3);
        assert_eq!(stats.total_stars, 555);
        assert_eq!(stats.repos_with_capability_toml, 2);
        assert_eq!(stats.repos_without_capability_toml, 1);
        assert_eq!(*stats.languages.get("Rust").unwrap(), 2);
        assert_eq!(*stats.languages.get("TypeScript").unwrap(), 1);
        assert_eq!(stats.top_starred[0], ("popular".to_owned(), 500));
    }

    #[test]
    fn test_empty_catalog() {
        let cat = FleetCatalog::from_repos(vec![], &HashMap::new());
        assert_eq!(cat.repos.len(), 0);
        assert!(cat.search("anything").is_empty());
        assert!(cat.by_language("Rust").is_empty());
        let stats = cat.stats();
        assert_eq!(stats.total_repos, 0);
        assert_eq!(stats.total_stars, 0);
    }

    #[test]
    fn test_stats_top_starred_truncation() {
        let repos: Vec<RepoInfo> = (0..20)
            .map(|i| repo(&format!("r-{i}"), "Rust", "", (20 - i) as u32, false))
            .collect();
        let cat = FleetCatalog::from_repos(repos, &HashMap::new());
        let stats = cat.stats();
        assert_eq!(stats.top_starred.len(), 10);
        assert_eq!(stats.top_starred[0].1, 20);
    }
}
