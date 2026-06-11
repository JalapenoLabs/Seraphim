//! Network access policy: translate the operator's chosen [`NetworkAccessLevel`]
//! into Claude Code `~/.claude/settings.json` permission rules.
//!
//! Claude Code's permission system is the agent's built-in network control:
//! `deny` rules are a hard boundary enforced in every permission mode (including
//! `bypassPermissions`, which the orchestrator uses), while `allow` rules name
//! the destinations the agent may reach without prompting. We express the policy
//! the same way Claude Code on the web's access levels do:
//!
//! - **None**    -> deny the network-capable tools outright.
//! - **Trusted** -> allow the built-in registry/VCS/cloud allow-list.
//! - **Full**    -> allow the network tools unrestricted (no deny).
//! - **Custom**  -> allow the operator's domains, optionally plus the defaults.
//!
//! The result is merged into any operator-provided `settings.json` during
//! provisioning (see `provision::apply_network_policy`); this module is the pure,
//! unit-tested core that decides *what* the permission block should contain.

use serde_json::{json, Value};

use crate::db::models::{NetworkAccessLevel, Settings};

/// The built-in allow-list used by the **Trusted** level (and by **Custom** when
/// "include defaults" is on). Mirrors Claude Code on the web's default allowed
/// domains: package registries, version-control hosts, container registries,
/// cloud SDKs, and OS package mirrors. `*.` entries match any subdomain.
pub const DEFAULT_ALLOWED_DOMAINS: &[&str] = &[
    // Anthropic services
    "api.anthropic.com",
    "statsig.anthropic.com",
    "docs.claude.com",
    "platform.claude.com",
    "code.claude.com",
    "claude.ai",
    // Version control
    "github.com",
    "api.github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "codeload.github.com",
    "npm.pkg.github.com",
    "ghcr.io",
    "gist.github.com",
    "gitlab.com",
    "registry.gitlab.com",
    "bitbucket.org",
    "api.bitbucket.org",
    // Container registries
    "registry-1.docker.io",
    "auth.docker.io",
    "index.docker.io",
    "hub.docker.com",
    "production.cloudflare.docker.com",
    "download.docker.com",
    "gcr.io",
    "*.gcr.io",
    "mcr.microsoft.com",
    "*.data.mcr.microsoft.com",
    "public.ecr.aws",
    // Cloud platforms
    "*.googleapis.com",
    "storage.googleapis.com",
    "cloud.google.com",
    "*.amazonaws.com",
    "*.api.aws",
    "azure.com",
    "packages.microsoft.com",
    "dev.azure.com",
    "*.microsoftonline.com",
    // JavaScript / Node
    "registry.npmjs.org",
    "npmjs.com",
    "npmjs.org",
    "yarnpkg.com",
    "registry.yarnpkg.com",
    "nodejs.org",
    // Python
    "pypi.org",
    "files.pythonhosted.org",
    "pythonhosted.org",
    "test.pypi.org",
    // Ruby
    "rubygems.org",
    "api.rubygems.org",
    "index.rubygems.org",
    // Rust
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    "static.rust-lang.org",
    "rustup.rs",
    // Go
    "proxy.golang.org",
    "sum.golang.org",
    "pkg.go.dev",
    "goproxy.io",
    // JVM
    "repo.maven.org",
    "repo1.maven.org",
    "repo.maven.apache.org",
    "plugins.gradle.org",
    "services.gradle.org",
    "repo.spring.io",
    // Other package managers
    "repo.packagist.org",
    "packagist.org",
    "api.nuget.org",
    "nuget.org",
    "pub.dev",
    "hex.pm",
    "metacpan.org",
    "cdn.cocoapods.org",
    "hackage.haskell.org",
    "swift.org",
    // Linux distributions
    "archive.ubuntu.com",
    "security.ubuntu.com",
    "*.ubuntu.com",
    "ppa.launchpad.net",
    "launchpad.net",
    "*.nixos.org",
    // Development tools and platforms
    "pkgs.k8s.io",
    "dl.k8s.io",
    "releases.hashicorp.com",
    "apt.releases.hashicorp.com",
    "repo.anaconda.com",
    "conda.anaconda.org",
    "downloads.apache.org",
    "archive.apache.org",
    "binaries.prisma.sh",
    "developer.apple.com",
    "developer.android.com",
    // Cloud services and monitoring
    "*.sentry.io",
    "*.datadoghq.com",
    "api.statsig.com",
    // Content delivery and schema
    "*.sourceforge.net",
    "fonts.googleapis.com",
    "fonts.gstatic.com",
    "json-schema.org",
    "json.schemastore.org",
    // Model Context Protocol
    "*.modelcontextprotocol.io",
];

