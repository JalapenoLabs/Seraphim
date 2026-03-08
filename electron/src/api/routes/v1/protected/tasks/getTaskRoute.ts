// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'

// Utility
import { parseRequestParams } from '../../validation'

// Misc
import { requireDatabaseClient } from '@electron/database'
import { taskIdSchema } from '@electron/validators'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: taskIdSchema,
})

export async function handleGetTaskRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Get task API')

  const params = parseRequestParams(
    taskParamsSchema,
    request,
    response,
    {
      context: 'Get task API',
      errorMessage: 'Task ID is required',
    },
  )
  if (!params) {
    return
  }

  const { taskId } = params

  try {
    const task = await databaseClient.task.findUnique({
      where: { id: taskId },
      include: {
        messages: {
          orderBy: { createdAt: 'asc' },
        },
      },
    })

    if (!task) {
      console.debug('Task not found', { taskId })
      response.status(404).json({ error: 'Task not found' })
      return
    }

    response.status(200).json({ task })
  }
  catch (error) {
    console.error('Failed to fetch task', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to fetch task' })
    }
  }
}
