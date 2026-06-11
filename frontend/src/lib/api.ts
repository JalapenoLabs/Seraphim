// Frontend API client. One ky instance, one named function per endpoint, each
// the source of truth for its request/response shape.

import ky from 'ky'

import type {
  AnswerKind,
  AvailabilityWindow,
  BoardResponse,
  ConfigBundle,
  EnvSuggestion,
  EnvVar,
  IssueComment,
  IssueDetail,
  IssueThread,
  JiraBoard,
  JiraDeployment,
  NetworkAccessLevel,
  PendingQuestion,
  Question,
  Repository,
  ReviewPolicy,
  Settings,
  Task,
  TaskColumn,
  TaskDetail
} from './types'

const apiClient = ky.create({ prefixUrl: '/api/v1' })

// --- Board + tasks -----------------------------------------------------------

export function getBoard() {
  return apiClient.get('board').json<BoardResponse>()
}

export function getTask(taskId: string) {
  return apiClient.get(`tasks/${taskId}`).json<TaskDetail>()
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

// Save the private per-task notepad. Stored only in our DB, never sent to the ticket.
export function setTaskNotes(taskId: string, notes: string) {
  return apiClient.put(`tasks/${taskId}/notes`, { json: { notes } }).json<{ saved: boolean }>()
}

// --- Environment suggestions -------------------------------------------------

export function acknowledgeSuggestion(suggestionId: string, acknowledged: boolean) {
  return apiClient
    .post(`suggestions/${suggestionId}/ack`, { json: { acknowledged } })
    .json<EnvSuggestion>()
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
  post_thoughts_enabled?: boolean
  jira_enabled?: boolean
  jira_deployment?: JiraDeployment
  jira_base_url?: string
  jira_email?: string
}

export function updateSettings(body: UpdateSettingsRequest) {
  return apiClient.patch('settings', { json: body }).json<Settings>()
}

export function setPaused(paused: boolean) {
  return apiClient.post('settings/pause', { json: { paused } }).json<Settings>()
}

export type TokensRequest = {
  claude_oauth_token?: string
  github_token?: string
  jira_api_token?: string
}

export function setTokens(body: TokensRequest) {
  return apiClient.post('settings/tokens', { json: body }).json<Settings>()
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

// --- Config export / import --------------------------------------------------

export function exportConfig() {
  return apiClient.get('export').json<ConfigBundle>()
}

export function importConfig(bundle: ConfigBundle) {
  return apiClient.post('import', { json: bundle }).json()
}
