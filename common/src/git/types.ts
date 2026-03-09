// Copyright © 2026 Jalapeno Labs

import type { Octokit } from '@octokit/core'
import type {
  GithubBranchSummary,
  StandardUrlParams,
  StandardPaginatedResponseData,
} from '@common/types'

type OctokitRequestResult<Shape> = Omit<Awaited<ReturnType<Octokit['request']>>, 'data'> & {
  data: Shape
}

export type RequestResult<Shape> = [
  string,
  OctokitRequestResult<Shape> | null
]

export type GitTokenValidation = {
  isValid: boolean
  message?: string
  type: 'Github classic' | 'Github fine-grained' | 'Unknown'
  username?: string
  emails?: string[]
  scopes: string[]
  acceptedScopes: string[]
  missingScopes: string[]
}

export type GitListBranchesOptions = Required<StandardUrlParams> & {
  repoPath: string
}

export type GitListBranchesResult = StandardPaginatedResponseData & {
  branches: GithubBranchSummary[]
  defaultBranch: string | null
}

export type GitPullRequestLifecycle =
  | 'open'
  | 'draft'
  | 'merged'
  | 'closed'

export type GitCreatePullRequestOptions = {
  repoPath: string
  title: string
  description?: string
  sourceBranch: string
  targetBranch: string
  draft?: boolean
  maintainersCanModify?: boolean
}

export type GitCreatePullRequestResult = {
  pullRequestNumber: number
  url: string
}

export type GitPullRequestLocator = {
  repoPath: string
  pullRequestNumber: number
}

export type GitPullRequestInfo = {
  pullRequestNumber: number
  title: string
  description: string | null
  state: 'open' | 'closed'
  lifecycle: GitPullRequestLifecycle
  isDraft: boolean
  isMerged: boolean
  url: string
  authorUsername: string | null
  authorUrl: string | null
  sourceBranch: string
  targetBranch: string
  sourceSha: string
  createdAt: string
  updatedAt: string
  closedAt: string | null
  mergedAt: string | null
}

export type GitPullRequestCiStatus = {
  status: 'pending' | 'success' | 'failure'
  sourceSha: string
}
