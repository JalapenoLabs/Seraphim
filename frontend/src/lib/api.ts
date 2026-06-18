// Frontend API client. One ky instance, one named function per endpoint, each
// the source of truth for its request/response shape.

import ky from 'ky'

import type {
  AnswerKind,
  AutomationRule,
  AutomationTrigger,
  AvailabilityWindow,
  BoardResponse,
  ComposeState,
  ComposeTarget,
  ConfigBundle,
  EnvSuggestion,
  IssueDraft,
  EnvVar,
  HeartAttack,
  IssueComment,
  IssueDetail,
  IssueThread,
  JiraBoard,
  JiraDeployment,
  NetworkAccessLevel,
  PendingQuestion,
  Question,
  Railway,
  RepoDeletionImpact,
  Repository,
  ResetSummary,
  ReviewPolicy,
  RuleAction,
  RuleGroup,
  RuleSource,
  Settings,
  Stats,
  TailscaleActionResponse,
  TailscaleStatus,
  Task,
  TaskColumn,
  TaskDetail
} from './types'

const apiClient = ky.create({ prefixUrl: '/api/v1' })

// Pulls the API's `{ error }` message out of a thrown ky HTTPError so callers can
// surface the backend's reason (e.g. a blocked railway reassign) in a toast.
// Falls back to the given default when there is no JSON error body.
export async function extractApiError(error: unknown, fallback: string): Promise<string> {
  if (error && typeof error === 'object' && 'response' in error) {
    try {
      const body = await (error as { response: Response }).response.json()
      if (body?.error) return String(body.error)
    } catch {
      // No JSON body; fall through to the default message.
    }
  }
  return fallback
}

// --- Board + tasks -----------------------------------------------------------

export function getBoard() {
  return apiClient.get('board').json<BoardResponse>()
}

// The tracked-file list across enabled repos, used to seed the watch page's
// activity forest so a refresh shows the full repo structure immediately (#216).
export function getRepoTree() {
  return apiClient.get('activity/tree').json<{ paths: string[] }>()
}

// The global scratchpad shown beside the board. Read/saved on its own so the
// text never rides along with every board poll.
export function getNotepad() {
  return apiClient.get('notepad').json<{ content: string }>()
}

export function setNotepad(content: string) {
  return apiClient.put('notepad', { json: { content } }).json<{ content: string }>()
}

export function getTask(taskId: string) {
  return apiClient.get(`tasks/${taskId}`).json<TaskDetail>()
}

// Create an internal ticket (no GitHub/Jira backing). Returns the new task.
// `repo_ids` are the repos the ticket targets, in priority order; the first is
// the primary one the agent branches in. An empty list leaves it tracking-only.
export function createInternalTask(body: {
  title: string
  body: string
  state: 'open' | 'closed'
  repo_ids?: string[]
}) {
  return apiClient.post('tasks', { json: body }).json<Task>()
}

// Set the repos an internal ticket targets (priority order; the first is the
// primary one the agent branches in), or clear them with an empty list. Only
// valid for internal tickets. Returns the updated task.
export function setTaskRepos(taskId: string, repoIds: string[]) {
  return apiClient.post(`tasks/${taskId}/repo`, { json: { repo_ids: repoIds } }).json<Task>()
}

// --- Live statistics ---------------------------------------------------------

export function getGlobalStats() {
  return apiClient.get('stats').json<Stats>()
}

export function getTaskStats(taskId: string) {
  return apiClient.get(`tasks/${taskId}/stats`).json<Stats>()
}

// --- Compose assistant (issue #181) ------------------------------------------

// The compose page's initial state: chat transcript, drafts, and running flag.
export function getComposeState() {
  return apiClient.get('compose').json<ComposeState>()
}

// Send the compose assistant a message; the turn runs in the background and is
// followed over the compose SSE stream.
export function sendComposeMessage(message: string) {
  return apiClient.post('compose/message', { json: { message } }).json<{ started: boolean }>()
}

// The compose assistant's own usage totals (its dedicated stats bar).
export function getComposeStats() {
  return apiClient.get('compose/stats').json<Stats>()
}

