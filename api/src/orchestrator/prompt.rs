//! Composes the prompt handed to Claude Code for a single task.
//!
//! The prompt layers the org-wide instructions, the repo-specific instructions,
//! the issue itself, and a fixed completion protocol so the agent reliably opens
//! a PR Seraphim can then detect.

use crate::db::models::{AnswerKind, Question, Repository, Settings, Task};

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

    prompt.push_str(ASKING_FOR_HELP);
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
