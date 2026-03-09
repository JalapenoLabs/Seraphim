// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'

// Lib
import { z } from 'zod'

// Utility
import { parseRequestParams } from '../../validation'
import { audioForSchema } from '@common/schema/audioFile'

// Misc
import { requireDatabaseClient } from '@electron/database'

type RouteParams = {
  audioFor: string
}

const routeParamsSchema = z.object({
  audioFor: audioForSchema,
})

export async function handleGetUserAudioFileRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Get user audio file API')

  const routeParams = parseRequestParams(
    routeParamsSchema,
    request,
    response,
    {
      context: 'Get user audio file API',
      errorMessage: 'AudioFor route parameter is required',
    },
  )
  if (!routeParams) {
    return
  }

  try {
    const user = await databaseClient.user.findFirst({
      orderBy: { createdAt: 'asc' },
    })

    if (!user) {
      console.debug('User audio file fetch requested, but no users exist')
      response.status(404).json({ error: 'User not found' })
      return
    }

    const audioFile = await databaseClient.audioFile.findUnique({
      where: {
        userId_audioFor: {
          userId: user.id,
          audioFor: routeParams.audioFor,
        },
      },
      select: {
        id: true,
        audioFor: true,
        fileName: true,
        mimeType: true,
        sizeBytes: true,
        updatedAt: true,
        createdAt: true,
      },
    })

    response.status(200).json({ audioFile })
  }
  catch (error) {
    console.error('Failed to fetch user audio file', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to fetch user audio file' })
    }
  }
}
