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
import { createVoiceRouter } from './voiceRouter'

type ApplyWebSocketToRouter = (router: Router) => void

export function createProtectedRouter(applyWebSocketToRouter: ApplyWebSocketToRouter): Router {
  const protectedRouter = createRouter()
  applyWebSocketToRouter(protectedRouter)

  protectedRouter.use('/git-accounts', createGitAccountsRouter())
  protectedRouter.use('/issue-tracking', createIssueTrackingRouter())
  protectedRouter.use('/llms', createLlmsRouter())
  protectedRouter.use('/docker', createDockerRouter())
  protectedRouter.use('/workspaces', createWorkspacesRouter())
  protectedRouter.use('/tasks', createTasksRouter())
  protectedRouter.use('/users', createUsersRouter())
  protectedRouter.use('/voice', createVoiceRouter(applyWebSocketToRouter))

  return protectedRouter
}
