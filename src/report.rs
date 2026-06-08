//! Output formatting — Markdown, JSON, and Graphviz DOT reports.

use crate::catalog::FleetCatalog;

/// Render the catalog as a Markdown document.
pub fn catalog_markdown(catalog: &FleetCatalog) -> String {
    let mut md = String::new();

    md.push_str("# SuperInstance Fleet Catalog\n\n");

    // Stats
    let stats = catalog.stats();
    md.push_str("## Overview\n\n");
    md.push_str(&format!("- **Total repos:** {}\n", stats.total_repos));
    md.push_str(&format!(
        "- **Unique capabilities:** {}\n",
        stats.total_capabilities
    ));
    md.push_str(&format!("- **Total stars:** {}\n", stats.total_stars));
    md.push_str(&format!(
        "- **With CAPABILITY.toml:** {}\n",
        stats.repos_with_capability_toml
    ));
    md.push_str(&format!(
        "- **Without CAPABILITY.toml:** {}\n\n",
        stats.repos_without_capability_toml
    ));

    // Languages
    md.push_str("## Languages\n\n");
    md.push_str("| Language | Repos |\n|----------|-------|\n");
    let mut langs: Vec<_> = stats.languages.iter().collect();
    langs.sort_by(|a, b| b.1.cmp(a.1));
    for (lang, count) in langs {
        md.push_str(&format!("| {} | {} |\n", lang, count));
    }
    md.push('\n');

    // Top starred
    md.push_str("## Top Starred\n\n");
    for (i, (name, stars)) in stats.top_starred.iter().enumerate() {
        md.push_str(&format!("{}. **{}** (⭐ {})\n", i + 1, name, stars));
    }
    md.push('\n');

    // Repos
    md.push_str("## Repositories\n\n");
    let topo = catalog.graph.topological_sort();
    let repos_by_name: std::collections::HashMap<&str, &crate::github::RepoInfo> =
        catalog.repos.iter().map(|r| (r.name.as_str(), r)).collect();

    for name in &topo {
        if let Some(repo) = repos_by_name.get(name.as_str()) {
            md.push_str(&format!("### {}\n\n", repo.name));
            if let Some(desc) = &repo.description {
                md.push_str(&format!("> {}\n\n", desc));
            }
            md.push_str(&format!(
                "- **Language:** {}\n",
                repo.language.as_deref().unwrap_or("Unknown")
            ));
            md.push_str(&format!("- **Stars:** {}\n", repo.stars));
            md.push_str(&format!(
                "- **CAPABILITY.toml:** {}\n",
                if repo.has_capability_toml { "✅" } else { "❌" }
            ));
            if !repo.topics.is_empty() {
                md.push_str(&format!("- **Topics:** {}\n", repo.topics.join(", ")));
            }

            let provides = catalog.graph.repo_provides(&repo.name);
            let depends = catalog.graph.repo_depends(&repo.name);
            if !provides.is_empty() {
                md.push_str(&format!("- **Provides:** {}\n", provides.join(", ")));
            }
            if !depends.is_empty() {
                md.push_str(&format!("- **Depends on:** {}\n", depends.join(", ")));
            }
            md.push('\n');
        }
    }

    // Dependency graph
    md.push_str("## Dependency Graph\n\n");
    md.push_str("```\n");
    for name in &topo {
        let depends = catalog.graph.repo_depends(name);
        if !depends.is_empty() {
            md.push_str(&format!(
                "{name} → [{}]\n",
                depends
                    .iter()
                    .map(|d| {
                        let provs = catalog.graph.find_providers(d);
                        if provs.is_empty() {
                            d.clone()
                        } else {
                            provs.join("|")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    md.push_str("```\n");

    md
}

/// Render the catalog as a JSON document.
pub fn catalog_json(catalog: &FleetCatalog) -> String {
    let stats = catalog.stats();
    let topo = catalog.graph.topological_sort();

    let repos_json: Vec<serde_json::Value> = catalog
        .repos
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "description": r.description,
                "language": r.language,
                "topics": r.topics,
                "stars": r.stars,
                "has_capability_toml": r.has_capability_toml,
                "provides": catalog.graph.repo_provides(&r.name),
                "depends": catalog.graph.repo_depends(&r.name),
            })
        })
        .collect();

    let caps_json: Vec<serde_json::Value> = catalog
        .capabilities
        .iter()
        .flat_map(|(_repo, caps)| caps.iter())
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "description": c.description,
                "version": c.version,
                "provides": c.provides,
                "depends": c.depends,
            })
        })
        .collect();

    let json = serde_json::json!({
        "stats": {
            "total_repos": stats.total_repos,
            "total_capabilities": stats.total_capabilities,
            "total_stars": stats.total_stars,
            "languages": stats.languages,
        },
        "topological_order": topo,
        "repos": repos_json,
        "capabilities": caps_json,
    });

    serde_json::to_string_pretty(&json).unwrap_or_default()
}

