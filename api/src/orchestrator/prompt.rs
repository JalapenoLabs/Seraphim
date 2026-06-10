//! Composes the prompts handed to Claude Code for a task.
//!
//! Both prompts share the same context header (org and global instructions, the
//! repo instructions, and the issue itself), then append a task-specific working
//! agreement: a fresh-work protocol that ends in a PR, or a CI-fix protocol that
//! re-engages on the PR's existing branch.

use crate::db::models::{AnswerKind, Question, Repository, Settings, Task};

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

    prompt.push_str(ASKING_FOR_HELP);
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

/// Builds the instruction text to revisit a PR the agent had given up on.
///
/// Unlike [`build_ci_fix`], this names merge conflicts as a likely cause (the
/// usual reason auto-merge blocks) and tells the agent to merge the base branch
/// in to clear them, in addition to fixing any failing checks. `reason` is the
/// note recorded when the PR was set aside.
pub fn build_revisit(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    reason: &str,
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let blocker = if reason.trim().is_empty() {
        "(no reason was recorded)".to_string()
    } else {
        reason.trim().to_string()
    };
    let mut prompt = context_header(settings, repo, task);

    prompt.push_str(&format!(
        "# Revisiting a stuck pull request\n\
         - The pull request for this issue was set aside as stuck. Reason recorded: {blocker}\n\
         - It may have a merge conflict with `{default}`, failing CI, or both.\n\
         - Your cwd is `/workspace`. The focus repo `{repo}` is at `{repo_path}`, already checked \
         out on branch `{branch}` with your earlier commits.\n\
         - If it conflicts with the base, bring the latest base in and resolve it: \
         `git fetch origin && git merge origin/{default}` (or rebase), fix the conflicts, and \
         continue.\n\
         - Investigate any failing checks with `gh pr checks` and `gh run view --log-failed`, then \
         fix them.\n\
         - Run the project's build/tests/linters, commit, and push. Do not open a new pull \
         request; the existing one updates automatically.\n\
         - If the conflict or failures are genuinely out of scope or unresolvable, leave a brief \
         comment on the PR explaining why, and stop without committing.\n",
        blocker = blocker,
        default = repo.default_branch,
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
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

/// Guidance, appended to every fresh task prompt, on escalating to the user.
const ASKING_FOR_HELP: &str = "\n\
    # Asking the user for help\n\
    If you hit a decision you should not guess on (an ambiguous requirement, a \
    tradeoff with no clear winner, missing access, or anything where a wrong \
    assumption would be costly), ask the user instead of guessing. Run:\n\n\
    \x20 seraphim-ask '{\"questions\":[{\"prompt\":\"<your question>\",\
    \"options\":[{\"title\":\"<short answer>\",\"description\":\"<why this>\"}]}]}'\n\n\
    Offer up to 3 suggested options per question, and you may ask several \
    questions at once. After running it, STOP and end your turn; you will be \
    automatically resumed once the user answers. Prefer asking over guessing \
    whenever it matters.\n";

/// Builds the prompt that resumes a parked task once the user has answered.
///
/// The shared Claude session is also used by other tasks while this one is
/// parked, so the prompt re-orients the agent to the specific issue and branch
/// before delivering the answers.
pub fn build_resume(repo: &Repository, task: &Task, branch: &str, answers: &[Question]) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let mut prompt = format!(
        "You are resuming work on issue #{number} (\"{title}\") in `{repo}` at `{repo_path}`, \
         on branch `{branch}`. The user has answered the question(s) you asked; continue from \
         where you left off using their guidance.\n\n",
        number = task.external_id,
        title = task.title,
        repo = repo.full_name,
    );

    for question in answers {
        prompt.push_str(&format!("Question: {}\n", question.prompt.trim()));
        let answer = question.answer.as_deref().unwrap_or("").trim();
        match question.answer_kind {
            Some(AnswerKind::Declined) => {
                if answer.is_empty() {
                    prompt.push_str(
                        "The user declined to choose and wants to discuss this further.\n\n",
                    );
                } else {
                    prompt.push_str(&format!(
                        "The user declined to choose and wants to discuss it. They said: {answer}\n\n"
                    ));
                }
            }
            _ => prompt.push_str(&format!("The user chose: {answer}\n\n")),
        }
    }

    prompt.push_str(
        "When you are finished, open the pull request as described in your original \
         working agreement. If you need another decision, you may ask again with \
         seraphim-ask.\n",
    );
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{QuestionOption, QuestionStatus, SourceKind, TaskColumn, TaskStatus};
    use sqlx::types::Json;

    fn question(prompt: &str, kind: AnswerKind, answer: &str) -> Question {
        Question {
            id: uuid::Uuid::nil(),
            task_id: uuid::Uuid::nil(),
            prompt: prompt.to_string(),
            options: Json(Vec::<QuestionOption>::new()),
            status: QuestionStatus::Answered,
            answer_kind: Some(kind),
            answer: Some(answer.to_string()),
            acknowledged: false,
            created_at: chrono::Utc::now(),
            answered_at: Some(chrono::Utc::now()),
        }
    }

    fn sample_repo() -> Repository {
        Repository {
            id: uuid::Uuid::nil(),
            full_name: "navarrotech/seraphim".to_string(),
            clone_url: String::new(),
            default_branch: "v3.0.0".to_string(),
            branch_template: String::new(),
            setup_script: String::new(),
            instructions: String::new(),
            review_policy: None,
            enabled: true,
            sync_issues: false,
            issue_labels: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn sample_task() -> Task {
        Task {
            id: uuid::Uuid::nil(),
            source_kind: SourceKind::Github,
            external_id: "57".to_string(),
            repo_id: None,
            title: "Ask the user for help".to_string(),
            body_snapshot: String::new(),
            url: String::new(),
            board_column: TaskColumn::InProgress,
            position: 0.0,
            status: TaskStatus::WaitingForInput,
            branch: None,
            pr_url: None,
            error: None,
            ci_fix_attempts: 0,
            hold: false,
            session_id: None,
            started_at: None,
            finished_at: None,
            last_activity_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn resume_prompt_reorients_and_renders_choices_and_declines() {
        let answers = [
            question("Which database?", AnswerKind::Option, "Postgres"),
            question(
                "Rename the API?",
                AnswerKind::Declined,
                "let's talk it over",
            ),
        ];
        let prompt = build_resume(
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &answers,
        );

        // Re-orientation header so the agent knows which task it is resuming.
        assert!(prompt.contains("issue #57"));
        assert!(prompt.contains("navarrotech/seraphim"));
        assert!(prompt.contains("seraphim/issue-57"));

        assert!(prompt.contains("Question: Which database?"));
        assert!(prompt.contains("The user chose: Postgres"));
        assert!(prompt.contains("Rename the API?"));
        assert!(prompt.contains("declined to choose"));
        assert!(prompt.contains("let's talk it over"));
        assert!(prompt.contains("seraphim-ask"));
    }
}
