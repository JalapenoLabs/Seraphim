// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { createGitAccountsRouter } from './accountsRouter'
import { createIssueTrackingRouter } from './issueTrackingRouter'
import { createLlmsRouter } from './llmsRouter'
import { createDockerRouter } from './docker/dockerRouter'
import { createTasksRouter } from './tasksRouter'
import { createUsersRouter } from './usersRouter'
import { createWorkspacesRouter } from './workspacesRouter'

export function createProtectedRouter(): Router {
  const protectedRouter = createRouter()

  protectedRouter.use('/git-accounts', createGitAccountsRouter())
  protectedRouter.use('/issue-tracking', createIssueTrackingRouter())
  protectedRouter.use('/llms', createLlmsRouter())
  protectedRouter.use('/docker', createDockerRouter())
  protectedRouter.use('/workspaces', createWorkspacesRouter())
  protectedRouter.use('/tasks', createTasksRouter())
  protectedRouter.use('/users', createUsersRouter())

  return protectedRouter
}
