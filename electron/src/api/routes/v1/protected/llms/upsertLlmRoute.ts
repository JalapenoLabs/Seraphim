// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'
import type { Llm, LlmWithRateLimits } from '@common/types'
import type { UpsertLlmRequest } from '@common/schema/llm'

// Core
import { parseRequestBody, parseRequestParams } from '../../validation'
import { broadcastSseChange } from '@electron/api/sse/sseEvents'
import { requireDatabaseClient } from '@electron/database'
import { getCallableLLM } from '@common/llms/call'
import { llmIdSchema } from '@electron/validators'

// Lib
import { z } from 'zod'

// Schema
import { upsertLlmSchema } from '@common/schema/llm'

// Utility
import { sanitizeLlm, setNewDefaultLlm } from './utils'

const upsertLlmParamsSchema = z.object({
  llmId: llmIdSchema.optional(),
})

type UpsertLlmRouteParams = z.infer<typeof upsertLlmParamsSchema>
export async function handleUpsertLlmRequest(
  request: Request<
    UpsertLlmRouteParams,
    unknown,
    UpsertLlmRequest
  >,
  response: Response,
): Promise<void> {
  const params = parseRequestParams(
    upsertLlmParamsSchema,
    request,
    response,
    {
      context: 'Upsert llm API',
      errorMessage: 'Llm ID is required',
    },
  )
  if (!params) {
    console.debug('Upsert llm request failed route param validation')
    return
  }

  const { llmId } = params

  const body = parseRequestBody(
    upsertLlmSchema,
    request,
    response,
    {
      context: llmId ? 'Update llm API' : 'Create llm API',
      errorMessage: 'Invalid request body',
    },
  )
  if (!body) {
    console.debug('Upsert llm request failed body validation')
    return
  }

  const prisma = requireDatabaseClient('Upsert llm API')

  try {
    const user = await prisma.user.findFirst({
      orderBy: { createdAt: 'asc' },
    })

    if (!user) {
      console.debug('LLM create requested, but no users exist')
      response.status(404).json({ error: 'User not found' })
      return
    }

    const existingLlm = llmId
      ? await prisma.llm.findUnique({
        where: { id: llmId },
      })
      : null

    if (llmId && !existingLlm) {
      console.debug('Llm update failed, llm not found', {
        llmId,
      })
      response.status(400).json({ error: 'Llm not found' })
      return
    }

    const resolvedName = body.name ?? existingLlm?.name ?? ''
    const resolvedPreferredModel = body.preferredModel
      ?? existingLlm?.preferredModel
      ?? ''
    const resolvedApiKey = body.apiKey || existingLlm?.apiKey || ''
    const resolvedTokenLimit = body.tokenLimit ?? existingLlm?.tokenLimit ?? 0
    const resolvedIsDefault = body.isDefault ?? existingLlm?.isDefault ?? false
    const resolvedUserId = existingLlm?.userId ?? user.id

    if (!resolvedName || !resolvedPreferredModel || !resolvedApiKey) {
      console.debug('Llm upsert missing required fields', {
        llmId,
        name: resolvedName,
        preferredModel: resolvedPreferredModel,
        apiKey: resolvedApiKey ? 'set' : 'missing',
      })
      response.status(400).json({
        error: 'Invalid request body',
      })
      return
    }

    const candidateLlm: Llm = {
      id: existingLlm?.id ?? 'draft',
      userId: resolvedUserId,
      type: body.type,
      name: resolvedName,
      preferredModel: resolvedPreferredModel,
      apiKey: resolvedApiKey,
      refreshToken: existingLlm?.refreshToken ?? null,
      expiresAt: existingLlm?.expiresAt ?? null,
      tokensUsed: existingLlm?.tokensUsed ?? 0,
      tokenLimit: resolvedTokenLimit,
      isDefault: resolvedIsDefault,
      lastUsedAt: existingLlm?.lastUsedAt ?? null,
      createdAt: existingLlm?.createdAt ?? new Date(),
      updatedAt: existingLlm?.updatedAt ?? new Date(),
    }

    const callableLlm = getCallableLLM(candidateLlm)
    const shouldValidate = !existingLlm || Boolean(body.apiKey)

    if (shouldValidate) {
      const [ isValid, errorMessage ] = await callableLlm.validateLlm()

      if (!isValid) {
        console.debug('LLM validation failed during upsert', {
          llmId,
          type: body.type,
          preferredModel: resolvedPreferredModel,
          errorMessage,
        })
        response.status(400).json({
          error: existingLlm
            ? errorMessage
            : `LLM validation failed: ${errorMessage}`,
        })
        return
      }
    }

    const [ savedLlm, rateLimits ] = await Promise.all([
      prisma.llm.upsert({
        where: {
          id: llmId ?? 'draft',
        },
        create: {
          user: {
            connect: {
              id: resolvedUserId,
            },
          },
          type: body.type,
          name: resolvedName,
          preferredModel: resolvedPreferredModel,
          apiKey: resolvedApiKey,
          tokenLimit: resolvedTokenLimit,
          isDefault: resolvedIsDefault,
        },
        update: {
          type: body.type,
          name: resolvedName,
          preferredModel: resolvedPreferredModel,
          apiKey: resolvedApiKey,
          tokenLimit: resolvedTokenLimit,
          isDefault: resolvedIsDefault,
        },
      }),
      callableLlm.getRateLimits(),
    ])

    if (resolvedIsDefault) {
      await setNewDefaultLlm(resolvedUserId, savedLlm.id)
    }

    const llmWithRateLimits: LlmWithRateLimits = {
      ...sanitizeLlm(savedLlm),
      rateLimits,
    }

    if (existingLlm) {
      broadcastSseChange({
        type: 'update',
        kind: 'llms',
        data: llmWithRateLimits,
      })
    }
    else {
      broadcastSseChange({
        type: 'create',
        kind: 'llms',
        data: llmWithRateLimits,
      })
    }

    response.status(201).json({
      llm: llmWithRateLimits,
    })
  }
  catch (error) {
    console.error('Failed to upsert llm', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to upsert llm' })
    }
  }
}
