# si-scanner

> Fleet-wide discovery and cataloging for [SuperInstance](https://github.com/SuperInstance) — scan a GitHub organization, extract capabilities from `CAPABILITY.toml` files, build a dependency graph, and generate reports.

---

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [Module Reference](#module-reference)
  - [github — GitHub API Scanner](#github--github-api-scanner)
  - [extractor — Capability Extractor](#extractor--capability-extractor)
  - [graph — Dependency Graph](#graph--dependency-graph)
  - [catalog — Fleet Catalog](#catalog--fleet-catalog)
  - [report — Output Formatting](#report--output-formatting)
- [CAPABILITY.toml Format](#capabilitytoml-format)
  - [Multi-Capability Format](#multi-capability-format)
  - [Legacy Single-Capability Format](#legacy-single-capability-format)
- [Usage Examples](#usage-examples)
  - [Scanning an Organization](#scanning-an-organization)
  - [Searching the Catalog](#searching-the-catalog)
  - [Filtering by Language](#filtering-by-language)
  - [Finding Capability Providers](#finding-capability-providers)
  - [Generating Reports](#generating-reports)
  - [Dependency Analysis](#dependency-analysis)
- [Testing](#testing)
- [Configuration](#configuration)
- [GitHub API Rate Limits](#github-api-rate-limits)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

`si-scanner` is a Rust library for fleet-wide discovery and cataloging of repositories in the SuperInstance GitHub organization. It provides:

1. **GitHub API scanning** — Enumerate all public repos in an org or user account, filtering out forks and archived repos.
2. **Capability extraction** — Parse `CAPABILITY.toml` files from each repo to understand what each project provides and depends on.
3. **Dependency graph construction** — Build a directed graph of inter-repo dependencies based on declared capabilities.
4. **Fleet catalog** — Aggregate everything into a searchable, queryable catalog.
5. **Report generation** — Output the catalog as Markdown, JSON, or Graphviz DOT for visualization.

### Why?

When you manage a fleet of repositories, you need to know:

- Which repos exist and what they do
- What capabilities each repo provides to the fleet
- What each repo depends on
- The correct build/deploy order (topological sort)
- Where the critical path lies

`si-scanner` answers all of these by treating `CAPABILITY.toml` as the contract between repos.

---

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
si-scanner = "0.1"
```

Or use `cargo add`:

```bash
cargo add si-scanner
```

### Requirements

- Rust 2021 edition or later
- A GitHub personal access token (for API scanning)

---

## Quick Start

```rust
use si_scanner::{FleetCatalog, GitHubScanner, catalog_markdown, catalog_json, dependency_dot};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create a scanner
    let scanner = GitHubScanner::new("ghp_your_token_here");

    // 2. Provide CAPABILITY.toml contents (or fetch them via scanner)
    let mut contents = HashMap::new();
    contents.insert(
        "si-core".to_owned(),
        r#"
[[capability]]
name = "si-core"
provides = ["config", "logging"]
depends = []
"#
        .to_owned(),
    );

    // 3. Scan and build the catalog
    let catalog = FleetCatalog::from_scan(&scanner, "SuperInstance", &contents)?;

    // 4. Query
    println!("Total repos: {}", catalog.repos.len());
    for repo in catalog.by_language("Rust") {
        println!("  Rust repo: {}", repo.name);
    }

    // 5. Generate reports
    println!("{}", catalog_markdown(&catalog));
    println!("{}", catalog_json(&catalog));
    println!("{}", dependency_dot(&catalog));

    Ok(())
}
```

---

## Architecture

```
┌─────────────┐
│  GitHub API  │
└──────┬──────┘
       │ HTTP (ureq)
       ▼
┌─────────────┐     ┌──────────────────┐
│ github.rs   │────▶│  Vec<RepoInfo>   │
│  Scanner    │     └────────┬─────────┘
└─────────────┘              │
                             ▼
                    ┌──────────────────┐
                    │ extractor.rs     │
                    │  Capability      │
                    │  Extraction      │
                    └────────┬─────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼                             ▼
     ┌─────────────────┐          ┌──────────────────┐
     │   graph.rs      │          │   catalog.rs     │
     │ DependencyGraph │          │  FleetCatalog    │
     └─────────────────┘          └────────┬─────────┘
                                           │
                                           ▼
                                  ┌──────────────────┐
                                  │   report.rs      │
                                  │  Markdown / JSON │
                                  │  / Graphviz DOT  │
                                  └──────────────────┘
```

### Data Flow

1. **Scan** → `GitHubScanner` queries GitHub REST API → produces `Vec<RepoInfo>`
2. **Extract** → `extract_all()` parses `CAPABILITY.toml` content → `HashMap<String, Vec<Capability>>`
3. **Graph** → `DependencyGraph` is built from repo/capability relationships
4. **Catalog** → `FleetCatalog` aggregates repos, capabilities, and graph
5. **Report** → `report` module formats output

---

## Module Reference

### github — GitHub API Scanner

The `github` module provides `GitHubScanner` for enumerating repositories via the GitHub REST API.

```rust
use si_scanner::github::{GitHubScanner, RepoInfo};

let scanner = GitHubScanner::new("ghp_your_token");

// Scan an organization
let repos: Vec<RepoInfo> = scanner.scan_org("SuperInstance")?;

// Scan a user
let repos: Vec<RepoInfo> = scanner.scan_user("octocat")?;
```

#### `RepoInfo` Fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Repository name |
| `description` | `Option<String>` | Repo description |
| `language` | `Option<String>` | Primary language |
| `topics` | `Vec<String>` | GitHub topics/tags |
| `updated_at` | `DateTime<Utc>` | Last push timestamp |
| `has_readme` | `bool` | Whether README.md exists |
| `has_capability_toml` | `bool` | Whether CAPABILITY.toml exists |
| `default_branch` | `Option<String>` | Default branch name |
| `archived` | `bool` | Whether the repo is archived |
| `fork` | `bool` | Whether the repo is a fork |
| `stars` | `u32` | Star count |
| `html_url` | `Option<String>` | GitHub URL |

#### Filtering

`scan_org` and `scan_user` automatically exclude:
- **Forks** — not original repos
- **Archived repos** — no longer maintained

---

### extractor — Capability Extractor

Parses `CAPABILITY.toml` content to extract capability declarations.

```rust
use si_scanner::extractor::{extract_capabilities_from_content, extract_all, Capability};

let content = r#"
[[capability]]
name = "si-auth"
provides = ["auth", "oauth2"]
depends = ["si-config"]
"#;

let caps: Vec<Capability> = extract_capabilities_from_content(content);
assert_eq!(caps[0].name, "si-auth");
```

#### `Capability` Fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Capability identifier |
| `description` | `String` | Human-readable description |
| `version` | `String` | Semantic version |
| `provides` | `Vec<String>` | Capabilities provided to others |
| `depends` | `Vec<String>` | Capabilities required |
| `tags` | `Vec<String>` | Keywords / categories |

#### Functions

- `extract_capabilities_from_content(content: &str) -> Vec<Capability>` — Parse raw TOML string
- `extract_capabilities(repo: &RepoInfo, content: Option<&str>) -> Vec<Capability>` — Convenience wrapper
- `extract_all(repos, contents) -> HashMap<String, Vec<Capability>>` — Batch extraction

---

### graph — Dependency Graph

Builds a directed graph of inter-repo dependencies based on capability declarations.

```rust
use si_scanner::graph::DependencyGraph;

let mut graph = DependencyGraph::new();

// Add repos with their capabilities
graph.add_repo(&repo1, &caps1);
graph.add_repo(&repo2, &caps2);

// Query
let providers = graph.find_providers("auth");    // repos that provide "auth"
let dependents = graph.find_dependents("auth");  // repos that need "auth"
let order = graph.topological_sort();             // build order
```

#### Key Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `new()` | `Self` | Create empty graph |
| `add_repo(repo, caps)` | `()` | Register a repo and its capabilities |
| `find_dependents(cap)` | `Vec<String>` | Repos that depend on this capability |
| `find_providers(cap)` | `Vec<String>` | Repos that provide this capability |
| `topological_sort()` | `Vec<String>` | Build order (providers before dependents) |
| `repo_provides(repo)` | `Vec<String>` | Capabilities a repo provides |
| `repo_depends(repo)` | `Vec<String>` | Capabilities a repo depends on |
| `repo_count()` | `usize` | Number of repos in the graph |
| `capability_count()` | `usize` | Number of unique capabilities |

#### Topological Sort

Uses **Kahn's algorithm** to produce a deterministic ordering where providers come before dependents. In case of cycles, cycle members are appended at the end in alphabetical order (best-effort).

---

### catalog — Fleet Catalog

The main aggregation point that combines repos, capabilities, and the dependency graph.

```rust
use si_scanner::catalog::FleetCatalog;

// From a live scan
let catalog = FleetCatalog::from_scan(&scanner, "SuperInstance", &contents)?;

// From pre-built data
let catalog = FleetCatalog::from_repos(repos, &contents);

// Query
catalog.search("auth");          // search by name/description
catalog.by_language("Rust");     // filter by language
catalog.by_capability("cache");  // filter by capability
catalog.stats();                 // summary statistics
```

#### `CatalogStats` Fields

| Field | Type | Description |
|-------|------|-------------|
| `total_repos` | `usize` | Total number of repos |
| `total_capabilities` | `usize` | Unique capability count |
| `languages` | `HashMap<String, usize>` | Language → repo count |
| `repos_with_capability_toml` | `usize` | Repos declaring capabilities |
| `repos_without_capability_toml` | `usize` | Repos without declarations |
| `total_stars` | `u32` | Sum of all star counts |
| `top_starred` | `Vec<(String, u32)>` | Top 10 by stars |

---

### report — Output Formatting

Generate human-readable and machine-parseable reports from the catalog.

```rust
use si_scanner::report::{catalog_markdown, catalog_json, dependency_dot};

let md = catalog_markdown(&catalog);    // Markdown report
let json = catalog_json(&catalog);      // JSON report
let dot = dependency_dot(&catalog);     // Graphviz DOT
```

#### Markdown Output

Includes:
- Overview statistics
- Language breakdown table
- Top starred repos
- Per-repo details (description, language, provides, depends)
- ASCII dependency graph

#### JSON Output

Structured JSON with:
- `stats` — Summary statistics
- `topological_order` — Build order
- `repos` — Per-repo details
- `capabilities` — All declared capabilities

#### Graphviz DOT

Color-coded nodes by language:
- Rust → orange
- Go → blue
- TypeScript → blue
- Python → dark blue
- JavaScript → yellow

Render with: `dot -Tpng fleet.dot -o fleet.png`

---

## CAPABILITY.toml Format

### Multi-Capability Format

A repo can declare multiple capabilities:

```toml
[[capability]]
name = "si-auth"
description = "Authentication and authorization service"
version = "2.1.0"
provides = ["auth", "oauth2", "sessions"]
depends = ["database", "config"]
tags = ["security", "core"]

[[capability]]
name = "si-auth-admin"
description = "Admin panel for user management"
version = "1.0.0"
provides = ["user-management"]
depends = ["auth"]
tags = ["admin", "ui"]
```

### Legacy Single-Capability Format

For simpler repos, a flat format is supported:

```toml
name = "si-core"
description = "Core configuration and utilities"
version = "3.0.0"
provides = ["config", "logging", "error-handling"]
depends = []
```

---

## Usage Examples

### Scanning an Organization

```rust
use si_scanner::{GitHubScanner, FleetCatalog};
use std::collections::HashMap;

let scanner = GitHubScanner::new("ghp_your_token_here");

// Scan with pre-fetched capability contents
let mut contents = HashMap::new();
// ... populate contents ...

let catalog = FleetCatalog::from_scan(&scanner, "SuperInstance", &contents)?;

// Or target a user
let catalog = FleetCatalog::from_scan(&scanner, "user:octocat", &contents)?;
```

### Searching the Catalog

```rust
// Case-insensitive search by name or description
let results = catalog.search("auth");
for repo in results {
    println!("Found: {} - {:?}", repo.name, repo.description);
}
```

### Filtering by Language

```rust
let rust_repos = catalog.by_language("Rust");
let go_repos = catalog.by_language("Go");

println!("Rust repos: {}", rust_repos.len());
println!("Go repos: {}", go_repos.len());
```

### Finding Capability Providers

```rust
// Which repos provide the "auth" capability?
let auth_providers = catalog.by_capability("auth");

// Which repos depend on "auth"?
let auth_consumers = catalog.graph.find_dependents("auth");
```

### Generating Reports

```rust
use si_scanner::report::{catalog_markdown, catalog_json, dependency_dot};

// Markdown report
let md = catalog_markdown(&catalog);
std::fs::write("FLEET.md", md)?;

// JSON report
let json = catalog_json(&catalog);
std::fs::write("fleet.json", json)?;

// Dependency graph visualization
let dot = dependency_dot(&catalog);
std::fs::write("fleet.dot", dot)?;
// Then: dot -Tpng fleet.dot -o fleet.png
```

### Dependency Analysis

```rust
// Get the correct build order
let build_order = catalog.graph.topological_sort();
println!("Build order:");
for (i, repo) in build_order.iter().enumerate() {
    println!("  {}. {}", i + 1, repo);
}

// Check what a specific repo depends on
let deps = catalog.graph.repo_depends("si-gateway");
println!("si-gateway depends on: {:?}", deps);

// Find all repos that would break if "si-core" goes down
let dependents = catalog.graph.find_dependents("config");
println!("Dependent on config: {:?}", dependents);
```

### Building a Catalog Without API Access

```rust
use si_scanner::{FleetCatalog, RepoInfo};
use std::collections::HashMap;

let repos = vec![
    // ... build RepoInfo manually or from cache ...
];

let mut contents = HashMap::new();
contents.insert("my-repo".into(), r#"
[[capability]]
name = "my-cap"
provides = ["feature-a"]
depends = []
"#.into());

let catalog = FleetCatalog::from_repos(repos, &contents);
```

---

## Testing

The crate includes **44 tests** covering all modules with mock data (no real API calls):

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific module tests
cargo test github::
cargo test extractor::
cargo test graph::
cargo test catalog::
cargo test report::
```

### Test Coverage

| Module | Tests | Coverage |
|--------|-------|----------|
| github | 8 | Deserialization, base64, scanner creation, edge cases |
| extractor | 9 | Multi/legacy/empty/invalid TOML, batch extraction, serde |
| graph | 9 | Empty/single/chain/cycle/independent/multi-provider |
| catalog | 9 | Search/filter/stats/by-lang/by-cap/empty/truncation |
| report | 9 | Markdown/JSON/DOT format, content verification, empty |

All tests use mock data and require no network access.

---

## Configuration

### GitHub Token

A personal access token is required for API access. Create one at:

https://github.com/settings/tokens

Recommended permissions:
- `public_repo` — Read access to public repositories

### Rate Limits

The GitHub REST API allows:
- **Authenticated**: 5,000 requests/hour
- **Unauthenticated**: 60 requests/hour

`si-scanner` uses authenticated requests. For large orgs (>100 repos), pagination is handled automatically with `per_page=100`.

---

## GitHub API Rate Limits

| Scenario | API Calls | Notes |
|----------|-----------|-------|
| 50 repos | ~100 | 1 list + 1 contents per repo |
| 100 repos | ~200 | May need pagination |
| 500 repos | ~1000 | Watch rate limits |

If you hit rate limits, wait for the reset window or cache results locally.

---

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Add tests for your changes
4. Ensure all tests pass (`cargo test`)
5. Commit with a descriptive message
6. Push to your fork
7. Open a Pull Request

### Development Setup

```bash
git clone https://github.com/SuperInstance/si-scanner.git
cd si-scanner
cargo build
cargo test
```

### Code Style

- Follow standard Rust conventions (`cargo fmt`)
- No `unwrap()` in library code — use `Result`
- All public APIs must have doc comments
- Tests must not require network access

---

## License

Licensed under the [MIT License](LICENSE).

---

## Related Projects

- [SuperInstance](https://github.com/SuperInstance) — The organization this tool catalogs
- `CAPABILITY.toml` — The capability declaration format used across the fleet

---

Built with ❤️ for the SuperInstance fleet.
