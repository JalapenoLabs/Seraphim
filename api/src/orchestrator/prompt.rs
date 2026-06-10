//! Composes the prompts handed to Claude Code for a task.
//!
//! Both prompts share the same context header (org and global instructions, the
//! repo instructions, and the issue itself), then append a task-specific working
//! agreement: a fresh-work protocol that ends in a PR, or a CI-fix protocol that
//! re-engages on the PR's existing branch.

use crate::db::models::{Repository, Settings, Task};

use super::provision::repo_dir_name;

/// Builds the instruction text for working `task` fresh on a new `branch`.
pub fn build(settings: &Settings, repo: &Repository, task: &Task, branch: &str) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let mut prompt = context_header(settings, repo, task);

    prompt.push_str(&format!(
        "# Working agreement\n\
         - Your cwd is `/workspace`. Every configured repo is cloned here as a sibling \
         directory, so you can read and edit across repos as needed.\n\
         - The focus repo for this issue is `{repo}` at `{repo_path}`, already on a fresh \
         branch `{branch}` cut from `{default}`. `cd` there to do the primary work.\n\
         - Implement the change, then run the project's build/tests/linters and make them pass.\n\
         - Commit your work and push the branch.\n\
         - Open a pull request against `{default}` with `gh pr create`, referencing issue #{number}.\n\
         - When done, finish with a short summary of what you changed.\n",
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
        default = repo.default_branch,
        number = task.external_id,
    ));

    prompt
}

/// Builds the instruction text to re-engage `task` on its PR's failing CI.
///
/// `failing_checks` names the checks that failed (may be empty if they couldn't
/// be enumerated). The agent works the existing `branch` and is told to stay in
/// scope: if the failures aren't this issue's doing, comment and stop rather
/// than force unrelated changes.
pub fn build_ci_fix(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    failing_checks: &[String],
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let checks = if failing_checks.is_empty() {
        "(the failing checks could not be enumerated; inspect them yourself)".to_string()
    } else {
        failing_checks.join(", ")
    };
    let mut prompt = context_header(settings, repo, task);

    prompt.push_str(&format!(
        "# Fixing CI\n\
         - You previously opened a pull request for this issue, but its CI is failing: {checks}.\n\
         - Your cwd is `/workspace`. The focus repo `{repo}` is at `{repo_path}`, already checked \
         out on branch `{branch}` with your earlier commits.\n\
         - Investigate the failures first: use `gh pr checks` and `gh run view --log-failed` (or \
         open the PR's checks) to read the actual errors before changing anything.\n\
         - Fix the failures on this branch, then run the project's build/tests/linters, commit, and \
         push. Do not open a new pull request; the existing one updates automatically.\n\
         - Stay in scope: if the failures are pre-existing on `{default}`, flaky, or otherwise \
         unrelated to this issue, do not force unrelated changes. Instead leave a brief comment on \
         the PR explaining why, and stop without committing.\n\
         - Aim to get the PR as green as you reasonably can, then finish with a short summary.\n",
        checks = checks,
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
        default = repo.default_branch,
    ));

    prompt
}

/// The shared prompt header: who the agent is, the org/global/repo instructions,
/// and the issue under work.
fn context_header(settings: &Settings, repo: &Repository, task: &Task) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!(
        "You are Seraphim, an autonomous developer working for {org}.\n\n",
        org = settings.org_name
    ));

    if !settings.global_instructions.trim().is_empty() {
        prompt.push_str("# Global instructions\n");
        prompt.push_str(settings.global_instructions.trim());
        prompt.push_str("\n\n");
    }

    if !repo.instructions.trim().is_empty() {
        prompt.push_str(&format!("# Instructions for {}\n", repo.full_name));
        prompt.push_str(repo.instructions.trim());
        prompt.push_str("\n\n");
    }

    prompt.push_str("# Your task\n");
    prompt.push_str(&format!(
        "Work issue #{number}: {title}\n\nIssue description:\n{body}\n\nIssue link: {url}\n\n",
        number = task.external_id,
        title = task.title,
        body = if task.body_snapshot.trim().is_empty() {
            "(no description provided)"
        } else {
            task.body_snapshot.trim()
        },
        url = task.url,
    ));

    prompt
}
