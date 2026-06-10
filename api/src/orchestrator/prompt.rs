//! Composes the prompt handed to Claude Code for a single task.
//!
//! The prompt layers the org-wide instructions, the repo-specific instructions,
//! the issue itself, and a fixed completion protocol so the agent reliably opens
//! a PR Seraphim can then detect.

use crate::db::models::{Repository, Settings, Task};

use super::provision::repo_dir_name;

/// Builds the full instruction text for working `task` on `branch`.
pub fn build(settings: &Settings, repo: &Repository, task: &Task, branch: &str) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
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
