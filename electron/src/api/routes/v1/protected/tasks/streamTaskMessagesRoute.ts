// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'

// Utility
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

type TaskMessagesStreamErrorCode =
  | 'INVALID_TASK_ID'
  | 'TASK_NOT_FOUND'
  | 'TASK_MESSAGES_UNAVAILABLE'

type TaskMessagesStreamError = {
  code: TaskMessagesStreamErrorCode
  message: string
  terminal: boolean
}

function writeTaskMessagesStreamErrorEvent(
  response: Response,
  taskError: TaskMessagesStreamError,
  retryAfterMilliseconds: number,
) {
  if (response.writableEnded) {
    console.debug('Task messages stream error event skipped because response is already closed', {
      taskError,
    })
    return
  }

  if (!response.headersSent) {
    initializeSseResponse(response)
  }

  response.write(`retry: ${retryAfterMilliseconds}\n`)
  response.write(formatSseEvent('task-error', JSON.stringify(taskError)))
  response.end()
}

export async function handleStreamTaskMessagesRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Stream task messages SSE')
  const parsedTaskParams = taskParamsSchema.safeParse(request.params)

  if (!parsedTaskParams.success) {
    console.debug('Task message stream requested with invalid task id', {
      taskId: request.params.taskId,
      issues: parsedTaskParams.error.issues,
    })
    writeTaskMessagesStreamErrorEvent(
      response,
      {
        code: 'INVALID_TASK_ID',
        message: 'Task ID is required',
        terminal: true,
      },
      0,
    )
    return
  }

  const { taskId } = parsedTaskParams.data

  try {
    const task = await databaseClient.task.findUnique({
      where: { id: taskId },
    })

    if (!task) {
      console.debug('Task message stream requested for missing task', { taskId })
      writeTaskMessagesStreamErrorEvent(
        response,
        {
          code: 'TASK_NOT_FOUND',
          message: 'Task not found',
          terminal: true,
        },
        0,
      )
      return
    }

    const messages = await databaseClient.message.findMany({
      where: { taskId },
      orderBy: { createdAt: 'asc' },
    })

    if (!response.headersSent) {
      initializeSseResponse(response)
    }

    if (!response.writableEnded) {
      response.write(formatSseEvent('message', JSON.stringify(messages)))
    }

    function handleClose() {
      response.end()
    }

    request.on('close', handleClose)
  }
  catch (error) {
    console.debug('Task message stream query failed', {
      taskId,
      error,
    })

    if (response.headersSent || response.writableEnded) {
      console.debug('Task message stream error response skipped because headers were already sent', {
        taskId,
      })
      return
    }

    writeTaskMessagesStreamErrorEvent(
      response,
      {
        code: 'TASK_MESSAGES_UNAVAILABLE',
        message: 'Task messages are temporarily unavailable',
        terminal: false,
      },
      5_000,
    )
  }
}
