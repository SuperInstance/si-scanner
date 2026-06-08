//! GitHub API scanner — discovers repos in an org or user account.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashSet;

/// Summary of a GitHub repository relevant to fleet cataloging.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub has_readme: bool,
    #[serde(default)]
    pub has_capability_toml: bool,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub fork: bool,
    #[serde(default, rename = "stargazers_count")]
    pub stars: u32,
    #[serde(default)]
    pub html_url: Option<String>,
}

/// Internal representation of the JSON returned by the GitHub repos API.
#[derive(Debug, Deserialize)]
struct GhRepo {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    updated_at: DateTime<Utc>,
    default_branch: Option<String>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    stargazers_count: u32,
    #[serde(default)]
    html_url: Option<String>,
}

/// Internal representation for repo contents check.
#[derive(Debug, Deserialize)]
struct GhContentsEntry {
    name: String,
}

/// Scanner that queries the GitHub REST API to enumerate repositories.
pub struct GitHubScanner {
    token: String,
    agent: String,
}

impl GitHubScanner {
    /// Create a new scanner authenticated with a GitHub personal-access token.
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            agent: "si-scanner/0.1.0".to_owned(),
        }
    }

    /// Scan all (non-fork, non-archived) repos belonging to an organization.
    pub fn scan_org(&self, org: &str) -> Result<Vec<RepoInfo>, String> {
        self.scan_target("orgs", org)
    }

    /// Scan all (non-fork, non-archived) repos belonging to a user.
    pub fn scan_user(&self, user: &str) -> Result<Vec<RepoInfo>, String> {
        self.scan_target("users", user)
    }

    fn scan_target(&self, scope: &str, target: &str) -> Result<Vec<RepoInfo>, String> {
        let url = format!(
            "https://api.github.com/{}/{}/repos?per_page=100&type=public",
            scope, target
        );

        let body = self.get_json(&url)?;
        let gh_repos: Vec<GhRepo> =
            serde_json::from_str(&body).map_err(|e| format!("parse repos: {e}"))?;

        let mut repos = Vec::new();
        for gh in gh_repos {
            if gh.fork || gh.archived {
                continue;
            }

            let branch = gh.default_branch.clone().unwrap_or_else(|| "main".to_owned());
            let (has_readme, has_capability_toml) = self.check_special_files(
                &gh.name,
                &branch,
            );

            repos.push(RepoInfo {
                name: gh.name,
                description: gh.description,
                language: gh.language,
                topics: gh.topics,
                updated_at: gh.updated_at,
                has_readme,
                has_capability_toml,
                default_branch: Some(branch),
                archived: gh.archived,
                fork: gh.fork,
                stars: gh.stargazers_count,
                html_url: gh.html_url,
            });
        }
        Ok(repos)
    }

    /// Check whether `README.md` and `CAPABILITY.toml` exist in the repo root.
    fn check_special_files(&self, repo: &str, branch: &str) -> (bool, bool) {
        let url = format!(
            "https://api.github.com/repos/SuperInstance/{}/contents?ref={}",
            repo, branch
        );

        let Ok(body) = self.get_json(&url) else {
            return (false, false);
        };

        let Ok(entries) = serde_json::from_str::<Vec<GhContentsEntry>>(&body) else {
            return (false, false);
        };

        let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        (
            names.contains("README.md"),
            names.contains("CAPABILITY.toml"),
        )
    }

    /// Fetch the raw content of a file from a repo.
    pub fn fetch_file_content(&self, repo: &str, path: &str, branch: &str) -> Result<String, String> {
        let url = format!(
            "https://api.github.com/repos/SuperInstance/{}/contents/{}?ref={}",
            repo, path, branch
        );

        let body = self.get_json(&url)?;
        #[derive(Deserialize)]
        struct FileResp {
            content: Option<String>,
            encoding: Option<String>,
        }
        let resp: FileResp =
            serde_json::from_str(&body).map_err(|e| format!("parse file resp: {e}"))?;

        match (resp.content, resp.encoding.as_deref()) {
            (Some(b64), Some("base64")) => {
                // Strip whitespace newlines GitHub inserts.
                let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
                let bytes = crate::github::base64_decode(&clean)
                    .map_err(|e| format!("base64 decode: {e}"))?;
                String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
            }
            _ => Err("no base64 content returned".to_owned()),
        }
    }

    fn get_json(&self, url: &str) -> Result<String, String> {
        let config = ureq::Agent::config_builder()
            .user_agent(&self.agent)
            .build();
        let agent = ureq::Agent::new_with_config(config);

        let resp = agent
            .get(url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("http {url}: {e}"))?;

        let mut body = String::new();
        use std::io::Read;
        resp.into_body()
            .into_reader()
            .read_to_string(&mut body)
            .map_err(|e| format!("read body: {e}"))?;
        Ok(body)
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const TABLE: &[Option<u8>; 128] = &{
        let mut table = [None; 128];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = Some(i);
            table[(b'a' + i) as usize] = Some(26 + i);
            i += 1;
        }
        let mut d = 0u8;
        while d < 10 {
            table[(b'0' + d) as usize] = Some(52 + d);
            d += 1;
        }
        table[b'+' as usize] = Some(62);
        table[b'/' as usize] = Some(63);
        table
    };

    let input = input.as_bytes();
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = [0u32; 4];
    let mut idx = 0;
    for &b in input {
        if b == b'=' {
            break;
        }
        let Some(&Some(val)) = TABLE.get(b as usize) else {
            return Err(format!("invalid base64 byte {b}"));
        };
        buf[idx] = val as u32;
        idx += 1;
        if idx == 4 {
            out.push(((buf[0] << 2) | (buf[1] >> 4)) as u8);
            out.push((((buf[1] & 0xF) << 4) | (buf[2] >> 2)) as u8);
            out.push((((buf[2] & 0x3) << 6) | buf[3]) as u8);
            idx = 0;
        }
    }
    match idx {
        2 => {
            out.push(((buf[0] << 2) | (buf[1] >> 4)) as u8);
        }
        3 => {
            out.push(((buf[0] << 2) | (buf[1] >> 4)) as u8);
            out.push((((buf[1] & 0xF) << 4) | (buf[2] >> 2)) as u8);
        }
        _ => {}
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_info_deserialize() {
        let json = r#"{
            "name": "test-repo",
            "description": "A test repo",
            "language": "Rust",
            "topics": ["cli", "scanner"],
            "updated_at": "2025-06-01T12:00:00Z",
            "has_readme": true,
            "has_capability_toml": false,
            "default_branch": "main",
            "archived": false,
            "fork": false,
            "stargazers_count": 42,
            "html_url": "https://github.com/SuperInstance/test-repo"
        }"#;
        let repo: RepoInfo = serde_json::from_str(json).unwrap();
        assert_eq!(repo.name, "test-repo");
        assert_eq!(repo.language.as_deref(), Some("Rust"));
        assert_eq!(repo.topics.len(), 2);
        assert_eq!(repo.stars, 42);
    }

    #[test]
    fn test_scanner_new() {
        let scanner = GitHubScanner::new("fake-token");
        assert_eq!(scanner.token, "fake-token");
        assert_eq!(scanner.agent, "si-scanner/0.1.0");
    }

    #[test]
    fn test_base64_decode_basic() {
        let decoded = base64_decode("SGVsbG8gV29ybGQ=").unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello World");
    }

    #[test]
    fn test_base64_decode_padding() {
        let decoded = base64_decode("AQID").unwrap();
        assert_eq!(decoded, vec![1, 2, 3]);
    }

    #[test]
    fn test_base64_decode_two_pad() {
        let decoded = base64_decode("AQ==").unwrap();
        assert_eq!(decoded, vec![1]);
    }

    #[test]
    fn test_base64_decode_empty() {
        let decoded = base64_decode("").unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_repo_info_minimal() {
        let json = r#"{
            "name": "minimal",
            "updated_at": "2025-01-01T00:00:00Z"
        }"#;
        let repo: RepoInfo = serde_json::from_str(json).unwrap();
        assert_eq!(repo.name, "minimal");
        assert!(repo.description.is_none());
        assert!(repo.language.is_none());
        assert!(repo.topics.is_empty());
        assert!(!repo.has_readme);
        assert!(!repo.has_capability_toml);
    }

    #[test]
    fn test_repo_info_archived_fork() {
        let json = r#"{
            "name": "old-fork",
            "updated_at": "2024-01-01T00:00:00Z",
            "archived": true,
            "fork": true
        }"#;
        let repo: RepoInfo = serde_json::from_str(json).unwrap();
        assert!(repo.archived);
        assert!(repo.fork);
    }
}
