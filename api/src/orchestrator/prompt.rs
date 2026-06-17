//! Composes the prompts handed to Claude Code for a task.
//!
//! Both prompts share the same context header (org and global instructions, the
//! repo instructions, and the issue itself with its full comment thread), then
//! append a task-specific working agreement: a fresh-work protocol that ends in a
//! PR, or a CI-fix protocol that re-engages on the PR's existing branch.

use crate::db::models::{AnswerKind, Question, Repository, Settings, SourceKind, Task};
use crate::git::{IssueComment, ReviewThread};

use super::provision::repo_dir_name;

/// An open (unmerged) dependency PR branch this ticket should build on top of.
///
/// `branch` is the dependency's branch name, `title` its ticket title (for the
/// brief), and `repos` the full names of the repos where it has an open PR (so
/// the agent merges it into the right clones). See [`build`] and issue #256.
#[derive(Debug, Clone)]
pub struct DependencyBranch {
    pub title: String,
    pub branch: String,
    pub repos: Vec<String>,
}

/// Builds the instruction text for working `task` fresh on a new `branch`.
///
/// `comments` is the issue's discussion thread (empty when there is none or it
/// could not be fetched); it is rendered into the brief so the agent works from
/// the full conversation, not just the title and description.
/// `target_repos` is the full set of repos the ticket targets (priority order,
/// the first being `repo`, the focus repo). For a multi-repo ticket they are all
/// named in the working agreement so the agent has the full context up front,
/// even though it may end up opening a PR in only some of them.
/// `dependencies` are open dependency PR branches the ticket builds on top of
/// (issue #256); when present, the agent is told to merge them in first rather
/// than discovering and re-implementing them.
pub fn build(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    comments: &[IssueComment],
    target_repos: &[Repository],
    dependencies: &[DependencyBranch],
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

    prompt.push_str(&render_dependencies(dependencies, branch));
    prompt.push_str(ASKING_FOR_HELP);
    prompt
}

