// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleGetGitAccountRequest } from './accounts/getAccountRoute'
import { handleListGitAccountsRequest } from './accounts/listAccountsRoute'
import { handleListBranchesRequest } from './accounts/listBranchesRoute'
import { handleListReposRequest } from './accounts/listReposRoute'
import { handleRemoveAccountRequest } from './accounts/removeAccountRoute'
import { handleUpsertAccountRequest } from './accounts/upsertAccountRoute'

export function createGitAccountsRouter(): Router {
  const gitAccountsRouter = createRouter()

  // /api/v1/protected/git-accounts
  gitAccountsRouter.get('/', handleListGitAccountsRequest)

  // /api/v1/protected/git-accounts/upsert
  gitAccountsRouter.post('/upsert', handleUpsertAccountRequest)
  gitAccountsRouter.post('/upsert/:accountId', handleUpsertAccountRequest)

  // /api/v1/protected/git-accounts/
  gitAccountsRouter.delete('/', handleRemoveAccountRequest)

  // /api/v1/protected/git-accounts/branches
  gitAccountsRouter.get('/branches', handleListBranchesRequest)

  // /api/v1/protected/git-accounts/repos
  gitAccountsRouter.get('/repos', handleListReposRequest)

  // /api/v1/protected/git-accounts/repos/:githubUsername
  gitAccountsRouter.get('/repos/:githubUsername', handleListReposRequest)

  // /api/v1/protected/git-accounts/:accountId
  gitAccountsRouter.get('/:accountId', handleGetGitAccountRequest)

  return gitAccountsRouter
}
