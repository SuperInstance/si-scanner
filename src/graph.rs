//! Dependency graph builder — tracks which repos provide / depend on which capabilities.

use crate::extractor::Capability;
use crate::github::RepoInfo;
use std::collections::{HashMap, HashSet};

/// Directed dependency graph across the fleet.
pub struct DependencyGraph {
    /// capability → set of repos that provide it.
    providers: HashMap<String, HashSet<String>>,
    /// capability → set of repos that depend on it.
    dependents: HashMap<String, HashSet<String>>,
    /// repo → set of capabilities it depends on.
    repo_deps: HashMap<String, HashSet<String>>,
    /// repo → set of capabilities it provides.
    repo_provides: HashMap<String, HashSet<String>>,
    /// All known repo names.
    repos: HashSet<String>,
}

impl DependencyGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            dependents: HashMap::new(),
            repo_deps: HashMap::new(),
            repo_provides: HashMap::new(),
            repos: HashSet::new(),
        }
    }

    /// Add a repo and its capabilities to the graph.
    pub fn add_repo(&mut self, repo: &RepoInfo, caps: &[Capability]) {
        let name = repo.name.clone();
        self.repos.insert(name.clone());

        let mut my_provides = HashSet::new();
        let mut my_deps = HashSet::new();

        for cap in caps {
            for p in &cap.provides {
                self.providers
                    .entry(p.clone())
                    .or_default()
                    .insert(name.clone());
                my_provides.insert(p.clone());
            }
            for d in &cap.depends {
                self.dependents
                    .entry(d.clone())
                    .or_default()
                    .insert(name.clone());
                my_deps.insert(d.clone());
            }
        }

        self.repo_provides.insert(name.clone(), my_provides);
        self.repo_deps.insert(name, my_deps);
    }

    /// Find all repos that depend on the given capability.
    pub fn find_dependents(&self, capability: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .dependents
            .get(capability)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Find all repos that provide the given capability.
    pub fn find_providers(&self, capability: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .providers
            .get(capability)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Return a topological ordering of repos (providers before dependents).
    ///
    /// If a cycle is detected, the ordering is still returned (best-effort).
    /// Repos with no dependencies or whose dependencies are not in the fleet
    /// are placed first.
    pub fn topological_sort(&self) -> Vec<String> {
        // Build an adjacency list: repo → set of repos it depends on.
        let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
        for repo in &self.repos {
            let deps = self.repo_deps.get(repo).cloned().unwrap_or_default();
            let mut dep_repos = HashSet::new();
            for dep_cap in deps {
                if let Some(provs) = self.providers.get(&dep_cap) {
                    for p in provs {
                        if p != repo {
                            dep_repos.insert(p.clone());
                        }
                    }
                }
            }
            adj.insert(repo.clone(), dep_repos);
        }

        // Kahn's algorithm
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for repo in &self.repos {
            in_degree.entry(repo.as_str()).or_insert(0);
        }
        for (_repo, deps) in &adj {
            for _dep in deps {
                // dep is depended-upon, so reverse edge: dep → repo
                // We want in_degree[repo]++ for each dep it has
            }
        }
        // Recompute: in_degree[repo] = number of deps that repo has (within fleet)
        for repo in &self.repos {
            let deg = adj
                .get(repo.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            in_degree.insert(repo.as_str(), deg);
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&r, _)| r)
            .collect();
        queue.sort();
        queue.reverse(); // pop() takes from end = smallest first

        // Build reverse adjacency for Kahn's
        let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
        for repo in &self.repos {
            if let Some(deps) = adj.get(repo.as_str()) {
                for dep in deps {
                    reverse.entry(dep.as_str()).or_default().push(repo.as_str());
                }
            }
        }

        let mut result = Vec::new();
        while let Some(node) = queue.pop() {
            result.push(node.to_owned());
            if let Some(neighbors) = reverse.get(node) {
                for &nbr in neighbors {
                    let deg = in_degree.get_mut(nbr).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        // Insert to maintain reverse-sorted order (smallest at end for pop)
                        let search = queue.binary_search(&nbr);
                        let pos = search.unwrap_err();
                        queue.insert(pos, nbr);
                    }
                }
            }
        }

        // Any remaining are in cycles — add them sorted at the end.
        let mut remaining: Vec<String> = self
            .repos
            .iter()
            .filter(|r| !result.contains(*r))
            .cloned()
            .collect();
        remaining.sort();
        result.extend(remaining);

        result
    }

    /// Get the set of capabilities a repo provides.
    pub fn repo_provides(&self, repo: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .repo_provides
            .get(repo)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Get the set of capabilities a repo depends on.
    pub fn repo_depends(&self, repo: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .repo_deps
            .get(repo)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Total number of repos in the graph.
    pub fn repo_count(&self) -> usize {
        self.repos.len()
    }

    /// Total number of unique capabilities tracked.
    pub fn capability_count(&self) -> usize {
        self.providers.keys().chain(self.dependents.keys()).collect::<HashSet<_>>().len()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::RepoInfo;
    use chrono::Utc;

    fn mock_repo(name: &str) -> RepoInfo {
        RepoInfo {
            name: name.to_owned(),
            description: None,
            language: Some("Rust".into()),
            topics: vec![],
            updated_at: Utc::now(),
            has_readme: true,
            has_capability_toml: false,
            default_branch: Some("main".into()),
            archived: false,
            fork: false,
            stars: 0,
            html_url: None,
        }
    }

    fn cap(name: &str, provides: &[&str], depends: &[&str]) -> Capability {
        Capability {
            name: name.to_owned(),
            description: String::new(),
            version: "1.0.0".into(),
            provides: provides.iter().map(|s| s.to_string()).collect(),
            depends: depends.iter().map(|s| s.to_string()).collect(),
            tags: vec![],
        }
    }

    #[test]
    fn test_empty_graph() {
        let g = DependencyGraph::new();
        assert_eq!(g.repo_count(), 0);
        assert_eq!(g.capability_count(), 0);
        assert!(g.topological_sort().is_empty());
    }

    #[test]
    fn test_single_repo() {
        let mut g = DependencyGraph::new();
        let r = mock_repo("core");
        g.add_repo(&r, &[cap("si-core", &["config"], &[])]);
        assert_eq!(g.repo_count(), 1);
        assert_eq!(g.find_providers("config"), vec!["core"]);
        assert!(g.find_dependents("config").is_empty());
    }

    #[test]
    fn test_dependency_chain() {
        let mut g = DependencyGraph::new();
        let core = mock_repo("si-core");
        let auth = mock_repo("si-auth");
        let gateway = mock_repo("si-gateway");

        g.add_repo(&core, &[cap("si-core", &["config", "logging"], &[])]);
        g.add_repo(&auth, &[cap("si-auth", &["auth"], &["config"])]);
        g.add_repo(&gateway, &[cap("si-gateway", &["gateway"], &["auth", "logging"])]);

        assert_eq!(g.find_providers("config"), vec!["si-core"]);
        assert_eq!(g.find_dependents("config"), vec!["si-auth"]);
        assert_eq!(g.find_dependents("auth"), vec!["si-gateway"]);
        assert_eq!(g.find_dependents("logging"), vec!["si-gateway"]);

        let topo = g.topological_sort();
        assert_eq!(topo.len(), 3);
        // si-core should come before si-auth
        assert!(topo.iter().position(|r| r == "si-core").unwrap()
            < topo.iter().position(|r| r == "si-auth").unwrap());
        // si-auth should come before si-gateway
        assert!(topo.iter().position(|r| r == "si-auth").unwrap()
            < topo.iter().position(|r| r == "si-gateway").unwrap());
    }

    #[test]
    fn test_multiple_providers() {
        let mut g = DependencyGraph::new();
        let r1 = mock_repo("provider-a");
        let r2 = mock_repo("provider-b");
        g.add_repo(&r1, &[cap("cap-a", &["cache"], &[])]);
        g.add_repo(&r2, &[cap("cap-b", &["cache"], &[])]);

        let provs = g.find_providers("cache");
        assert_eq!(provs.len(), 2);
        assert!(provs.contains(&"provider-a".to_string()));
        assert!(provs.contains(&"provider-b".to_string()));
    }

    #[test]
    fn test_repo_provides_and_depends() {
        let mut g = DependencyGraph::new();
        let r = mock_repo("my-repo");
        g.add_repo(&r, &[cap("cap", &["x", "y"], &["z"])]);

        let provides = g.repo_provides("my-repo");
        assert_eq!(provides, vec!["x", "y"]);

        let depends = g.repo_depends("my-repo");
        assert_eq!(depends, vec!["z"]);
    }

    #[test]
    fn test_unknown_capability() {
        let g = DependencyGraph::new();
        assert!(g.find_providers("nonexistent").is_empty());
        assert!(g.find_dependents("nonexistent").is_empty());
    }

    #[test]
    fn test_topological_sort_independent() {
        let mut g = DependencyGraph::new();
        g.add_repo(&mock_repo("a"), &[cap("a", &["cap-a"], &[])]);
        g.add_repo(&mock_repo("b"), &[cap("b", &["cap-b"], &[])]);
        g.add_repo(&mock_repo("c"), &[cap("c", &["cap-c"], &[])]);

        let topo = g.topological_sort();
        assert_eq!(topo.len(), 3);
        // All independent, should just be sorted alphabetically
        assert_eq!(topo, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_cycle_best_effort() {
        let mut g = DependencyGraph::new();
        let a = mock_repo("cycle-a");
        let b = mock_repo("cycle-b");
        // a depends on "x" provided by b, b depends on "y" provided by a
        g.add_repo(&a, &[cap("ca", &["y"], &["x"])]);
        g.add_repo(&b, &[cap("cb", &["x"], &["y"])]);

        let topo = g.topological_sort();
        // Should still return both, even though there's a cycle
        assert_eq!(topo.len(), 2);
    }

    #[test]
    fn test_default_trait() {
        let g = DependencyGraph::default();
        assert_eq!(g.repo_count(), 0);
    }
}
