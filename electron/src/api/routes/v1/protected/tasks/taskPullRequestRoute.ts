// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'

// Utility
import { parseRequestParams } from '../../validation'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: z.string().trim().min(1),
})

export async function handleTaskPullRequestRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const params = parseRequestParams(
    taskParamsSchema,
    request,
    response,
    {
      context: 'Task pull request API',
      errorMessage: 'Task ID is required',
    },
  )
  if (!params) {
    return
  }

  console.debug('Task pull request route called', {
    taskId: params.taskId,
  })

  response.status(200).json({
    message: 'Not implemented yet',
  })
}