/// Render the dependency graph in Graphviz DOT format.
pub fn dependency_dot(catalog: &FleetCatalog) -> String {
    let mut dot = String::from("digraph fleet {\n");
    dot.push_str("    rankdir=LR;\n");
    dot.push_str("    node [shape=box, style=filled, fillcolor=\"#f8f8f8\"];\n");
    dot.push_str("    edge [color=\"#666666\"];\n\n");

    // Node declarations
    for repo in &catalog.repos {
        let label = &repo.name;
        let lang = repo.language.as_deref().unwrap_or("Unknown");
        let color = match lang {
            "Rust" => "#dea584",
            "Go" => "#00ADD8",
            "TypeScript" => "#3178c6",
            "Python" => "#3572A5",
            "JavaScript" => "#f1e05a",
            _ => "#e8e8e8",
        };
        dot.push_str(&format!(
            "    \"{label}\" [label=\"{label}\\n({lang})\", fillcolor=\"{color}\"];\n"
        ));
    }
    dot.push('\n');

    // Edges: repo → provider-repo for each dependency
    for repo in &catalog.repos {
        let depends = catalog.graph.repo_depends(&repo.name);
        for dep_cap in &depends {
            let providers = catalog.graph.find_providers(dep_cap);
            for prov in &providers {
                dot.push_str(&format!(
                    "    \"{}\" -> \"{}\" [label=\"{}\"];\n",
                    repo.name, prov, dep_cap
                ));
            }
        }
    }

    dot.push_str("}\n");
    dot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FleetCatalog;
    use std::collections::HashMap;

    fn make_catalog() -> FleetCatalog {
        use crate::github::RepoInfo;
        use chrono::Utc;

        let repos = vec![
            RepoInfo {
                name: "si-core".into(),
                description: Some("Core library".into()),
                language: Some("Rust".into()),
                topics: vec!["core".into()],
                updated_at: Utc::now(),
                has_readme: true,
                has_capability_toml: true,
                default_branch: Some("main".into()),
                archived: false,
                fork: false,
                stars: 100,
                html_url: Some("https://github.com/SuperInstance/si-core".into()),
            },
            RepoInfo {
                name: "si-auth".into(),
                description: Some("Auth service".into()),
                language: Some("Rust".into()),
                topics: vec!["auth".into()],
                updated_at: Utc::now(),
                has_readme: true,
                has_capability_toml: true,
                default_branch: Some("main".into()),
                archived: false,
                fork: false,
                stars: 50,
                html_url: None,
            },
            RepoInfo {
                name: "si-web".into(),
                description: Some("Web UI".into()),
                language: Some("TypeScript".into()),
                topics: vec![],
                updated_at: Utc::now(),
                has_readme: true,
                has_capability_toml: false,
                default_branch: Some("main".into()),
                archived: false,
                fork: false,
                stars: 30,
                html_url: None,
            },
        ];

        let mut contents = HashMap::new();
        contents.insert(
            "si-core".into(),
            r#"[[capability]]
name = "si-core"
provides = ["config", "logging"]
depends = []
"#
            .into(),
        );
        contents.insert(
            "si-auth".into(),
            r#"[[capability]]
name = "si-auth"
provides = ["auth"]
depends = ["config"]
"#
            .into(),
        );

        FleetCatalog::from_repos(repos, &contents)
    }

    #[test]
    fn test_markdown_basic() {
        let cat = make_catalog();
        let md = catalog_markdown(&cat);
        assert!(md.contains("# SuperInstance Fleet Catalog"));
        assert!(md.contains("si-core"));
        assert!(md.contains("si-auth"));
        assert!(md.contains("si-web"));
        assert!(md.contains("## Languages"));
        assert!(md.contains("## Top Starred"));
    }

    #[test]
    fn test_markdown_contains_provides() {
        let cat = make_catalog();
        let md = catalog_markdown(&cat);
        assert!(md.contains("config"));
        assert!(md.contains("logging"));
    }

    #[test]
    fn test_json_valid() {
        let cat = make_catalog();
        let json = catalog_json(&cat);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["stats"]["total_repos"], 3);
        assert!(parsed["repos"].as_array().unwrap().len() == 3);
    }

    #[test]
    fn test_json_contains_capabilities() {
        let cat = make_catalog();
        let json = catalog_json(&cat);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let caps = parsed["capabilities"].as_array().unwrap();
        assert!(!caps.is_empty());
    }

    #[test]
    fn test_dot_format() {
        let cat = make_catalog();
        let dot = dependency_dot(&cat);
        assert!(dot.starts_with("digraph fleet {"));
        assert!(dot.contains("\"si-auth\" -> \"si-core\""));
        assert!(dot.contains("rankdir=LR"));
    }

    #[test]
    fn test_dot_node_labels() {
        let cat = make_catalog();
        let dot = dependency_dot(&cat);
        assert!(dot.contains("si-core"));
        assert!(dot.contains("Rust"));
        assert!(dot.contains("TypeScript"));
    }

    #[test]
    fn test_empty_catalog_markdown() {
        let cat = FleetCatalog::from_repos(vec![], &HashMap::new());
        let md = catalog_markdown(&cat);
        assert!(md.contains("Total repos:** 0"));
    }

    #[test]
    fn test_empty_catalog_json() {
        let cat = FleetCatalog::from_repos(vec![], &HashMap::new());
        let json = catalog_json(&cat);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["stats"]["total_repos"], 0);
    }

    #[test]
    fn test_empty_catalog_dot() {
        let cat = FleetCatalog::from_repos(vec![], &HashMap::new());
        let dot = dependency_dot(&cat);
        assert!(dot.contains("digraph fleet"));
    }
}
