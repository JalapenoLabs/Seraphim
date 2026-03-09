// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'

// Utility
import { parseRequestParams } from '../../validation'

// Misc
import { getTaskManager } from '@electron/tasks/taskManager'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: z.string().trim().min(1),
})

export async function handleInterruptTaskRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const params = parseRequestParams(
    taskParamsSchema,
    request,
    response,
    {
      context: 'Interrupt task API',
      errorMessage: 'Task ID is required',
    },
  )
  if (!params) {
    return
  }

  const taskManager = getTaskManager()
  const task = taskManager.getTask(params.taskId)
  if (!task) {
    response.status(404).json({
      error: 'Task not found',
    })
    return
  }

  const wasInterrupted = await task.interruptAndStop()
  if (!wasInterrupted) {
    response.status(409).json({
      error: 'No running turn to interrupt',
    })
    return
  }

  response.status(200).json({
    message: 'Interrupted',
  })
}
