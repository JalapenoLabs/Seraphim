// Copyright © 2026 Jalapeno Labs

import type {
  GitAccount,
  AuthProvider,
  IssueTracking,
  IssueTrackingProvider,
  Llm,
  Message,
  Turn,
  Task,
  TaskState,
  User,
  UserSettings as UserSettingsPrisma,
  Workspace,
  WorkspaceEnv,
} from '@prisma/client'

import type { RateLimitSnapshot, ThreadTokenUsage } from '@common/vendor/codex-protocol/v2'

// ////////////////////////// //
//        Common Basic        //
// ////////////////////////// //

export type DateISO = string
export type AbsoluteFilePath = string
export type Theme = 'light' | 'dark'
export type ThemePreference = Theme | 'system'
export type UserLanguage = 'auto' | 'en-US'
export type LogLevel = 'info' | 'log' | 'warn' | 'error' | 'debug' | 'verbose' | 'silly'
export type SystemStatus = 'operational' | 'degraded' | 'failure' | 'offline'
export type Color = 'default' | 'primary' | 'secondary' | 'success' | 'danger' | 'warning'
export type UnsubscribeFunction = () => any
export type StandardFilePointer = string | string[]

// ////////////////////////// //
//         Prisma Ext         //
// ////////////////////////// //

export type {
  GitAccount,
  AuthProvider,
  IssueTracking,
  IssueTrackingProvider,
  Llm,
  Message,
  Turn,
  Task,
  TaskState,
  User,
  Workspace,
  WorkspaceEnv,
}

export type UserSettings = Omit<UserSettingsPrisma, 'language' | 'theme'> & {
  language: UserLanguage
  theme: ThemePreference
}

export type WorkspaceWithEnv = Workspace & {
  envEntries: WorkspaceEnv[]
}

export type TurnWithMessages = Turn & {
  messages: Message[]
}

export type TaskWithFullContext = Task & {
  llm: Llm
  gitAccount: GitAccount
  turns: TurnWithMessages[]
  user: User
  workspace: WorkspaceWithEnv
}

export type LlmWithRateLimits = Llm & {
  rateLimits: RateLimitSnapshot | null
}

export type UserWithSettings = User & {
  settings: UserSettings | null
}

export type LlmUsage = {
  taskId: string
  llmId: string
  usage: ThreadTokenUsage | null
  rateLimits: RateLimitSnapshot | null
}

export type GithubRepoSummary = {
  id: number
  name: string
  fullName: string
  description: string | null
  htmlUrl: string
  cloneUrl: string
  sshUrl: string
  defaultBranch: string
  ownerLogin: string
  isPrivate: boolean
  isFork: boolean
  isArchived: boolean
  updatedAt: string
}

export type GithubBranchSummary = {
  name: string
  sha: string
  isProtected: boolean
}

// ////////////////////////// //
//            SSE             //
// ////////////////////////// //

export type SseChangeType = 'create' | 'update' | 'delete'

type SsePayloadByKind = {
  gitAccounts: GitAccount
  issueTracking: IssueTracking
  settings: UserSettings
  workspaces: WorkspaceWithEnv
  tasks: Task
  llms: LlmWithRateLimits
  usage: LlmUsage
}
export type SseChangeKind = keyof SsePayloadByKind

export type SseChangePayload<Kind extends SseChangeKind = SseChangeKind> = {
  type: SseChangeType
  kind: Kind
  data: SsePayloadByKind[Kind]
}

// ////////////////////////// //
//        Other types         //
// ////////////////////////// //

export type StandardUrlParams = {
  q?: string
  page?: number
  limit?: number
}

export type StandardPaginatedResponseData = {
  totalCount: number
  page: number
  limit: number
}

export type CodexAuthJson = {
  OPENAI_API_KEY?: string | null,
  tokens: {
    id_token?: string
    access_token?: string
    refresh_token?: string
    account_id?: string
  },
  last_refresh?: string,
}

export type FileFilter = {
  name: string
  // This does NOT include dots
  // e.g. ['txt', 'js'] not ['.txt', '.js']
  extensions: string[]
}

export type OpenDialogOptions = {
  title?: string
  defaultPath?: string
  buttonLabel?: string
  filters?: FileFilter[]
  properties?: Array<
    'openFile'
    | 'openDirectory'
    | 'multiSelections'
    | 'showHiddenFiles'
    | 'createDirectory'
    | 'promptToCreate'
    | 'noResolveAliases'
    | 'treatPackageAsDirectory'
    | 'dontAddToRecent'
  >
}

export type CommandResponse = {
  successful: boolean
  errors?: string[]
}
