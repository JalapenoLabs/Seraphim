// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'
import type { LlmWithRateLimits } from '@common/types'

// Core
import { parseRequestParams } from '../../validation'

// Lib
import { z } from 'zod'

// Utility
import { getCallableLLM } from '@common/llms/call'
import { requireDatabaseClient } from '@electron/database'
import { llmIdSchema } from '@electron/validators'
import { sanitizeLlm } from './utils'

type RouteParams = {
  llmId: string
}

type GetLlmResponse = {
  llm: LlmWithRateLimits
}

const llmParamsSchema = z.object({
  llmId: llmIdSchema,
})

export async function handleGetLlmRequest(
  request: Request<RouteParams>,
  response: Response<GetLlmResponse | { error: string }>,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Get llm API')

  const params = parseRequestParams(
    llmParamsSchema,
    request,
    response,
    {
      context: 'Get llm API',
      errorMessage: 'Llm ID is required',
    },
  )
  if (!params) {
    console.debug('Get llm request failed route param validation')
    return
  }

  try {
    const llm = await databaseClient.llm.findUnique({
      where: { id: params.llmId },
    })

    if (!llm) {
      console.debug('Llm not found', {
        llmId: params.llmId,
      })
      response.status(404).json({ error: 'Llm not found' })
      return
    }

    const callableLlm = getCallableLLM(llm)
    const rateLimits = await callableLlm.getRateLimits()

    response.status(200).json({
      llm: {
        ...sanitizeLlm(llm),
        rateLimits,
      },
    })
  }
  catch (error) {
    console.error('Failed to fetch llm', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to fetch llm' })
    }
  }
}
