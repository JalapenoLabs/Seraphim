// Copyright © 2026 Jalapeno Labs

import type {
  GitAccount,
  StandardUrlParams,
  GithubRepoSummary,
  GithubBranchSummary,
  StandardPaginatedResponseData,
  AuthProvider,
} from '@common/types'
import type { UpsertAccountRequest } from '@common/schema/accounts'

// Core
import { buildUrlParams, parseRequestBeforeSend } from '@common/api'
import { frontendClient } from '@frontend/framework/api'

// Redux
import { accountActions } from '@frontend/framework/redux/stores/accounts'
import { dispatch } from '@frontend/framework/store'

// Schema
import { upsertAccountSchema } from '@common/schema/accounts'

// /////////////////////////////// //
//           List Accounts         //
// /////////////////////////////// //

type ListAccountsResponse = {
  gitAccounts: GitAccount[]
}

// This route intentionally has no pagination
export async function listAccounts(): Promise<ListAccountsResponse> {
  const response = await frontendClient
    .get('v1/protected/git-accounts')
    .json<ListAccountsResponse>()

  dispatch(
    accountActions.setAccounts(response.gitAccounts),
  )

  return response
}


// /////////////////////////////// //
//           Get Account          //
// /////////////////////////////// //

type GetAccountResponse = {
  account: GitAccount
}

export async function getAccount(accountId: string): Promise<GetAccountResponse> {
  const response = await frontendClient
    .get(`v1/protected/git-accounts/${accountId}`)
    .json<GetAccountResponse>()

  dispatch(
    accountActions.upsertAccount(response.account),
  )

  return response
}

// /////////////////////////////// //
//        List Account Repos       //
// /////////////////////////////// //

type ListReposVisibility = 'all' | 'private' | 'public'
type ListReposRequest = StandardUrlParams & {
  visibility?: ListReposVisibility
}
type ListReposResponse = {
  results: {
    accountId: string
    username: string
    name: string
    email: string
    repos: GithubRepoSummary[]
  }[]
  failures: {
    accountId: string
    username: string
    error: string
  }[]
}

export function listRepos(request: ListReposRequest) {
  return frontendClient
    .get('v1/protected/git-accounts/repos', {
      searchParams: buildUrlParams(request),
    })
    .json<ListReposResponse>()
}

// /////////////////////////////// //
//      List Account Branches      //
// /////////////////////////////// //

type ListBranchesRequest = StandardUrlParams & {
  workspaceId: string
  gitAccountId: string
}

type ListBranchesResponse = StandardPaginatedResponseData & {
  workspaceId: string
  gitAccountId: string
  repoPath: string
  defaultBranch: string | null
  branches: GithubBranchSummary[]
}

export function listBranches(request: ListBranchesRequest) {
  return frontendClient
    .get('v1/protected/git-accounts/branches', {
      searchParams: buildUrlParams(request),
    })
    .json<ListBranchesResponse>()
}

// /////////////////////////////// //
//          Update Account         //
// /////////////////////////////// //

type UpsertAccountResponse = {
  account: GitAccount
  type: 'Github classic' | 'Github fine-grained' | 'Unknown'
  gitUserName?: string
  gitUserEmail?: string
  githubIdentity?: {
    username?: string
    emails?: string[] | null
  }
  grantedScopes?: string[]
  acceptedScopes?: string[]
}

export async function upsertGitAccount(accountId: string = '', raw: UpsertAccountRequest) {
  const json = parseRequestBeforeSend(upsertAccountSchema, raw)

  const response = await frontendClient
    .post(`v1/protected/git-accounts/upsert/${accountId}`, { json })
    .json<UpsertAccountResponse>()

  dispatch(
    accountActions.upsertAccount(response.account),
  )

  return response
}

// /////////////////////////////// //
//          Remove Account         //
// /////////////////////////////// //

type RemoveAccountRequest = {
  gitAccount: GitAccount
}

type RemoveAccountResponse = {
  provider: AuthProvider
  accountId: string
  revoked: boolean
}

export async function removeAccount(json: RemoveAccountRequest) {
  const response = await frontendClient
    .delete('v1/protected/git-accounts', { json })
    .json<RemoveAccountResponse>()

  dispatch(
    accountActions.removeAccount(json.gitAccount),
  )

  return response
}