/// Renders the stacked-dependency section: each open dependency PR branch the
/// ticket builds on, and the merge the agent should run before implementing.
/// Returns an empty string when there are no open dependencies, so a standalone
/// ticket's brief is unchanged.
fn render_dependencies(dependencies: &[DependencyBranch], branch: &str) -> String {
    if dependencies.is_empty() {
        return String::new();
    }

    let mut section = String::from(
        "# Stacked on unmerged dependencies\n\
         - This ticket depends on other tickets whose pull requests are still OPEN, so their work \
         is NOT yet on the default branch your branch was cut from. Before implementing, bring each \
         dependency branch into your branch so you build on top of it, and do not re-implement work \
         that already exists on it.\n",
    );
    for dependency in dependencies {
        section.push_str(&format!(
            "- `{dep_branch}` (from \"{title}\") has an open PR in: {repos}. In each of those \
             repos, merge it into `{branch}` before you build: \
             `git fetch origin && git merge origin/{dep_branch}`.\n",
            dep_branch = dependency.branch,
            title = dependency.title,
            repos = dependency.repos.join(", "),
            branch = branch,
        ));
    }
    section.push_str(
        "- Resolve any merge conflicts, and keep database migrations and lockfiles linear \
         (renumber yours onto the dependency's if they collide). Then build and test on top of the \
         merged result. The dependency's PR will merge on its own; you do not need to merge it.\n",
    );
    section
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

/// Builds the instruction text to address unresolved PR review comments before
/// the PR merges.
///
/// Sent when a task's PR(s) are green and (auto-)approved but still carry
/// unresolved review threads, from the org CI reviewer bots or humans. The agent
/// works the existing `branch` (each repo with a PR is checked out): it makes the
/// changes the actionable comments call for, replies to and resolves the threads
/// it handles, and pushes, after which the review loop re-checks and merges.
/// `threads` are the unresolved threads across the task's PRs (each tagged with
/// its `repo#pr` plus `file:line`). The pass is best-effort: the agent is told to
/// reply and move on rather than force out-of-scope changes, so the queue never
/// stalls.
pub fn build_address_review(
    settings: &Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
    threads: &[ReviewThread],
    comments: &[IssueComment],
) -> String {
    let repo_path = format!("/workspace/{}", repo_dir_name(&repo.full_name));
    let mut prompt = context_header(settings, repo, task, comments);

    prompt.push_str(&format!(
        "# Addressing pull request review comments\n\
         - Your pull request for this issue passed CI, but reviewers (the org CI reviewer bots \
         and/or humans) left review comments, or requested changes, that have not been addressed. \
         The merge is gated on this: it will NOT merge while any review thread is unresolved or a \
         reviewer's \"changes requested\" stands, no matter the approval state. Address them now.\n\
         - Your cwd is `/workspace`. The focus repo `{repo}` is at `{repo_path}`, already checked \
         out on branch `{branch}` with your earlier commits. If this issue spans several repos, \
         each one with a PR is also checked out on `{branch}` as a sibling directory; each comment \
         below is tagged with its `repo#pr` so you know which repo it belongs to.\n\
         - These automated reviewers (and humans) are NOT the authority on the code; you are. They \
         can be wrong, hallucinate, or ask for defensive code that isn't warranted. Apply the \
         comments that genuinely improve the change; for ones you disagree with or that are out of \
         scope, do not force the change.\n\
         - For each comment you act on: make the fix on this branch. For each comment you decline: \
         decide deliberately. Either way, reply to the thread (a short note saying what you did or \
         why you didn't) and resolve it, so the conversation is closed out.\n\
         {reply}\
         - When you have addressed what you reasonably can, run the project's build/tests/linters, \
         then commit and push. Do not open a new pull request; the existing one updates \
         automatically. It is fine if a comment needs no code change (a reply and resolve is \
         enough).\n\
         - Do not get stuck: if a thread needs a decision you can't make or is genuinely \
         unresolvable, leave a brief reply explaining why and resolve it anyway, then move on. The \
         merge proceeds once every thread is resolved; if some are still unresolved after several \
         attempts the PR is parked for a human rather than merged over open comments.\n\
         - After pushing (or replying where no code change was needed), stop and finish with a short \
         summary. Do not watch or wait for CI; Seraphim re-checks the PR and merges it (or brings \
         you back) automatically.\n\n",
        repo = repo.full_name,
        repo_path = repo_path,
        branch = branch,
        reply = REVIEW_THREAD_COMMANDS,
    ));

    prompt.push_str(&render_review_threads(threads));
    prompt
}

/// Concrete commands for replying to and resolving a review thread, shared into
/// the addressing prompt so the agent has exact handles rather than guessing.
const REVIEW_THREAD_COMMANDS: &str = "\
    - To reply to a thread, post to its first comment id (the `comment id` shown below):\n\
    \x20    `gh api repos/OWNER/REPO/pulls/PR/comments/COMMENT_ID/replies -X POST -f body='...'`\n\
    - To resolve a thread, use the GraphQL `resolveReviewThread` mutation on its thread id (the \
    `thread id` shown below):\n\
    \x20    `gh api graphql -f query='mutation{resolveReviewThread(input:{threadId:\"THREAD_ID\"})\
    {thread{id}}}'`\n";

/// Renders the unresolved review threads for the addressing brief, each tagged
/// with `repo#pr`, its `file:line`, the author, and the identifiers the agent
/// needs to reply and resolve. Returns a short fallback when the list is empty.
fn render_review_threads(threads: &[ReviewThread]) -> String {
    if threads.is_empty() {
        return "# Review comments\n(The unresolved comments could not be enumerated; inspect the \
                PR's review threads yourself with `gh api` or `gh pr view`.)\n"
            .to_string();
    }

    let mut section = format!(
        "# Review comments\n\
         {count} unresolved review thread{plural} across this task's pull request(s), each with \
         the handles to reply and resolve it:\n\n",
        count = threads.len(),
        plural = if threads.len() == 1 { "" } else { "s" },
    );

    for (index, thread) in threads.iter().enumerate() {
        let location = match (&thread.path, thread.line) {
            (Some(path), Some(line)) => format!("{path}:{line}"),
            (Some(path), None) => path.clone(),
            _ => "(not anchored to a file)".to_string(),
        };
        let author = if thread.author.is_empty() {
            "unknown"
        } else {
            &thread.author
        };
        let comment_id = thread
            .comment_id
            .map_or_else(|| "(unknown)".to_string(), |id| id.to_string());
        let owner_repo = thread
            .repo_full_name
            .split('/')
            .next_back()
            .unwrap_or(&thread.repo_full_name);
        section.push_str(&format!(
            "## {n}. {repo}#{pr} {location} (by {author})\n\
             - repo: {full_repo} | thread id: {thread_id} | comment id: {comment_id}\n\
             {body}\n\n",
            n = index + 1,
            repo = owner_repo,
            pr = thread.pr_number,
            location = location,
            author = author,
            full_repo = thread.repo_full_name,
            thread_id = thread.thread_id,
            comment_id = comment_id,
            body = thread.body.trim(),
        ));
    }

    section
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
    // Likewise the visual self-review loop: any turn that touches the UI must look
    // at it before declaring done, so the standing instruction lives in the header
    // too (fresh work, CI fixes, revisits all alike).
    prompt.push_str(VISUAL_SELF_REVIEW);
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
    record it before you finish so it is not buried in your output. Pass valid \
    JSON to `seraphim-suggest` on stdin via a heredoc, so quotes and apostrophes \
    in your text never need shell-escaping:\n\n\
    \x20 seraphim-suggest <<'JSON'\n\
    \x20 {\"suggestions\":[{\"title\":\"<short recommendation>\",\"detail\":\"<why it helps, e.g. a setup-script snippet>\"}]}\n\
    \x20 JSON\n\n\
    Only suggest things that genuinely help; if nothing comes to mind, skip it. \
    This does not replace opening the pull request.\n";

/// Standing instruction (issue #244) to visually self-review any UI change.
///
/// Capability without a standing instruction does nothing, so this bakes the
/// loop into every task prompt (via the shared header) rather than relying on the
/// operator's global instructions. It uses the Playwright MCP baked into the
/// workspace (issue #243), prefers cheap computed-style checks over screenshots to
/// keep token cost down, reads the per-repo dev-server facts from the repo's
/// `CLAUDE.md`, and degrades gracefully: a repo with no runnable UI is skipped
/// cleanly rather than failing the task.
const VISUAL_SELF_REVIEW: &str = "\n\
    # Visual self-review (UI changes)\n\
    If your change affects the UI, you are NOT done until you have looked at it in a \
    real browser, not just reasoned about the code:\n\
    - Find how to run the repo's UI in its `CLAUDE.md`: the dev-server command, the \
    base URL/port, and the key routes (e.g. \"dev server: `npm run dev` on :5173; \
    check /, /login, /dashboard\"). If that is not recorded there, infer it (e.g. \
    the `dev` script in `package.json`) and add a short note to the repo's \
    `CLAUDE.md` so the next run has it.\n\
    - Start the dev server with test/dev data only (never production data) and open \
    the affected route(s) with the Playwright MCP browser tools (navigate, snapshot, \
    evaluate, screenshot), headless.\n\
    - Check layout (centering, spacing, alignment) using computed styles via \
    `evaluate` and the accessibility snapshot. Prefer these computed-style checks \
    over screenshots to keep cost down; take a screenshot only to confirm the final \
    result.\n\
    - Confirm it renders correctly at both mobile (375px) and desktop (1280px) \
    widths.\n\
    - If the repo has no runnable UI (a backend-only or non-web repo, no dev server, \
    or the browser tools are unavailable), SKIP this review: do not fail the task or \
    hold the PR for it, just note in your summary that you skipped visual review and \
    why.\n";

/// Guidance, appended to every fresh task prompt, on escalating to the user.
const ASKING_FOR_HELP: &str = "\n\
    # Asking the user for help\n\
    If you hit a decision you should not guess on (an ambiguous requirement, a \
    tradeoff with no clear winner, missing access, or anything where a wrong \
    assumption would be costly), ask the user instead of guessing. Pass valid \
    JSON to `seraphim-ask` on stdin via a heredoc, so quotes and apostrophes in \
    your question never need shell-escaping (a malformed payload is rejected with \
    an error, never shown to the user as a question):\n\n\
    \x20 seraphim-ask <<'JSON'\n\
    \x20 {\"questions\":[{\"prompt\":\"<your question>\",\"options\":[{\"title\":\"<short answer>\",\"description\":\"<why this>\"}]}]}\n\
    \x20 JSON\n\n\
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
            claude_account_email: String::new(),
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
            sync_error: None,
            sync_error_at: None,
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
            review_fix_attempts: 0,
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
            &[],
        );
        assert!(prompt.contains("Work issue #57"));
        assert!(prompt.contains("referencing issue #57"));
    }

    #[test]
    fn fresh_prompt_bakes_in_the_visual_self_review_loop() {
        // The visual self-review (issue #244) is a standing instruction, so a plain
        // fresh task carries it without the user asking, including the cost-saving
        // computed-style preference, the responsive widths, and the clean skip.
        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &[],
            &[],
            &[],
        );
        assert!(prompt.contains("Visual self-review"));
        assert!(prompt.contains("Playwright MCP"));
        assert!(prompt.contains("computed styles"));
        assert!(prompt.contains("375px") && prompt.contains("1280px"));
        // It reads per-repo dev facts from the repo's CLAUDE.md and skips cleanly.
        assert!(prompt.contains("CLAUDE.md"));
        assert!(prompt.contains("SKIP this review"));
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
            &[],
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
            &[],
        );
        assert!(!prompt.contains("targets multiple repositories"));
    }

    #[test]
    fn dependency_branches_are_surfaced_with_a_merge_instruction() {
        let dependencies = [DependencyBranch {
            title: "A1: Package scaffold".to_string(),
            branch: "seraphim/issue-4-package-scaffold".to_string(),
            repos: vec!["mooreslabaiv1/frontend-core".to_string()],
        }];
        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &[],
            &[sample_repo()],
            &dependencies,
        );

        assert!(prompt.contains("Stacked on unmerged dependencies"));
        // It names the dependency branch, its ticket, the repo, and the exact merge.
        assert!(prompt.contains("seraphim/issue-4-package-scaffold"));
        assert!(prompt.contains("A1: Package scaffold"));
        assert!(prompt.contains("mooreslabaiv1/frontend-core"));
        assert!(prompt.contains("git merge origin/seraphim/issue-4-package-scaffold"));
        // And it tells the agent not to re-implement the dependency's work.
        assert!(prompt.contains("do not re-implement"));
    }

    #[test]
    fn no_dependencies_leaves_the_brief_unchanged() {
        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &[],
            &[sample_repo()],
            &[],
        );
        assert!(!prompt.contains("Stacked on unmerged dependencies"));
    }

    #[test]
    fn help_guidance_uses_the_stdin_heredoc_form() {
        // Issue #260: recommend the heredoc stdin form, which sidesteps the shell
        // quoting of quotes/apostrophes that produced malformed payloads, and tell
        // the agent a malformed payload is rejected rather than shown to the user.
        let prompt = build(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &[],
            &[sample_repo()],
            &[],
        );
        assert!(prompt.contains("seraphim-ask <<'JSON'"));
        assert!(prompt.contains("seraphim-suggest <<'JSON'"));
        assert!(prompt.contains("malformed payload is rejected"));
        // The brittle single-quoted-argument example is gone.
        assert!(!prompt.contains("seraphim-ask '{"));
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

    fn review_thread(
        repo_full_name: &str,
        pr: i64,
        path: &str,
        line: i64,
        author: &str,
        body: &str,
    ) -> ReviewThread {
        ReviewThread {
            repo_full_name: repo_full_name.to_string(),
            pr_number: pr,
            thread_id: "PRRT_thread1".to_string(),
            comment_id: Some(987_654),
            path: Some(path.to_string()),
            line: Some(line),
            author: author.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn address_review_prompt_lists_each_comment_with_its_handles() {
        let threads = [review_thread(
            "navarrotech/seraphim",
            11,
            "api/src/orchestrator/review.rs",
            42,
            "mooreslabai-claude",
            "This branch can panic on an empty slice.",
        )];
        let prompt = build_address_review(
            &sample_settings(),
            &sample_repo(),
            &sample_task(),
            "seraphim/issue-57",
            &threads,
            &[],
        );

        // It reorients the agent to the addressing pass and stays on the branch.
        assert!(prompt.contains("Addressing pull request review comments"));
        assert!(prompt.contains("Do not open a new pull request"));
        // Each comment is rendered tagged with repo#pr and file:line, plus the
        // handles the agent needs to reply and resolve.
        assert!(prompt
            .contains("seraphim#11 api/src/orchestrator/review.rs:42 (by mooreslabai-claude)"));
        assert!(prompt.contains("This branch can panic on an empty slice."));
        assert!(prompt.contains("thread id: PRRT_thread1"));
        assert!(prompt.contains("comment id: 987654"));
        // It carries the reply + resolve commands and the not-the-authority guidance.
        assert!(prompt.contains("resolveReviewThread"));
        assert!(prompt.contains("/replies"));
        assert!(prompt.contains("NOT the authority"));
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