// The operator's manual edit of one draft (writes to the stored draft), including
// its target repo and railway (issue #207).
export function updateDraft(
  id: string,
  body: { title: string; body: string; repo_id: string | null; railway_id: string | null }
) {
  return apiClient.put(`compose/drafts/${id}`, { json: body }).json<IssueDraft>()
}

// Reorder the drafts to the operator's dependency sequence; bulk-create routes the
// cards into their lane's To Do in this order (issue #207).
export function reorderDrafts(ids: string[]) {
  return apiClient.post('compose/drafts/reorder', { json: { ids } }).json<IssueDraft[]>()
}

export function deleteDraft(id: string) {
  return apiClient.delete(`compose/drafts/${id}`).json<{ deleted: boolean }>()
}

// Clear all drafts and wipe the assistant's conversation history.
export function resetCompose() {
  return apiClient.post('compose/reset').json<{ reset: boolean }>()
}

// Deterministically create every draft as the chosen tracker's issue.
export function bulkCreateDrafts(target: ComposeTarget) {
  return apiClient
    .post('compose/bulk-create', { json: { target } })
    .json<{ created: number; urls: string[]; errors: string[] }>()
}

export function resetStats() {
  return apiClient.post('stats/reset').json<{ reset: boolean }>()
}

export function getIssueThread(taskId: string) {
  return apiClient.get(`tasks/${taskId}/issue`).json<IssueThread>()
}

export function addIssueComment(taskId: string, body: string) {
  return apiClient.post(`tasks/${taskId}/comment`, { json: { body } }).json<IssueComment>()
}

export function setIssueState(
  taskId: string,
  state: 'open' | 'closed',
  reason?: 'completed' | 'not_planned'
) {
  return apiClient.post(`tasks/${taskId}/issue/state`, { json: { state, reason } }).json<IssueDetail>()
}

export function moveTask(taskId: string, column: TaskColumn, position: number) {
  return apiClient.post(`tasks/${taskId}/move`, { json: { column, position } }).json<Task>()
}

export function setTaskHold(taskId: string, hold: boolean) {
  return apiClient.post(`tasks/${taskId}/hold`, { json: { hold } }).json<Task>()
}

// Mark a task blocking: while it is in progress, the agent starts no new work.
export function setTaskBlocking(taskId: string, blocking: boolean) {
  return apiClient.post(`tasks/${taskId}/blocking`, { json: { blocking } }).json<Task>()
}

// --- Bulk edit (board multi-select) ------------------------------------------

// Set hold and/or blocking across a selection. Omit a field to leave it as is.
export function bulkSetTaskFields(ids: string[], fields: { hold?: boolean; blocking?: boolean }) {
  return apiClient.post('tasks/bulk/fields', { json: { ids, ...fields } }).json<{ updated: number }>()
}

// Move a selection into a column. Done closes the linked tickets; moving out of
// Done reopens any that were closed.
export function bulkSetTaskStatus(ids: string[], column: TaskColumn) {
  return apiClient.post('tasks/bulk/status', { json: { ids, column } }).json<{ updated: number }>()
}

// Permanently delete a selection of tasks.
export function bulkDeleteTasks(ids: string[]) {
  return apiClient.post('tasks/bulk/delete', { json: { ids } }).json<{ deleted: number }>()
}

// Save the private per-task notepad. Stored only in our DB, never sent to the ticket.
export function setTaskNotes(taskId: string, notes: string) {
  return apiClient.put(`tasks/${taskId}/notes`, { json: { notes } }).json<{ saved: boolean }>()
}

// Hard-reset a stuck task: stop the agent if it's mid-turn on it, close the PR,
// delete the branch (remote + workspace), reopen a closed issue, and return the
// card to Available. Returns a summary of what was actually done.
export function hardResetTask(taskId: string) {
  return apiClient.post(`tasks/${taskId}/reset`).json<ResetSummary>()
}

// --- Environment suggestions -------------------------------------------------

export function acknowledgeSuggestion(suggestionId: string, acknowledged: boolean) {
  return apiClient
    .post(`suggestions/${suggestionId}/ack`, { json: { acknowledged } })
    .json<EnvSuggestion>()
}

