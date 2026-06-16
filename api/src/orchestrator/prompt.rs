//! Composes the prompts handed to Claude Code for a task.
//!
//! Both prompts share the same context header (org and global instructions, the
//! repo instructions, and the issue itself with its full comment thread), then
//! append a task-specific working agreement: a fresh-work protocol that ends in a
//! PR, or a CI-fix protocol that re-engages on the PR's existing branch.

use crate::db::models::{AnswerKind, Question, Repository, Settings, SourceKind, Task};
use crate::git::IssueComment;

use super::provision::repo_dir_name;

/// Builds the instruction text for working `task` fresh on a new `branch`.
///
/// `comments` is the issue's discussion thread (empty when there is none or it
/// could not be fetched); it is rendered into the brief so the agent works from
/// the full conversation, not just the title and description.
/// `target_repos` is the full set of repos the ticket targets (priority order,
/// the first being `repo`, the focus repo). For a multi-repo ticket they are all
/// named in the working agreement so the agent has the full context up front,
/// even though it may end up opening a PR in only some of them.
pub fn build(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    comments: &[IssueComment],
    target_repos: &[Repository],
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let mut prompt = context_header(settings, repo, task, comments);

    // A GitHub task's PR should reference its issue (so it links/closes on merge);
    // an internal task has no upstream issue, so the PR is opened without one.
    let issue_reference = match task.source_kind {
        SourceKind::Github => format!(", referencing issue #{}", task.external_id),
        _ => String::new(),
    };

    prompt.push_str(&format!(
        "# Working agreement\n\
         - Your cwd is `/workspace`. Every configured repo is cloned here as a sibling \
         directory, so you can read and edit across repos as needed.\n\
         - The focus repo for this issue is `{repo}` at `{repo_path}`, already on a fresh \
         branch `{branch}` cut from `{default}`. `cd` there to do the primary work.\n\
         - Implement the change, then run the project's build/tests/linters and make them pass.\n\
         - Commit your work and push the branch.\n\
         - Open a pull request against `{default}` with `gh pr create`{issue_reference}.\n\
         - If this issue needs changes in more than one repo, make them in each affected sibling \
         repo on a branch with the SAME name `{branch}`, and open a pull request in each. Seraphim \
         tracks every PR you open on `{branch}`: the task is not done until all of them pass CI and \
         merge, so do not leave a needed repo without its PR. In each such repo, branch from the \
         up-to-date target first (`git fetch origin`, then cut `{branch}` from the latest default \
         branch) so your PR opens on top of the current target and avoids needless merge conflicts.\n\
         - After the PR(s) are open, stop. Do not poll, watch, or wait for CI to finish: Seraphim \
         watches every PR's checks for you and automatically brings you back to fix anything that \
         fails, so a long `gh pr checks` wait only blocks the queue behind you.\n\
         - Finish with a short summary of what you changed.\n",
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
        default = repo.default_branch,
        issue_reference = issue_reference,
    ));

    // When the ticket targets several repos, name them so the agent knows the full
    // blast radius up front. It branches in the focus repo above; it should change
    // and open a PR in each repo that actually needs it. Some targets may need no
    // change, and that is fine: do not force an empty PR.
    if target_repos.len() > 1 {
        let names = target_repos
            .iter()
            .map(|repo| repo.full_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        prompt.push_str(&format!(
            "- This ticket targets multiple repositories: {names}. They are all cloned as \
             siblings under `/workspace`, and `{focus}` (above) is the primary one. Make the \
             changes each affected repo needs and open a pull request (same branch name \
             `{branch}`) in every repo you actually change. A target that needs no change \
             needs no PR.\n",
            names = names,
            focus = repo.full_name,
            branch = branch,
        ));
    }

    prompt.push_str(ASKING_FOR_HELP);
    prompt
}

/// Builds the instruction text to re-engage `task` on its PR's failing CI.
///
/// `failing_checks` names the checks that failed (may be empty if they couldn't
/// be enumerated). The agent works the existing `branch` and is told to stay in
/// scope: re-run a transient infrastructure flake once, and for failures that
/// aren't this issue's doing comment and stop rather than force unrelated changes.
pub fn build_ci_fix(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    failing_checks: &[String],
    comments: &[IssueComment],
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let checks = if failing_checks.is_empty() {
        "(the failing checks could not be enumerated; inspect them yourself)".to_string()
    } else {
        failing_checks.join(", ")
    };
    let mut prompt = context_header(settings, repo, task, comments);

    prompt.push_str(&format!(
        "# Fixing CI\n\
         - You previously opened a pull request for this issue, but its CI is failing: {checks}.\n\
         - Your cwd is `/workspace`. The focus repo `{repo}` is at `{repo_path}`, already checked \
         out on branch `{branch}` with your earlier commits. If this issue spans several repos, \
         each one with a PR is also checked out on `{branch}` as a sibling directory; a failing \
         check above is tagged with its `repo#pr` so you know which repo to fix.\n\
         - Investigate the failures first: use `gh pr checks` and `gh run view --log-failed` (or \
         open the PR's checks) to read the actual errors before changing anything.\n\
         - Fix the failures on this branch, then run the project's build/tests/linters, commit, and \
         push. Do not open a new pull request; the existing one updates automatically.\n\
         - If a failure is a transient infrastructure flake (a runner hiccup, a network blip, a \
         known base-image pull failure) rather than a real problem with the code, re-run just the \
         failed jobs once with `gh run rerun --failed` and then stop without committing.\n\
         - Stay in scope: if the failures are pre-existing on `{default}` or otherwise unrelated to \
         this issue, do not force unrelated changes. Leave a brief comment on the PR explaining why, \
         and stop without committing.\n\
         - After pushing a fix (or re-running a flake), stop and finish with a short summary. Do not \
         watch or wait for CI; Seraphim re-checks the PR and brings you back if it is still failing.\n",
        checks = checks,
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
        default = repo.default_branch,
    ));

    prompt
}

/// Builds the instruction text to resolve a merge conflict that blocked the PR's
/// auto-merge.
///
/// Sent when a green PR fails to auto-merge, which almost always means another
/// pull request landed on the base first and the branch now conflicts. The agent
/// works the existing `branch`: merge the base in, resolve the conflicts, verify
/// any migrations stay linear, and push, after which the review loop re-merges.
/// `reason` is the note recorded when the merge failed.
pub fn build_merge_conflict(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    reason: &str,
    comments: &[IssueComment],
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let blocker = if reason.trim().is_empty() {
        "(no reason was recorded)".to_string()
    } else {
        reason.trim().to_string()
    };
    let mut prompt = context_header(settings, repo, task, comments);

    prompt.push_str(&format!(
        "# Resolving a merge conflict\n\
         - Auto-merge of this issue's pull request was blocked: {blocker}\n\
         - This almost always means another pull request merged into `{default}` first and your \
         branch now conflicts with it. Resolve it yourself rather than leaving it for a human.\n\
         - Your cwd is `/workspace`. The focus repo `{repo}` is at `{repo_path}`, already checked \
         out on branch `{branch}` with your earlier commits.\n\
         - Bring the latest base in and resolve the conflict: \
         `git fetch origin && git merge origin/{default}`, fix every conflicted file, and complete \
         the merge. (Use a merge, not a rebase, so you don't have to force-push.)\n\
         {migrations}\
         - Run the project's build/tests/linters to confirm the merge is clean, then commit and \
         push. Do not open a new pull request; the existing one updates automatically.\n\
         - If the conflict is genuinely out of scope or you cannot resolve it safely, leave a \
         brief comment on the PR explaining why, and stop without committing.\n\
         - After pushing, stop and finish with a short summary. Do not wait for CI; Seraphim \
         re-checks the PR and re-merges it (or brings you back) automatically.\n",
        blocker = blocker,
        default = repo.default_branch,
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
        migrations = MIGRATION_LINEARITY,
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
    comments: &[IssueComment],
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let blocker = if reason.trim().is_empty() {
        "(no reason was recorded)".to_string()
    } else {
        reason.trim().to_string()
    };
    let mut prompt = context_header(settings, repo, task, comments);

    prompt.push_str(&format!(
        "# Revisiting a stuck pull request\n\
         - The pull request for this issue was set aside as stuck. Reason recorded: {blocker}\n\
         - It may have a merge conflict with `{default}`, failing CI, or both.\n\
         - Your cwd is `/workspace`. The focus repo `{repo}` is at `{repo_path}`, already checked \
         out on branch `{branch}` with your earlier commits.\n\
         - If it conflicts with the base, bring the latest base in and resolve it: \
         `git fetch origin && git merge origin/{default}` (or rebase), fix the conflicts, and \
         continue.\n\
         {migrations}\
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
        migrations = MIGRATION_LINEARITY,
    ));

    prompt
}

/// The shared prompt header: who the agent is, the org/global/repo instructions,
/// the issue under work, and its comment thread.
fn context_header(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    comments: &[IssueComment],
) -> String {
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
    let body = if task.body_snapshot.trim().is_empty() {
        "(no description provided)"
    } else {
        task.body_snapshot.trim()
    };
    // A GitHub task carries an issue number and link; an internal task is just a
    // brief in Seraphim, so it is described without a (meaningless) issue number.
    match task.source_kind {
        SourceKind::Github => prompt.push_str(&format!(
            "Work issue #{number}: {title}\n\nIssue description:\n{body}\n\nIssue link: {url}\n\n",
            number = task.external_id,
            title = task.title,
            url = task.url,
        )),
        _ => prompt.push_str(&format!(
            "Work this task: {title}\n\nTask description:\n{body}\n\n",
            title = task.title,
        )),
    }

    // The full discussion (when any), so the agent treats comments as part of the
    // brief rather than only the title and description.
    prompt.push_str(&render_discussion(comments));

    // Shared by every mode: noticing missing tooling can happen on any run, so
    // the recommend-improvements guidance lives in the common header.
    prompt.push_str(ENVIRONMENT_SUGGESTIONS);
    prompt
}

/// Renders the issue's comment thread for the brief, oldest first.
///
/// Comment bodies are kept verbatim (Markdown intact), so any attachments,
/// which GitHub embeds as image/file links in the issue or comment Markdown,
/// reach the agent as openable URLs. Returns an empty string when there are no
/// comments with content, so the header stays unchanged for plain issues.
fn render_discussion(comments: &[IssueComment]) -> String {
    // Skip comments with no text (e.g. an attachment-only body GitHub stored
    // empty); they would add a header with nothing under it.
    let rendered: Vec<&IssueComment> = comments
        .iter()
        .filter(|comment| !comment.body.as_deref().unwrap_or("").trim().is_empty())
        .collect();
    if rendered.is_empty() {
        return String::new();
    }

    let mut section = format!(
        "# Issue discussion\n\
         The issue has {count} comment{plural} below, oldest first. Treat them as part \
         of the task: they may clarify, refine, correct, or override the description \
         above. Any images or files attached to the issue or a comment appear as links \
         in this Markdown; open them with `gh` or `curl` if you need their contents.\n\n",
        count = rendered.len(),
        plural = if rendered.len() == 1 { "" } else { "s" },
    );

    for (index, comment) in rendered.iter().enumerate() {
        let body = comment.body.as_deref().unwrap_or("").trim();
        // GitHub's author_association ("OWNER", "MEMBER", "CONTRIBUTOR", ...) helps
        // the agent weigh a maintainer's note over a passer-by's; omit the noise of
        // "NONE" and the rare empty value.
        let association = match comment.author_association.trim() {
            "" | "NONE" => String::new(),
            other => format!(", {other}"),
        };
        // The created date alone (the leading "YYYY-MM-DD" of the ISO timestamp) is
        // enough ordering context without the verbose time component.
        let date = comment.created_at.get(0..10).unwrap_or(&comment.created_at);
        section.push_str(&format!(
            "## Comment {n} by {author}{association} ({date})\n{body}\n\n",
            n = index + 1,
            author = comment.user.login,
        ));
    }

    section
}

/// Guidance, shared by the conflict-resolution prompts, on keeping database
/// migrations in a single linear order after merging the base branch in.
///
/// Two PRs that each add a migration are fine in isolation, but once both land
/// the branch holds migrations authored in parallel; without a check they can end
/// up out of order, duplicated, or numbered to collide. The agent verifies the
/// migrations within its own scope still form one linear sequence.
const MIGRATION_LINEARITY: &str = "\
    - If your branch adds database migrations, double-check them after merging the base in: \
    another PR may have added migrations too. Make sure the migrations in your scope still form a \
    single linear sequence (correct ordering and numbering, no duplicate or colliding versions), \
    and renumber yours onto the latest if they now clash.\n";

/// Guidance, appended to every task prompt, on recommending setup improvements.
const ENVIRONMENT_SUGGESTIONS: &str = "\n\
    # Recommend environment improvements\n\
    If during the task you noticed tooling or setup that was missing or would \
    make future runs on a fresh workstation go smoother (a toolchain you had to \
    install, a CLI that was absent, a slow step that a cached tool would fix), \
    record it before you finish so it is not buried in your output. Run:\n\n\
    \x20 seraphim-suggest '{\"suggestions\":[{\"title\":\"<short recommendation>\",\
    \"detail\":\"<why it helps and how to apply it, e.g. a setup-script snippet>\"}]}'\n\n\
    Only suggest things that genuinely help; if nothing comes to mind, skip it. \
    This does not replace opening the pull request.\n";

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
    // GitHub tasks re-orient by their issue number; internal tasks by their title.
    let reference = match task.source_kind {
        SourceKind::Github => format!("issue #{} (\"{}\")", task.external_id, task.title),
        _ => format!("task \"{}\"", task.title),
    };
    let mut prompt = format!(
        "You are resuming work on {reference} in `{repo}` at `{repo_path}`, \
         on branch `{branch}`. The user has answered the question(s) you asked; continue from \
         where you left off using their guidance.\n\n",
        reference = reference,
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
    use crate::git::IssueUser;
    use sqlx::types::Json;

    fn comment(
        login: &str,
        association: &str,
        created_at: &str,
        body: Option<&str>,
    ) -> IssueComment {
        IssueComment {
            user: IssueUser {
                login: login.to_string(),
                avatar_url: String::new(),
                html_url: String::new(),
            },
            body: body.map(str::to_string),
            created_at: created_at.to_string(),
            author_association: association.to_string(),
        }
    }

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

    /// A minimal `Settings` for the prompt builders. Only `org_name` and
    /// `global_instructions` reach the prompt; the rest are inert defaults.
    fn sample_settings() -> Settings {
        use crate::db::models::{JiraDeployment, NetworkAccessLevel, ReviewPolicy};
        Settings {
            org_name: "JalapenoLabs".to_string(),
            global_instructions: String::new(),
            default_review_policy: ReviewPolicy::None,
            agent_paused: false,
            claude_model: String::new(),
            workspace_image_tag: String::new(),
            base_setup_script: String::new(),
            config_repo_url: String::new(),
            default_branch_template: String::new(),
            config_repo_error: None,
            current_session_id: None,
            updated_at: chrono::Utc::now(),
            claude_token_set: false,
            claude_auth_mode: crate::db::models::ClaudeAuthMode::Subscription,
            claude_usage_token_set: false,
            github_token_set: false,
            availability_enabled: false,
            availability_timezone: "UTC".to_string(),
            availability_windows: Json(Vec::new()),
            availability_skip_dates: Json(Vec::new()),
            network_access_level: NetworkAccessLevel::Full,
            network_access_domains: Json(Vec::new()),
            network_access_include_defaults: true,
            usage_limit_pause_enabled: false,
            usage_limit_threshold: 80,
            usage_paused_until: None,
            railway_idle_timeout_minutes: 30,
            post_thoughts_enabled: false,
            close_issue_on_done: true,
            jira_enabled: false,
            jira_deployment: JiraDeployment::Cloud,
            jira_base_url: String::new(),
            jira_email: String::new(),
            jira_assigned_to_me_only: true,
            jira_account_id: String::new(),
            jira_token_set: false,
            github_webhook_secret_set: false,
            jira_webhook_secret_set: false,
            attention_sound_enabled: true,
            completion_sound_enabled: true,
            attention_sound_custom: false,
            completion_sound_custom: false,
            jira_token_preview: None,
            claude_token_preview: None,
            github_token_preview: None,
            cooldown_until: None,
        }
    }

    fn sample_repo() -> Repository {
        Repository {
            id: uuid::Uuid::nil(),
            railway_id: uuid::Uuid::nil(),
            full_name: "navarrotech/seraphim".to_string(),
            clone_url: String::new(),
            default_branch: "v3.0.0".to_string(),
            branch_template: None,
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
            railway_id: uuid::Uuid::nil(),
            source_kind: SourceKind::Github,
            external_id: "57".to_string(),
            repo_id: None,
            target_repo_ids: Json(Vec::new()),
            jira_board_id: None,
            title: "Ask the user for help".to_string(),
            body_snapshot: String::new(),
            url: String::new(),
            author_login: None,
            author_avatar_url: None,
            external_state: Some("open".to_string()),
            board_column: TaskColumn::InProgress,
            position: 0.0,
            status: TaskStatus::WaitingForInput,
            branch: None,
            pr_url: None,
            error: None,
            ci_fix_attempts: 0,
            hold: false,
            blocking: false,
            notes: String::new(),
            session_id: None,
            started_at: None,
            finished_at: None,
            last_activity_at: None,
            stats_reset_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn internal_task_prompt_omits_the_issue_number_and_reference() {
        let mut task = sample_task();
        task.source_kind = SourceKind::Internal;
        task.external_id = "3".to_string();
        task.title = "Port over the PDF docs".to_string();
        task.body_snapshot = "Move the docs from DebugAgent.".to_string();

        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &task,
            "seraphim/task-3",
            &[],
            &[],
        );

        // An internal ticket has no upstream issue, so the brief and the PR step
        // must not reference a (meaningless, possibly colliding) issue number.
        assert!(prompt.contains("Work this task: Port over the PDF docs"));
        assert!(prompt.contains("Move the docs from DebugAgent."));
        assert!(!prompt.contains("issue #3"));
        assert!(!prompt.contains("referencing issue"));
        // The PR step is still present, just without the issue reference.
        assert!(prompt.contains("Open a pull request against `v3.0.0` with `gh pr create`."));
    }

    #[test]
    fn github_task_prompt_keeps_the_issue_reference() {
        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &[],
            &[],
        );
        assert!(prompt.contains("Work issue #57"));
        assert!(prompt.contains("referencing issue #57"));
    }

    #[test]
    fn multi_repo_ticket_names_every_target_repo() {
        let mut task = sample_task();
        task.source_kind = SourceKind::Internal;

        let focus = sample_repo();
        let mut other = sample_repo();
        other.id = uuid::Uuid::from_u128(1);
        other.full_name = "navarrotech/yearloom".to_string();
        let targets = [focus.clone(), other];

        let prompt = build(
            &sample_settings(),
            &focus,
            &task,
            "seraphim/task-57",
            &[],
            &targets,
        );

        assert!(prompt.contains("This ticket targets multiple repositories"));
        assert!(prompt.contains("navarrotech/seraphim"));
        assert!(prompt.contains("navarrotech/yearloom"));
    }

    #[test]
    fn single_repo_ticket_omits_the_multi_repo_line() {
        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &[],
            &[sample_repo()],
        );
        assert!(!prompt.contains("targets multiple repositories"));
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

    #[test]
    fn merge_conflict_prompt_directs_a_base_merge_and_linear_migrations() {
        let settings = sample_settings();
        let prompt = build_merge_conflict(
            &settings,
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            "Auto-merge failed: not mergeable.",
            &[],
        );

        // It reorients the agent to the conflict and the exact recovery command.
        assert!(prompt.contains("Resolving a merge conflict"));
        assert!(prompt.contains("Auto-merge failed: not mergeable."));
        assert!(prompt.contains("git merge origin/v3.0.0"));
        // It carries the migration-linearity guidance the issue asked for.
        assert!(prompt.contains("migrations"));
        assert!(prompt.contains("single linear sequence"));
        // It must not tell the agent to open a new PR; the existing one updates.
        assert!(prompt.contains("Do not open a new pull request"));
    }

    #[test]
    fn revisit_prompt_also_carries_migration_guidance() {
        let settings = sample_settings();
        let prompt = build_revisit(
            &settings,
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            "set aside as stuck",
            &[],
        );
        assert!(prompt.contains("single linear sequence"));
    }

    #[test]
    fn discussion_renders_every_comment_with_its_attachments() {
        let comments = [
            comment(
                "maintainer",
                "OWNER",
                "2026-01-02T10:00:00Z",
                Some("Use Postgres, not SQLite."),
            ),
            comment(
                "passerby",
                "NONE",
                "2026-01-03T11:30:00Z",
                Some("Here is a screenshot ![shot](https://example.com/a.png)"),
            ),
        ];
        let section = render_discussion(&comments);

        // The section header counts the comments and both render verbatim, oldest
        // first, so the attachment link survives for the agent to open.
        assert!(section.starts_with("# Issue discussion"));
        assert!(section.contains("2 comments below"));
        assert!(section.contains("## Comment 1 by maintainer, OWNER (2026-01-02)"));
        assert!(section.contains("Use Postgres, not SQLite."));
        assert!(section.contains("## Comment 2 by passerby (2026-01-03)"));
        assert!(section.contains("https://example.com/a.png"));
        // A "NONE" association is dropped, and the timestamp is trimmed to the day.
        assert!(!section.contains("NONE"));
        assert!(!section.contains("T11:30:00Z"));
    }

    #[test]
    fn discussion_is_empty_without_substantive_comments() {
        // Nothing to render for no comments, or a comment whose body is blank.
        assert!(render_discussion(&[]).is_empty());
        let blank = [comment(
            "ghost",
            "NONE",
            "2026-01-01T00:00:00Z",
            Some("   "),
        )];
        assert!(render_discussion(&blank).is_empty());
        let bodiless = [comment("ghost", "NONE", "2026-01-01T00:00:00Z", None)];
        assert!(render_discussion(&bodiless).is_empty());
    }
}