/// The network-capable tools we gate. `Bash` reaches the network through many
/// commands and cannot be domain-restricted by a permission pattern, so for the
/// **None** level we additionally deny the common fetchers as a best effort; the
/// authoritative blocks are the `WebFetch`/`WebSearch` denies.
const BASH_FETCHERS: &[&str] = &["Bash(curl:*)", "Bash(wget:*)"];

/// Builds the managed `permissions` block (an object with `allow` and `deny`
/// arrays) for the workspace's `settings.json`, given the current settings.
pub fn managed_permissions(settings: &Settings) -> Value {
    let (allow, deny) = match settings.network_access_level {
        NetworkAccessLevel::Full => (
            vec!["WebFetch".to_string(), "WebSearch".to_string()],
            vec![],
        ),
        NetworkAccessLevel::None => {
            let mut deny = vec!["WebFetch".to_string(), "WebSearch".to_string()];
            deny.extend(BASH_FETCHERS.iter().map(|rule| (*rule).to_string()));
            (vec![], deny)
        }
        NetworkAccessLevel::Trusted => (
            allow_for_domains(DEFAULT_ALLOWED_DOMAINS.iter().copied()),
            vec![],
        ),
        NetworkAccessLevel::Custom => (allow_for_domains(custom_domains(settings)), vec![]),
    };

    json!({ "allow": allow, "deny": deny })
}

/// The effective domain list for the **Custom** level: the operator's entries
/// (trimmed, de-duplicated, order preserved), with the built-in defaults
/// prepended when "include defaults" is on.
fn custom_domains(settings: &Settings) -> Vec<String> {
    let mut domains: Vec<String> = Vec::new();
    if settings.network_access_include_defaults {
        domains.extend(
            DEFAULT_ALLOWED_DOMAINS
                .iter()
                .map(|domain| (*domain).to_string()),
        );
    }
    domains.extend(
        settings
            .network_access_domains
            .0
            .iter()
            .map(|domain| domain.trim().to_string())
            .filter(|domain| !domain.is_empty()),
    );
    dedup_preserving_order(domains)
}

/// `["WebSearch", "WebFetch(domain:a)", "WebFetch(domain:b)", ...]`. `WebSearch`
/// is Anthropic-mediated (it never reaches the listed hosts directly), so the
/// restricted levels keep it available alongside the per-domain `WebFetch` rules.
fn allow_for_domains(domains: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    let mut allow = vec!["WebSearch".to_string()];
    for domain in domains {
        let domain = domain.as_ref().trim();
        if !domain.is_empty() {
            allow.push(format!("WebFetch(domain:{domain})"));
        }
    }
    dedup_preserving_order(allow)
}

