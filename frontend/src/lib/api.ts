// Frontend API client. One ky instance, one named function per endpoint, each
// the source of truth for its request/response shape.

import ky from 'ky'

import type {
  BoardResponse,
  ConfigBundle,
  EnvVar,
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

export function moveTask(taskId: string, column: TaskColumn, position: number) {
  return apiClient.post(`tasks/${taskId}/move`, { json: { column, position } }).json<Task>()
}

export function setTaskHold(taskId: string, hold: boolean) {
  return apiClient.post(`tasks/${taskId}/hold`, { json: { hold } }).json<Task>()
}

// --- Repositories ------------------------------------------------------------

export function listRepos() {
  return apiClient.get('repos').json<Repository[]>()
}

export type UpsertRepoRequest = {
  full_name: string
  clone_url: string
  default_branch?: string
  branch_template?: string
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
}

export function setTokens(body: TokensRequest) {
  return apiClient.post('settings/tokens', { json: body }).json<Settings>()
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