// Where a one-click "create issue from this recommendation" lands.
export type CreateIssueTarget = 'internal' | 'github' | 'jira'

// Turns a recommendation into a tracked issue (Seraphim / GitHub / Jira) and
// marks it done. Returns the updated suggestion and a link when there is one.
export function createIssueFromSuggestion(suggestionId: string, target: CreateIssueTarget) {
  return apiClient
    .post(`suggestions/${suggestionId}/create`, { json: { target } })
    .json<{ suggestion: EnvSuggestion; url: string | null }>()
}

// --- Heart attacks (dead-agent management) -----------------------------------

// Clears a heart attack from the board banner once the operator has read it.
export function acknowledgeHeartAttack(id: string) {
  return apiClient.post(`heart-attacks/${id}/ack`).json<HeartAttack>()
}

// --- Questions ---------------------------------------------------------------

type PendingQuestionsResponse = {
  questions: PendingQuestion[]
}

export function getPendingQuestions() {
  return apiClient.get('questions/pending').json<PendingQuestionsResponse>()
}

export function answerQuestion(questionId: string, kind: AnswerKind, text: string) {
  return apiClient.post(`questions/${questionId}/answer`, { json: { kind, text } }).json<Question>()
}

// --- Repositories ------------------------------------------------------------

export function listRepos() {
  return apiClient.get('repos').json<Repository[]>()
}

export type UpsertRepoRequest = {
  full_name: string
  clone_url: string
  default_branch?: string
  branch_template?: string | null
  setup_script?: string
  instructions?: string
  review_policy?: ReviewPolicy | null
  enabled?: boolean
  sync_issues?: boolean
  issue_labels?: string[]
  // Re-run the setup script before every task, not just on first clone (issue #275).
  setup_script_always_run?: boolean
}

export function upsertRepo(body: UpsertRepoRequest) {
  return apiClient.post('repos', { json: body }).json<Repository>()
}

// Update an existing repo by id. Used for edits so renaming the full name
// renames the row instead of creating a duplicate (which POST, keyed on
// full_name, would do).
export function updateRepo(repoId: string, body: UpsertRepoRequest) {
  return apiClient.put(`repos/${repoId}`, { json: body }).json<Repository>()
}

// What a delete would purge, so the confirmation can spell it out first.
export function repoDeletionImpact(repoId: string) {
  return apiClient.get(`repos/${repoId}/deletion-impact`).json<RepoDeletionImpact>()
}

export function deleteRepo(repoId: string) {
  return apiClient.delete(`repos/${repoId}`).json()
}

export function importOrg(owner: string, issueLabels: string[] = []) {
  return apiClient.post('repos/import-org', { json: { owner, issue_labels: issueLabels } }).json<{
    discovered: number
    imported: number
  }>()
}

export function syncNow() {
  return apiClient.post('sync').json()
}

// --- Railways (swimlanes) ----------------------------------------------------

export function listRailways() {
  return apiClient.get('railways').json<Railway[]>()
}

export function createRailway(body: { name: string; description?: string }) {
  return apiClient.post('railways', { json: body }).json<Railway>()
}

export function updateRailway(id: string, body: { name: string; description?: string }) {
  return apiClient.put(`railways/${id}`, { json: body }).json<Railway>()
}

export function deleteRailway(id: string) {
  return apiClient.delete(`railways/${id}`).json<{ deleted: boolean }>()
}

// Toggle one lane's per-railway pause. Independent of the global master pause
// (`setPaused`); either being set stops the lane pulling new work.
export function setRailwayPaused(id: string, paused: boolean) {
  return apiClient.post(`railways/${id}/pause`, { json: { paused } }).json<Railway>()
}

// Move a repo (and all its tasks) onto a lane. The backend blocks this while a
// live turn is working the repo on its current lane and returns a 400 with an
// `{ error }` message; surface that to the operator.
export function assignRepoToRailway(railwayId: string, repoId: string) {
  return apiClient.post(`railways/${railwayId}/repos`, { json: { repo_id: repoId } }).json<Repository>()
}

export function startRailway(id: string) {
  return apiClient.post(`railways/${id}/start`).json<{ status: string; message?: string }>()
}

