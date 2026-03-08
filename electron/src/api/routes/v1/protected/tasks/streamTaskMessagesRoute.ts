// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'

// Utility
import { parseRequestParams } from '../../validation'

// Misc
import {
  formatSseEvent,
  initializeSseResponse,
} from '@electron/api/sse/sseManager'
import { requireDatabaseClient } from '@electron/database'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: z.string().trim().min(1),
})

export async function handleStreamTaskMessagesRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Stream task messages SSE')

  const params = parseRequestParams(
    taskParamsSchema,
    request,
    response,
    {
      context: 'Stream task messages SSE',
      errorMessage: 'Task ID is required',
    },
  )
  if (!params) {
    return
  }

  const { taskId } = params

  const task = await databaseClient.task.findUnique({
    where: { id: taskId },
  })
  if (!task) {
    console.debug('Task message stream requested for missing task', { taskId })
    response.status(404).json({ error: 'Task not found' })
    return
  }

  const messages = await databaseClient.message.findMany({
    where: { taskId },
    orderBy: { createdAt: 'asc' },
  })

  initializeSseResponse(response)
  response.write(formatSseEvent('message', JSON.stringify(messages)))

  function handleClose() {
    response.end()
  }

  request.on('close', handleClose)
}
