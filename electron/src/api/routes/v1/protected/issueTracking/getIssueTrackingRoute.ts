// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@prisma/client'
import type { Request, Response } from 'express'

// Core
import { parseRequestParams } from '../../validation'

// Lib
import { z } from 'zod'

// Utility
import { issueTrackingIdSchema } from '@electron/validators'
import { requireDatabaseClient } from '@electron/database'
import { sanitizeIssueTracking } from './utils'

type RouteParams = {
  issueTrackingId: string
}

type GetIssueTrackingResponse = {
  issueTracking: IssueTracking
}

const getIssueTrackingParamsSchema = z.object({
  issueTrackingId: issueTrackingIdSchema,
})

export async function handleGetIssueTrackingRequest(
  request: Request<RouteParams>,
  response: Response<GetIssueTrackingResponse | { error: string }>,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Get issue tracking')

  const params = parseRequestParams(
    getIssueTrackingParamsSchema,
    request,
    response,
    {
      context: 'Get issue tracking',
      errorMessage: 'Invalid issue tracking identifier',
    },
  )
  if (!params) {
    console.debug('Get issue tracking request failed route param validation')
    return
  }

  try {
    const issueTracking = await databaseClient.issueTracking.findUnique({
      where: {
        id: params.issueTrackingId,
      },
    })

    if (!issueTracking) {
      console.debug('Issue tracking not found', {
        issueTrackingId: params.issueTrackingId,
      })
      response.status(404).json({ error: 'Issue tracking not found' })
      return
    }

    response.status(200).json({
      issueTracking: sanitizeIssueTracking(issueTracking),
    })
  }
  catch (error) {
    console.error('Failed to fetch issue tracking', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to fetch issue tracking' })
    }
  }
}