export function stopRailway(id: string) {
  return apiClient.post(`railways/${id}/stop`).json<{ status: string; message?: string }>()
}

// One lane's live stats. The usage-gauge fields are the same shared subscription
// figure as the global stats; the lane reads only context, cost, tokens, time.
export function getRailwayStats(id: string) {
  return apiClient.get(`railways/${id}/stats`).json<Stats>()
}

// --- Settings + workspace ----------------------------------------------------

export function getSettings() {
  return apiClient.get('settings').json<Settings>()
}

export type UpdateSettingsRequest = {
  org_name?: string
  global_instructions?: string
  default_review_policy?: ReviewPolicy
  claude_model?: string
  base_setup_script?: string
  config_repo_url?: string
  default_branch_template?: string
  availability_enabled?: boolean
  availability_timezone?: string
  availability_windows?: AvailabilityWindow[]
  availability_skip_dates?: string[]
  network_access_level?: NetworkAccessLevel
  network_access_domains?: string[]
  network_access_include_defaults?: boolean
  usage_limit_pause_enabled?: boolean
  usage_limit_threshold?: number
  railway_idle_timeout_minutes?: number
  post_thoughts_enabled?: boolean
  close_issue_on_done?: boolean
  jira_enabled?: boolean
  jira_deployment?: JiraDeployment
  jira_base_url?: string
  jira_email?: string
  jira_assigned_to_me_only?: boolean
  attention_sound_enabled?: boolean
  completion_sound_enabled?: boolean
}

export function updateSettings(body: UpdateSettingsRequest) {
  return apiClient.patch('settings', { json: body }).json<Settings>()
}

// --- Notification sounds -----------------------------------------------------

export type SoundKind = 'attention' | 'completion'

// The URL to play for a sound: the stored custom clip when one is uploaded, else
// the bundled default chime in static/. `custom` comes from settings.*_sound_custom.
export function soundUrl(kind: SoundKind, custom: boolean): string {
  return custom ? `/api/v1/settings/sounds/${kind}` : `/sounds/${kind}.wav`
}

// Upload a custom clip (the raw file is the body; its type sets the MIME).
export function uploadSound(kind: SoundKind, file: File) {
  return apiClient
    .post(`settings/sounds/${kind}`, {
      body: file,
      headers: { 'content-type': file.type || 'application/octet-stream' }
    })
    .json<Settings>()
}

// Clear a custom clip so the event falls back to the bundled default.
export function clearSound(kind: SoundKind) {
  return apiClient.delete(`settings/sounds/${kind}`).json<Settings>()
}

export function setPaused(paused: boolean) {
  return apiClient.post('settings/pause', { json: { paused } }).json<Settings>()
}

export type TokensRequest = {
  claude_oauth_token?: string
  github_token?: string
  jira_api_token?: string
  github_webhook_secret?: string
  jira_webhook_secret?: string
}

export function setTokens(body: TokensRequest) {
  return apiClient.post('settings/tokens', { json: body }).json<Settings>()
}

// --- Claude authentication ---------------------------------------------------

export type OauthStartResponse = {
  authorize_url: string
}

// Begins a Claude subscription OAuth login; returns the consent URL to open.
export function startClaudeOauth() {
  return apiClient.post('settings/claude/oauth/start').json<OauthStartResponse>()
}

// Completes the login with the code pasted from the consent callback page.
export function finishClaudeOauth(code: string) {
  return apiClient.post('settings/claude/oauth/finish', { json: { code } }).json<Settings>()
}

// Stores an Anthropic API key and switches the agent to API-key auth.
export function setClaudeApiKey(apiKey: string) {
  return apiClient.post('settings/claude/api-key', { json: { api_key: apiKey } }).json<Settings>()
}

// --- Jira --------------------------------------------------------------------

export type JiraTestResult = { ok: boolean; user?: string; error?: string }

export function testJira() {
  return apiClient.post('jira/test').json<JiraTestResult>()
}

export function listJiraBoards() {
  return apiClient.get('jira/boards').json<JiraBoard[]>()
}

// Pull boards from Jira and start following any new ones; returns the full list.
export function discoverJiraBoards() {
  return apiClient.post('jira/discover').json<JiraBoard[]>()
}

