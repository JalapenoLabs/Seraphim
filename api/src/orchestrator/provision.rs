//! Provisioning the multi-repo workspace.
//!
//! Claude is always spawned at `/workspace`, with every enabled repo cloned flat
//! beside it (`/workspace/{repo}`) so cross-repo work is natural. Global agent
//! instructions become `/workspace/AGENTS.md`; per-repo instructions become
//! `/workspace/{repo}/CLAUDE.md`. The `~/.claude` config repo is cloned into
//! `CLAUDE_CONFIG_DIR` for a portable, host-mount-free setup.
//!
//! Two entry points: [`provision_workspace`] (heavy, sets up everything) and
//! [`prepare_branch`] (light, per-task: ensure the focus repo and cut a branch).
//!
//! Every prep exec is scoped to a railway's container (issue #203). `main` always
//! targets the compose-managed workspace, so its behavior is unchanged; a non-
//! `main` railway targets its own per-railway container and clones only the repos
//! assigned to that railway. The plumbing is the same in both cases; only the
//! container name and the repo set differ.

use base64::Engine;
use eyre::{eyre, Result};

use super::network;
use super::railway::RailwayHandle;
use crate::db::models::{Repository, Settings};
use crate::db::queries;
use crate::state::AppState;

/// The flat directory name a repo is cloned into: the part after the last `/`.
pub fn repo_dir_name(full_name: &str) -> &str {
    full_name.rsplit('/').next().unwrap_or(full_name)
}

/// Base64 so file contents cross the `docker exec` boundary unquoted.
fn encode(content: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(content)
}

/// A bash snippet that writes `content` to `path`, or removes the file when the
/// content is empty (so stale instructions don't linger).
fn write_file_snippet(path: &str, content: &str) -> String {
    if content.trim().is_empty() {
        format!("rm -f \"{path}\"\n")
    } else {
        format!("echo \"{}\" | base64 -d > \"{path}\"\n", encode(content))
    }
}

/// Clone-or-update the `~/.claude` config repo into `CLAUDE_CONFIG_DIR`. Uses
/// init+fetch+checkout so a non-empty dir (with a persisted `projects/`) is fine.
fn config_repo_snippet(config_repo_url: &str) -> String {
    if config_repo_url.trim().is_empty() {
        return String::new();
    }
    format!(
        "if [ -d \"$CLAUDE_CONFIG_DIR/.git\" ]; then\n\
           git -C \"$CLAUDE_CONFIG_DIR\" checkout -- . 2>/dev/null || true\n\
           git -C \"$CLAUDE_CONFIG_DIR\" pull --ff-only\n\
         else\n\
           mkdir -p \"$CLAUDE_CONFIG_DIR\"\n\
           git init -q \"$CLAUDE_CONFIG_DIR\"\n\
           git -C \"$CLAUDE_CONFIG_DIR\" remote add origin \"{url}\" 2>/dev/null \
             || git -C \"$CLAUDE_CONFIG_DIR\" remote set-url origin \"{url}\"\n\
           git -C \"$CLAUDE_CONFIG_DIR\" fetch --depth 1 origin\n\
           DEFAULT_REF=$(git -C \"$CLAUDE_CONFIG_DIR\" remote show origin | sed -n 's/.*HEAD branch: //p')\n\
           git -C \"$CLAUDE_CONFIG_DIR\" checkout -f \"${{DEFAULT_REF:-main}}\"\n\
         fi\n",
        url = config_repo_url
    )
}

/// Common script prelude: config dir default + global AGENTS.md. The config repo
/// is handled separately by [`provision_config_repo`] so its failures are
/// isolated and reported.
fn prelude_agents(settings: &Settings) -> String {
    format!(
        ": \"${{CLAUDE_CONFIG_DIR:=/workspace/.claude}}\"\n\
         mkdir -p \"$CLAUDE_CONFIG_DIR/projects\"\n\
         {agents}",
        agents = write_file_snippet("/workspace/AGENTS.md", &settings.global_instructions),
    )
}