fn dedup_preserving_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{NetworkAccessLevel, Settings};
    use chrono::Utc;
    use sqlx::types::Json;

    fn settings(level: NetworkAccessLevel, domains: &[&str], include_defaults: bool) -> Settings {
        Settings {
            org_name: String::new(),
            global_instructions: String::new(),
            default_review_policy: crate::db::models::ReviewPolicy::None,
            agent_paused: false,
            claude_model: String::new(),
            workspace_image_tag: String::new(),
            base_setup_script: String::new(),
            config_repo_url: String::new(),
            default_branch_template: String::new(),
            config_repo_error: None,
            current_session_id: None,
            updated_at: Utc::now(),
            claude_token_set: false,
            github_token_set: false,
            availability_enabled: false,
            availability_timezone: "UTC".to_string(),
            availability_windows: Json(Vec::new()),
            availability_skip_dates: Json(Vec::new()),
            network_access_level: level,
            network_access_domains: Json(domains.iter().map(|d| (*d).to_string()).collect()),
            network_access_include_defaults: include_defaults,
            usage_limit_pause_enabled: true,
            usage_limit_threshold: 80,
            usage_paused_until: None,
            post_thoughts_enabled: false,
            jira_enabled: false,
            jira_deployment: crate::db::models::JiraDeployment::Cloud,
            jira_base_url: String::new(),
            jira_email: String::new(),
            jira_token_set: false,
            jira_token_preview: None,
            claude_token_preview: None,
            github_token_preview: None,
            cooldown_until: None,
        }
    }

    fn allow(value: &serde_json::Value) -> Vec<String> {
        value["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn deny(value: &serde_json::Value) -> Vec<String> {
        value["deny"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn full_allows_the_network_tools_and_denies_nothing() {
        let value = managed_permissions(&settings(NetworkAccessLevel::Full, &[], true));
        assert_eq!(allow(&value), vec!["WebFetch", "WebSearch"]);
        assert!(deny(&value).is_empty());
    }

    #[test]
    fn none_denies_the_network_tools_and_common_fetchers() {
        let value = managed_permissions(&settings(NetworkAccessLevel::None, &[], true));
        assert!(allow(&value).is_empty());
        let deny = deny(&value);
        assert!(deny.contains(&"WebFetch".to_string()));
        assert!(deny.contains(&"WebSearch".to_string()));
        assert!(deny.contains(&"Bash(curl:*)".to_string()));
        // None must never grant a bare WebFetch allow.
        assert!(!deny.is_empty());
    }

    #[test]
    fn trusted_allows_default_domains_but_not_bare_webfetch() {
        let value = managed_permissions(&settings(NetworkAccessLevel::Trusted, &[], true));
        let allow = allow(&value);
        assert!(deny(&value).is_empty());
        assert!(!allow.contains(&"WebFetch".to_string()));
        assert!(allow.contains(&"WebFetch(domain:registry.npmjs.org)".to_string()));
        assert!(allow.contains(&"WebFetch(domain:github.com)".to_string()));
        assert!(allow.contains(&"WebSearch".to_string()));
    }

    #[test]
    fn custom_with_defaults_combines_lists_without_duplicates() {
        let value = managed_permissions(&settings(
            NetworkAccessLevel::Custom,
            &["api.example.com", "github.com", "  ", "api.example.com"],
            true,
        ));
        let allow = allow(&value);
        assert!(allow.contains(&"WebFetch(domain:api.example.com)".to_string()));
        // github.com is in the defaults too, so it must appear exactly once.
        let count = allow
            .iter()
            .filter(|r| *r == "WebFetch(domain:github.com)")
            .count();
        assert_eq!(count, 1);
        // Blank entries are dropped.
        assert!(!allow.iter().any(|r| r.contains("domain:)")));
    }

    #[test]
    fn custom_without_defaults_allows_only_listed_domains() {
        let value = managed_permissions(&settings(
            NetworkAccessLevel::Custom,
            &["api.example.com", "*.internal.example.com"],
            false,
        ));
        let allow = allow(&value);
        assert_eq!(
            allow,
            vec![
                "WebSearch",
                "WebFetch(domain:api.example.com)",
                "WebFetch(domain:*.internal.example.com)",
            ]
        );
        assert!(!allow.contains(&"WebFetch(domain:registry.npmjs.org)".to_string()));
    }
}
