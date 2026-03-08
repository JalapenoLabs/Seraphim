// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@prisma/client'
import type { Request, Response } from 'express'
import type { UpsertIssueTrackingRequest } from '@common/schema/issueTracking'

// Core
import { parseRequestBody, parseRequestParams } from '../../validation'
import { broadcastSseChange } from '@electron/api/sse/sseEvents'
import { requireDatabaseClient } from '@electron/database'
import { getIssueTracker } from '@common/issueTracking/getIssueTracker'
import { IssueTracker } from '@common/issueTracking/polymorphism/issueTracker'

// Lib
import { z } from 'zod'

// Schema
import { upsertIssueTrackingSchema } from '@common/schema/issueTracking'

// Utility
import { sanitizeIssueTracking } from './utils'

const upsertIssueTrackingParamsSchema = z.object({
  issueTrackingId: z.string().trim().uuid().optional(),
})

type UpsertIssueTrackingRouteParams = z.infer<
  typeof upsertIssueTrackingParamsSchema
>

export async function handleUpsertIssueTrackingRequest(
  request: Request<
    UpsertIssueTrackingRouteParams,
    unknown,
    UpsertIssueTrackingRequest
  >,
  response: Response,
): Promise<void> {
  const params = parseRequestParams(
    upsertIssueTrackingParamsSchema,
    request,
    response,
    {
      context: 'Upsert issue tracking',
      errorMessage: 'Invalid issue tracking identifier',
    },
  )
  if (!params) {
    console.debug('Upsert issue tracking request failed route param validation')
    return
  }

  const { issueTrackingId } = params

  const payload = parseRequestBody(
    upsertIssueTrackingSchema,
    request,
    response,
    issueTrackingId
      ? {
          context: 'Update issue tracking',
          errorMessage: 'Invalid issue tracking update request body',
        }
      : {
          context: 'Create issue tracking',
          errorMessage: 'Invalid issue tracking request',
        },
  )
  if (!payload) {
    console.debug('Upsert issue tracking request failed body validation')
    return
  }

  const prisma = requireDatabaseClient('Upsert issue tracking')

  const user = await prisma.user.findFirst({
    orderBy: { createdAt: 'asc' },
  })

  if (!user) {
    console.debug('Issue tracking upsert requested but no users exist')
    response.status(404).json({ error: 'User not found' })
    return
  }

  let existingIssueTracking: IssueTracking | null = null
  if (issueTrackingId) {
    existingIssueTracking = await prisma.issueTracking.findUnique({
      where: {
        id: issueTrackingId,
      },
    })

    if (!existingIssueTracking) {
      console.debug(
        'Upsert issue tracking failed because record was not found',
        {
          issueTrackingId,
        },
      )
      response.status(400).json({
        error: 'Issue tracking not found',
      })
      return
    }
  }

  const resolvedBaseUrl = IssueTracker.resolveIssueTrackingBaseUrl(
    payload.baseUrl ?? existingIssueTracking?.baseUrl,
    payload.provider,
  )
  const resolvedAccessToken = payload.accessToken
    || existingIssueTracking?.accessToken
    || ''
  const resolvedName = payload.name ?? existingIssueTracking?.name ?? ''
  const resolvedEmail = payload.email ?? existingIssueTracking?.email ?? ''
  const resolvedTargetBoard = payload.targetBoard
    ?? existingIssueTracking?.targetBoard
    ?? ''
  const resolvedLastUsedAt = payload.accessToken
    ? new Date()
    : existingIssueTracking?.lastUsedAt ?? new Date()

  const issueTrackingForValidation: IssueTracking = {
    id: existingIssueTracking?.id ?? 'draft',
    userId: existingIssueTracking?.userId ?? user.id,
    provider: payload.provider,
    accessToken: resolvedAccessToken,
    baseUrl: resolvedBaseUrl,
    name: resolvedName,
    email: resolvedEmail,
    targetBoard: resolvedTargetBoard,
    lastUsedAt: existingIssueTracking?.lastUsedAt ?? null,
    createdAt: existingIssueTracking?.createdAt ?? new Date(),
    updatedAt: existingIssueTracking?.updatedAt ?? new Date(),
  }

  const tracker = getIssueTracker(issueTrackingForValidation)

  let isValid = false
  let errorMessage = ''

  try {
    const [ success, error ] = await tracker.check()
    isValid = success
    errorMessage = error
  }
  catch (error) {
    console.error('Issue tracking validation failed during upsert', error)
    response.status(500).json({
      error: 'Issue tracking validation failed',
    })
    return
  }

  if (!isValid) {
    console.debug('Issue tracking validation failed during upsert', {
      issueTrackingId,
      baseUrl: resolvedBaseUrl,
      targetBoard: resolvedTargetBoard,
      errorMessage,
    })
    response.status(400).json({
      error: errorMessage || 'Issue tracking credentials are invalid',
    })
    return
  }

  const issueTracking = await prisma.issueTracking.upsert({
    where: {
      id: issueTrackingId ?? 'draft',
    },
    create: {
      userId: existingIssueTracking?.userId ?? user.id,
      provider: payload.provider,
      name: resolvedName,
      baseUrl: resolvedBaseUrl,
      email: resolvedEmail,
      accessToken: resolvedAccessToken,
      targetBoard: resolvedTargetBoard,
      lastUsedAt: resolvedLastUsedAt,
    },
    update: {
      name: resolvedName,
      email: resolvedEmail,
      baseUrl: resolvedBaseUrl,
      accessToken: resolvedAccessToken,
      targetBoard: resolvedTargetBoard,
      lastUsedAt: resolvedLastUsedAt,
    },
  })

  const sanitized = sanitizeIssueTracking(issueTracking)

  if (issueTrackingId) {
    broadcastSseChange({
      type: 'update',
      kind: 'issueTracking',
      data: sanitized,
    })

    response.status(200)
  }
  else {
    broadcastSseChange({
      type: 'create',
      kind: 'issueTracking',
      data: sanitized,
    })

    response.status(201)
  }

  response.json({
    issueTracking: sanitized,
  })
}