/// Clones/refreshes the `~/.claude` config repo as its own hard-failing step and
/// records the outcome in `settings.config_repo_error`. A blank `config_repo_url`
/// is a no-op that clears any prior error (the agent runs unconfigured).
///
/// On failure the error is persisted (so the UI banners it and the agent halts)
/// and returned.
///
/// `handle` selects the railway's container; the config repo is cloned into each
/// railway's own container so every lane has the agent's brain (issue #203).
pub async fn provision_config_repo(state: &AppState, handle: &RailwayHandle) -> Result<()> {
    let settings = queries::get_settings(&state.db).await?;
    if settings.config_repo_url.trim().is_empty() {
        queries::set_config_repo_error(&state.db, None).await?;
        return Ok(());
    }

    let script = format!(
        "set -e\n\
         : \"${{CLAUDE_CONFIG_DIR:=/workspace/.claude}}\"\n\
         {config}\
         mkdir -p \"$CLAUDE_CONFIG_DIR/projects\"\n",
        config = config_repo_snippet(&settings.config_repo_url),
    );

    match run(state, handle.container(), &script).await {
        Ok(()) => {
            queries::set_config_repo_error(&state.db, None).await?;
            Ok(())
        }
        Err(error) => {
            // Recording the failure is the priority; surface it even if the write
            // itself somehow fails.
            let _ = queries::set_config_repo_error(&state.db, Some(&format!("{error}"))).await;
            Err(error)
        }
    }
}

/// A small Python program that merges the managed network `permissions` block
/// into the agent's `settings.json` without clobbering anything else. It reads
/// the target path and the managed `{allow, deny}` object from the environment,
/// drops any previously-managed network rules, keeps the operator's own rules,
/// and appends the current policy. Python (not jq) because it ships in the
/// workspace image and handles "file missing / not yet valid JSON" cleanly.
const NETWORK_MERGE_PY: &str = r#"
import json, os

path = os.environ["SERAPHIM_SETTINGS_PATH"]
managed = json.loads(os.environ["SERAPHIM_MANAGED_PERMS"])

try:
    with open(path) as fh:
        data = json.load(fh)
    if not isinstance(data, dict):
        data = {}
except (FileNotFoundError, json.JSONDecodeError):
    data = {}

perms = data.get("permissions")
if not isinstance(perms, dict):
    perms = {}
data["permissions"] = perms

def is_network_rule(rule):
    return (
        rule == "WebFetch"
        or rule.startswith("WebFetch(")
        or rule == "WebSearch"
        or rule in ("Bash(curl:*)", "Bash(wget:*)")
    )

for key in ("allow", "deny"):
    current = perms.get(key)
    current = [r for r in current if isinstance(r, str)] if isinstance(current, list) else []
    kept = [r for r in current if not is_network_rule(r)]
    added = [r for r in managed.get(key, []) if r not in kept]
    perms[key] = kept + added

with open(path, "w") as fh:
    json.dump(data, fh, indent=2)
    fh.write("\n")
"#;

/// Translates the operator's network access policy into the agent's
/// `~/.claude/settings.json` permissions and merges it in. Runs after the config
/// repo is (re)cloned so it patches whatever `settings.json` the operator ships,
/// rather than being overwritten by it.
pub async fn apply_network_policy(state: &AppState, handle: &RailwayHandle) -> Result<()> {
    let settings = queries::get_settings(&state.db).await?;
    let managed = encode(&serde_json::to_string(&network::managed_permissions(
        &settings,
    ))?);
    let script = encode(NETWORK_MERGE_PY);

    let bash = format!(
        ": \"${{CLAUDE_CONFIG_DIR:=/workspace/.claude}}\"\n\
         mkdir -p \"$CLAUDE_CONFIG_DIR\"\n\
         SERAPHIM_SETTINGS_PATH=\"$CLAUDE_CONFIG_DIR/settings.json\" \\\n\
         SERAPHIM_MANAGED_PERMS=\"$(echo {managed} | base64 -d)\" \\\n\
         python3 -c \"$(echo {script} | base64 -d)\"\n",
    );
    run(state, handle.container(), &bash).await
}

/// Full provision of a railway's container: config repo (hard fail) + network
/// policy + env setup + every enabled repo assigned to this railway (clone/update,
/// per-repo CLAUDE.md, per-repo setup).
///
/// `handle` selects which container to provision and which repos belong to it: a
/// repo belongs to exactly one railway, so a non-`main` railway clones only its
/// own repos, while `main` clones everything assigned to `main`. With only the
/// `main` railway present, that is every repo, identical to the prior behavior.
pub async fn provision_workspace(state: &AppState, handle: &RailwayHandle) -> Result<()> {
    // The config repo is the agent's brain (AGENTS.md, skills, docs); set it up
    // first and stop if it fails.
    provision_config_repo(state, handle).await?;
    // Then stamp the network policy onto its settings.json (or a fresh one).
    apply_network_policy(state, handle).await?;

    let settings = queries::get_settings(&state.db).await?;
    let repos = queries::list_repositories_for_railway(&state.db, handle.id).await?;

    let mut script = String::from("set -e\n");
    script.push_str(&prelude_agents(&settings));

    // Environment setup runs once here (installs CLIs/toolchains), not per task.
    if !settings.base_setup_script.trim().is_empty() {
        script.push_str("# --- environment setup ---\n");
        script.push_str(&settings.base_setup_script);
        script.push('\n');
    }

    for repo in repos.iter().filter(|repo| repo.enabled) {
        script.push_str(&repo_block(repo, true));
    }

    run(state, handle.container(), &script).await
}

