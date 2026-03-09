// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'
import type { TaskNewMessageRequest } from '@common/schema/task'

// Lib
import { z } from 'zod'

// Utility
import { parseRequestBody, parseRequestParams } from '../../validation'
import { taskNewMessageSchema } from '@common/schema/task'

// Misc
import { getTaskManager } from '@electron/tasks/taskManager'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: z.string().trim().min(1),
})

export async function handleTaskNewMessageRequest(
  request: Request<RouteParams, unknown, TaskNewMessageRequest>,
  response: Response,
): Promise<void> {
  const params = parseRequestParams(
    taskParamsSchema,
    request,
    response,
    {
      context: 'Queue task message API',
      errorMessage: 'Task ID is required',
    },
  )
  if (!params) {
    return
  }

  const body = parseRequestBody(
    taskNewMessageSchema,
    request,
    response,
    {
      context: 'Queue task message API',
      errorMessage: 'Message is required',
    },
  )
  if (!body) {
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

  await task.queueUserMessage({
    role: 'User',
    type: 'userMessage',
    content: body.message,
  })

  response.status(200).json({
    message: 'Queued',
  })
}
