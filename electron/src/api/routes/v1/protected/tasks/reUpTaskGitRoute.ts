// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'
import { getTaskManager } from '@electron/tasks/taskManager'

// Utility
import { parseRequestParams } from '../../validation'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: z.string().trim().min(1),
})

export async function handleReUpTaskGitRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const params = parseRequestParams(
    taskParamsSchema,
    request,
    response,
    {
      context: 'Re-up task git API',
      errorMessage: 'Task ID is required',
    },
  )
  if (!params) {
    return
  }

  console.debug('Re-up task git route called', {
    taskId: params.taskId,
  })

  const taskManager = getTaskManager()
  const task = taskManager.getTask(params.taskId)

  if (!task) {
    response.status(404).json({ error: 'Task not found' })
    return
  }

  // TODO: put this in a better route!
  await task.executeCmd('git add . && git commit -m "Updates" && git push -u origin HEAD')

  response.status(200).json({
    message: 'Completed',
  })
}