/// Bash that returns a repo's working tree to a clean state before a checkout.
///
/// A turn interrupted mid-merge/rebase (e.g. the API restarting during a
/// conflict-resolving revisit) leaves an unresolved index that makes every later
/// `git checkout` fail with "you need to resolve your current index first". These
/// are safe no-ops when nothing is in progress.
fn reset_tree_snippet() -> &'static str {
    "git merge --abort 2>/dev/null || true\n\
     git rebase --abort 2>/dev/null || true\n\
     git reset --hard 2>/dev/null || true\n"
}

/// Per-task prep: ensure config + AGENTS.md + the focus repo, then cut `branch`,
/// all in the task's railway container (`handle`).
pub async fn prepare_branch(
    state: &AppState,
    handle: &RailwayHandle,
    settings: &Settings,
    repo: &Repository,
    branch: &str,
) -> Result<()> {
    let dir = format!("/workspace/{}", repo_dir_name(&repo.full_name));

    let mut script = String::from("set -e\n");
    script.push_str(&prelude_agents(settings));
    // Ensure the focus repo exists (clone + setup on first sight), then branch.
    // Repos opted into `setup_script_always_run` also re-run setup on the existing
    // clone, so a stacked-dependency merge that added deps gets them reinstalled
    // before this task's build/test (issue #275).
    script.push_str(&repo_block(repo, repo.setup_script_always_run));
    script.push_str(&format!("cd \"{dir}\"\n", dir = dir));
    script.push_str(reset_tree_snippet());
    script.push_str(&branch_prep_snippet(&repo.default_branch, branch));
    // Re-sync submodules to the freshly-checked-out branch's pinned commits, failing
    // loudly if a private submodule is inaccessible (issue #251).
    script.push_str(&submodule_update_snippet(&dir));

    run(state, handle.container(), &script).await
}

/// Bash that re-ups the target branch, then cuts a fresh work branch from it.
///
/// Always fetches `default` from origin so the work branch starts from the
/// latest target rather than whatever the clone last saw, then branches from
/// the freshly-fetched `origin/{default}`. The fetch runs under the caller's
/// `set -e`: an unreachable origin fails preparation loudly instead of silently
/// building on a stale base, which would otherwise leave this PR (and every one
/// cut after it) with avoidable merge conflicts once another PR lands on the
/// target in parallel (issue #187). Mirrors the hard fetch in
/// [`prepare_existing_branch`].
fn branch_prep_snippet(default: &str, branch: &str) -> String {
    format!(
        "git fetch origin \"{default}\"\n\
         git checkout -B \"{branch}\" \"origin/{default}\"\n"
    )
}

/// Per-task prep for a CI fix: ensure the repo and AGENTS.md, then check out the
/// PR's existing branch at its pushed tip (so the agent's earlier commits are
/// present). Unlike [`prepare_branch`], this never re-cuts the branch from the
/// default, which would discard the work the PR is built on.
pub async fn prepare_existing_branch(
    state: &AppState,
    handle: &RailwayHandle,
    settings: &Settings,
    repo: &Repository,
    branch: &str,
) -> Result<()> {
    let dir = format!("/workspace/{}", repo_dir_name(&repo.full_name));

    let mut script = String::from("set -e\n");
    script.push_str(&prelude_agents(settings));
    // Ensure the focus repo exists (clone on first sight), then sync to the
    // remote branch tip CI actually tested. Honor `setup_script_always_run` here
    // too so a fix turn rebuilds deps if the branch's deps changed (issue #275).
    script.push_str(&repo_block(repo, repo.setup_script_always_run));
    script.push_str(&format!("cd \"{dir}\"\n", dir = dir));
    script.push_str(reset_tree_snippet());
    script.push_str(&format!(
        "git fetch origin\n\
         git checkout -B \"{branch}\" \"origin/{branch}\"\n\
         git reset --hard \"origin/{branch}\"\n",
        branch = branch,
    ));
    // Re-sync submodules to this branch's pinned commits, failing loudly if a
    // private submodule is inaccessible (issue #251).
    script.push_str(&submodule_update_snippet(&dir));

    run(state, handle.container(), &script).await
}

