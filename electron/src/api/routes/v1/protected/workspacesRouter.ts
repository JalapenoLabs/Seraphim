// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleDeleteWorkspaceRequest } from './workspaces/deleteWorkspaceRoute'
import { handleGetWorkspaceRequest } from './workspaces/getWorkspaceRoute'
import { handleListWorkspacesRequest } from './workspaces/listWorkspacesRoute'
import { handleUpsertWorkspaceRequest } from './workspaces/upsertWorkspaceRoute'

export function createWorkspacesRouter(): Router {
  const workspacesRouter = createRouter()

  // /api/v1/protected/workspaces
  workspacesRouter.get('/', handleListWorkspacesRequest)
  // /api/v1/protected/workspaces/:workspaceId
  workspacesRouter.get('/:workspaceId', handleGetWorkspaceRequest)
  // /api/v1/protected/workspaces
  // /api/v1/protected/workspaces/:workspaceId
  workspacesRouter.post('/', handleUpsertWorkspaceRequest)
  workspacesRouter.post('/:workspaceId', handleUpsertWorkspaceRequest)
  // /api/v1/protected/workspaces/:workspaceId
  workspacesRouter.delete('/:workspaceId', handleDeleteWorkspaceRequest)

  return workspacesRouter
}