export type UpdateJiraBoardRequest = {
  sync_enabled: boolean
  status_map: Record<string, TaskColumn>
  repo_ids: string[]
}

export function updateJiraBoard(id: string, body: UpdateJiraBoardRequest) {
  return apiClient.put(`jira/boards/${id}`, { json: body }).json<JiraBoard>()
}

export function deleteJiraBoard(id: string) {
  return apiClient.delete(`jira/boards/${id}`).json<{ deleted: boolean }>()
}

type EnvVarsResponse = {
  variables: EnvVar[]
}

export function listEnvVars() {
  return apiClient.get('settings/env').json<EnvVarsResponse>()
}

// One variable to write. `value` is omitted for a secret left unchanged, so the
// server keeps its stored value (the UI never holds the raw secret to resend).
export type EnvVarWrite = {
  key: string
  value?: string
  is_secret: boolean
}

export function setEnvVars(variables: EnvVarWrite[]) {
  return apiClient.put('settings/env', { json: { variables } }).json<EnvVarsResponse>()
}

export function restartWorkspace() {
  return apiClient.post('workspace/restart').json()
}

export function recreateWorkspace() {
  return apiClient.post('workspace/recreate').json()
}

export function provisionWorkspace() {
  return apiClient.post('workspace/provision').json()
}

export function resetAgent(purgeMemories: boolean) {
  return apiClient.post('agent/reset', { json: { purge_memories: purgeMemories } }).json()
}

// --- Tailscale ---------------------------------------------------------------

export function getTailscaleStatus() {
  return apiClient.get('tailscale/status').json<TailscaleStatus>()
}

export function tailscaleUp() {
  return apiClient.post('tailscale/up').json<TailscaleActionResponse>()
}

export function tailscaleDown() {
  return apiClient.post('tailscale/down').json<TailscaleActionResponse>()
}

// Start an interactive login to authenticate the node. `force` re-authenticates
// an already-connected node (to get a fresh login URL / move it to a new tailnet).
export function tailscaleReauth(force: boolean) {
  return apiClient.post('tailscale/reauth', { json: { force } }).json<TailscaleActionResponse>()
}

export function tailscaleRestart() {
  return apiClient.post('tailscale/restart').json<TailscaleActionResponse>()
}

// --- Automation rules --------------------------------------------------------

export type RuleRequest = {
  name: string
  enabled: boolean
  source_kind: RuleSource
  triggers: AutomationTrigger[]
  criteria: RuleGroup
  action: RuleAction
}

export function listAutomationRules() {
  return apiClient.get('automation/rules').json<AutomationRule[]>()
}

export function createAutomationRule(body: RuleRequest) {
  return apiClient.post('automation/rules', { json: body }).json<AutomationRule>()
}

export function updateAutomationRule(id: string, body: RuleRequest) {
  return apiClient.put(`automation/rules/${id}`, { json: body }).json<AutomationRule>()
}

export function deleteAutomationRule(id: string) {
  return apiClient.delete(`automation/rules/${id}`)
}

// --- Self-update -------------------------------------------------------------

export type UpdateStatus = {
  current_sha: string
  current_branch: string
  latest_sha: string | null
  update_available: boolean
  // Whether the in-app update is wired up (HOST_REPO_DIR set).
  configured: boolean
  updating: boolean
  checked_at: string | null
  error: string | null
  agent_paused: boolean
  agent_working: boolean
}

export function getUpdateStatus() {
  return apiClient.get('update/status').json<UpdateStatus>()
}

export function checkForUpdate() {
  return apiClient.post('update/check').json<UpdateStatus>()
}

export function runUpdate() {
  return apiClient.post('update').json<{ status: string }>()
}

export type VersionInfo = { sha: string; branch: string }

export function getVersion() {
  return apiClient.get('version').json<VersionInfo>()
}

// --- Config export / import --------------------------------------------------

export function exportConfig() {
  return apiClient.get('export').json<ConfigBundle>()
}

export function importConfig(bundle: ConfigBundle) {
  return apiClient.post('import', { json: bundle }).json()
}