/// Bash that initializes a repo's git submodules (issue #251), generic to any repo
/// that has them: a no-op when there is no `.gitmodules`, otherwise
/// `git submodule update --init --recursive`.
///
/// Crucially it does NOT fail silently (the same principle as the config-repo and
/// repo-sync error paths). The common failure is the workspace's mounted SSH /
/// deploy key lacking read access to a *private* submodule repo (e.g. Plunder's
/// `brand`); when that happens this names the offending submodule URL(s) and the
/// fix, then exits non-zero, so the operator gets an actionable message up front
/// instead of a generic build failure mid-task. Runs under the caller's `set -e`.
fn submodule_update_snippet(dir: &str) -> String {
    format!(
        "if [ -f \"{dir}/.gitmodules\" ]; then\n\
         if ! git -C \"{dir}\" submodule update --init --recursive; then\n\
         echo \"ERROR: could not initialize git submodules for {dir}.\" >&2\n\
         echo \"The workspace SSH/deploy key likely lacks read access to a private submodule repo:\" >&2\n\
         git config -f \"{dir}/.gitmodules\" --get-regexp 'submodule[.].*[.]url' 2>/dev/null | awk '{{ print \"  - \" $2 }}' >&2\n\
         echo \"Grant the mounted host SSH key (or a deploy key) read access to the repo(s) above, then re-provision.\" >&2\n\
         echo \"For Plunder's private brand submodule this is the BRAND_DEPLOY_KEY deploy key (companion ticket).\" >&2\n\
         exit 1\n\
         fi\n\
         fi\n"
    )
}

/// Bash to clone-or-update a single repo, write its CLAUDE.md, and run its setup
/// script. Setup always runs on a fresh clone; on an existing clone it runs only
/// when `always_setup` is set (a full provision, or a repo opted into
/// `setup_script_always_run` for per-task re-runs, issue #275).
fn repo_block(repo: &Repository, always_setup: bool) -> String {
    let dir = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let setup = repo.setup_script.trim();

    // Run the setup script in a subshell `cd`'d into the repo. `cd` is on its own
    // line so every newline-separated command runs there, sequentially, under the
    // outer `set -e` (no `&&` chaining required by the user).
    let setup_block = if setup.is_empty() {
        String::new()
    } else {
        format!("(\ncd \"{dir}\"\n{setup}\n)\n")
    };

    // Run setup after a fresh clone; on an existing clone, only when the caller
    // opted in (full provision, or a per-task re-run for an opted-in repo, #275).
    let clone_setup = setup_block.clone();
    let update_setup = if always_setup {
        setup_block
    } else {
        String::new()
    };

    // Clone (or fetch) the superproject, then initialize submodules explicitly via
    // `submodule_update_snippet` so a missing/private submodule fails loudly and
    // helpfully rather than as a silent partial clone (issue #251). Submodules are
    // brought up BEFORE the setup script, since a repo's build/setup (e.g. Plunder's)
    // may import from a submodule.
    let submodules = submodule_update_snippet(&dir);
    format!(
        "if [ -d \"{dir}/.git\" ]; then\n\
           git -C \"{dir}\" fetch origin || true\n\
           {submodules}\
           {update_setup}\
         else\n\
           git clone \"{clone_url}\" \"{dir}\"\n\
           {submodules}\
           {clone_setup}\
         fi\n\
         {claude_md}",
        dir = dir,
        clone_url = repo.clone_url,
        submodules = submodules,
        claude_md = write_file_snippet(&format!("{dir}/CLAUDE.md"), &repo.instructions),
    )
}

