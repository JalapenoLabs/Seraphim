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

use base64::Engine;
use eyre::{eyre, Result};

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
pub async fn provision_config_repo(state: &AppState) -> Result<()> {
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

    match run(state, &script).await {
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

/// Full provision: config repo (hard fail) + env setup + every enabled repo
/// (clone/update, per-repo CLAUDE.md, per-repo setup).
pub async fn provision_workspace(state: &AppState) -> Result<()> {
    // The config repo is the agent's brain (AGENTS.md, skills, docs); set it up
    // first and stop if it fails.
    provision_config_repo(state).await?;

    let settings = queries::get_settings(&state.db).await?;
    let repos = queries::list_repositories(&state.db).await?;

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

    run(state, &script).await
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

/// Per-task prep: ensure config + AGENTS.md + the focus repo, then cut `branch`.
pub async fn prepare_branch(
    state: &AppState,
    settings: &Settings,
    repo: &Repository,
    branch: &str,
) -> Result<()> {
    let dir = format!("/workspace/{}", repo_dir_name(&repo.full_name));

    let mut script = String::from("set -e\n");
    script.push_str(&prelude_agents(settings));
    // Ensure the focus repo exists (clone + setup on first sight), then branch.
    script.push_str(&repo_block(repo, false));
    script.push_str(&format!("cd \"{dir}\"\n", dir = dir));
    script.push_str(reset_tree_snippet());
    script.push_str(&format!(
        "git checkout \"{default}\"\n\
         git pull --ff-only origin \"{default}\" || true\n\
         git checkout -B \"{branch}\" \"origin/{default}\"\n\
         git submodule update --init --recursive || true\n",
        default = repo.default_branch,
        branch = branch,
    ));

    run(state, &script).await
}

/// Per-task prep for a CI fix: ensure the repo and AGENTS.md, then check out the
/// PR's existing branch at its pushed tip (so the agent's earlier commits are
/// present). Unlike [`prepare_branch`], this never re-cuts the branch from the
/// default, which would discard the work the PR is built on.
pub async fn prepare_existing_branch(
    state: &AppState,
    settings: &Settings,
    repo: &Repository,
    branch: &str,
) -> Result<()> {
    let dir = format!("/workspace/{}", repo_dir_name(&repo.full_name));

    let mut script = String::from("set -e\n");
    script.push_str(&prelude_agents(settings));
    // Ensure the focus repo exists (clone on first sight), then sync to the
    // remote branch tip CI actually tested.
    script.push_str(&repo_block(repo, false));
    script.push_str(&format!("cd \"{dir}\"\n", dir = dir));
    script.push_str(reset_tree_snippet());
    script.push_str(&format!(
        "git fetch origin\n\
         git checkout -B \"{branch}\" \"origin/{branch}\"\n\
         git reset --hard \"origin/{branch}\"\n\
         git submodule update --init --recursive || true\n",
        branch = branch,
    ));

    run(state, &script).await
}

/// Bash to clone-or-update a single repo, write its CLAUDE.md, and (on a fresh
/// clone, or always during a full provision) run its setup script.
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

    // Run setup after a fresh clone; during a full provision, run it every time.
    let clone_setup = setup_block.clone();
    let update_setup = if always_setup {
        setup_block
    } else {
        String::new()
    };

    // Submodules are common across the user's orgs, so fetch them on update and
    // pull them down on a fresh clone.
    format!(
        "if [ -d \"{dir}/.git\" ]; then\n\
           git -C \"{dir}\" fetch origin --recurse-submodules || true\n\
           {update_setup}\
         else\n\
           git clone --recurse-submodules \"{clone_url}\" \"{dir}\"\n\
           {clone_setup}\
         fi\n\
         {claude_md}",
        dir = dir,
        clone_url = repo.clone_url,
        claude_md = write_file_snippet(&format!("{dir}/CLAUDE.md"), &repo.instructions),
    )
}

/// Runs a prep script in the workspace, surfacing a non-zero exit as an error.
async fn run(state: &AppState, script: &str) -> Result<()> {
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
        .exec_capture(
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
