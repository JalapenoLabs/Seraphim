// Copyright © 2026 Jalapeno Labs

import type { WorkspaceWithEnv } from '@common/types'
import type { Request, Response } from 'express'

// Core
import { parseRequestParams } from '../../validation'

// Lib
import { z } from 'zod'

// Utility
import { requireDatabaseClient } from '@electron/database'
import { workspaceIdSchema } from '@electron/validators'

type RouteParams = {
  workspaceId: string
}

type GetWorkspaceResponse = {
  workspace: WorkspaceWithEnv
}

const workspaceParamsSchema = z.object({
  workspaceId: workspaceIdSchema,
})

export async function handleGetWorkspaceRequest(
  request: Request<RouteParams>,
  response: Response<GetWorkspaceResponse | { error: string }>,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Get workspace API')

  const params = parseRequestParams(
    workspaceParamsSchema,
    request,
    response,
    {
      context: 'Get workspace API',
      errorMessage: 'Workspace ID is required',
    },
  )
  if (!params) {
    console.debug('Get workspace request failed route param validation')
    return
  }

  try {
    const workspace = await databaseClient.workspace.findUnique({
      where: { id: params.workspaceId },
      include: { envEntries: true },
    })

    if (!workspace) {
      console.debug('Workspace not found', {
        workspaceId: params.workspaceId,
      })
      response.status(404).json({ error: 'Workspace not found' })
      return
    }

    response.status(200).json({ workspace })
  }
  catch (error) {
    console.error('Failed to fetch workspace', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to fetch workspace' })
    }
  }
}