/// Runs a prep script in `container`, surfacing a non-zero exit as an error.
///
/// The container is the railway's: `main`'s is the compose-managed workspace, a
/// non-`main` railway's is its own per-railway container (issue #203).
async fn run(state: &AppState, container: &str, script: &str) -> Result<()> {
    let github_token = queries::get_github_token(&state.db).await?;
    // Wire git's credential helper for HTTPS remotes (GH_TOKEN is in this exec's
    // env); SSH remotes use the mounted key instead.
    let full_script = format!("gh auth setup-git >/dev/null 2>&1 || true\n{script}");
    // User-defined env vars are available to setup scripts (e.g. registry tokens).
    let mut env = vec![format!("GH_TOKEN={github_token}")];
    for variable in queries::list_environment_variables(&state.db).await? {
        env.push(format!("{}={}", variable.key, variable.value));
    }
    let output = state
        .workspace
        .exec_capture_in(
            container,
            "/workspace",
            vec!["bash".to_string(), "-lc".to_string(), full_script],
            env,
        )
        .await?;

    if !output.succeeded() {
        return Err(eyre!(
            "workspace prep exited {}: {}",
            output.exit_code,
            output.output
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{branch_prep_snippet, repo_block, submodule_update_snippet};
    use crate::db::models::Repository;
    use chrono::Utc;
    use uuid::Uuid;

    fn repo() -> Repository {
        Repository {
            id: Uuid::nil(),
            railway_id: Uuid::nil(),
            full_name: "JalapenoLabs/Plunder".to_string(),
            clone_url: "git@github.com:JalapenoLabs/Plunder.git".to_string(),
            default_branch: "main".to_string(),
            branch_template: None,
            setup_script: String::new(),
            instructions: String::new(),
            review_policy: None,
            enabled: true,
            sync_issues: false,
            issue_labels: Vec::new(),
            setup_script_always_run: false,
            sync_error: None,
            sync_error_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn branch_prep_fetches_the_target_then_cuts_from_its_fresh_tip() {
        let script = branch_prep_snippet("develop", "seraphim/issue-187-foo");

        // Re-up the target branch before any work begins (issue #187).
        assert!(script.contains("git fetch origin \"develop\""));
        // Cut the work branch from the freshly-fetched remote tip, not a local
        // branch that may be behind.
        assert!(script.contains("git checkout -B \"seraphim/issue-187-foo\" \"origin/develop\""));
    }

    #[test]
    fn branch_prep_does_not_swallow_a_failed_fetch() {
        // The old `git pull --ff-only ... || true` masked an unreachable origin
        // and let a stale base through. The fetch must run bare so prep fails
        // loudly instead.
        let script = branch_prep_snippet("main", "branch");
        assert!(!script.contains("pull"));
        assert!(!script.contains("git fetch origin \"main\" || true"));
        // Submodule handling moved to `submodule_update_snippet`, which no longer
        // swallows failures; the silenced form must be gone (issue #251).
        assert!(!script.contains("submodule update --init --recursive || true"));
    }

    #[test]
    fn submodule_snippet_inits_and_fails_loudly_with_an_actionable_message() {
        let script = submodule_update_snippet("/workspace/Plunder");

        // Only acts when the repo actually has submodules, so it is a safe no-op
        // for the common (no-submodule) repo.
        assert!(script.contains(".gitmodules"));
        // Initializes recursively.
        assert!(
            script.contains("git -C \"/workspace/Plunder\" submodule update --init --recursive")
        );
        // Does NOT fail silently (the whole point of issue #251).
        assert!(!script.contains("submodule update --init --recursive || true"));
        // On failure it names the access problem + the offending repo URLs + the fix,
        // then exits non-zero so it surfaces instead of a generic mid-task failure.
        assert!(script.contains("read access to a private submodule repo"));
        assert!(script.contains("--get-regexp"));
        assert!(script.contains("BRAND_DEPLOY_KEY"));
        assert!(script.contains("exit 1"));
    }

    #[test]
    fn repo_block_reruns_setup_on_existing_clone_only_when_opted_in() {
        // A repo with a setup script. The marker is distinctive so we can count how
        // many times the setup block is emitted across the clone/update branches.
        let mut repo = repo();
        repo.setup_script = "yarn install --frozen-lockfile".to_string();

        // Per-task default (existing clone): setup runs on a fresh clone only, so
        // the marker appears exactly once (issue #275: opted-out repos unchanged).
        let once = repo_block(&repo, false);
        assert_eq!(once.matches("yarn install --frozen-lockfile").count(), 1);

        // Opted in (or a full provision): setup also runs on the existing clone, so
        // the marker appears in both the clone and the update branch.
        let twice = repo_block(&repo, true);
        assert_eq!(twice.matches("yarn install --frozen-lockfile").count(), 2);
    }

    #[test]
    fn repo_block_initializes_submodules_explicitly_without_recurse_clone() {
        let script = repo_block(&repo(), true);

        // The clone no longer relies on `--recurse-submodules` (which would fail with
        // a generic message); submodules are initialized by the explicit, loud snippet.
        assert!(!script.contains("--recurse-submodules"));
        assert!(
            script.contains("git submodule update --init --recursive")
                || script.contains("submodule update --init --recursive")
        );
        assert!(script.contains("exit 1"));
    }
}
