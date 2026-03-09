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

export async function handleDeleteUserAudioFileRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Delete user audio file API')

  const routeParams = parseRequestParams(
    routeParamsSchema,
    request,
    response,
    {
      context: 'Delete user audio file API',
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
      console.debug('User audio file delete requested, but no users exist')
      response.status(404).json({ error: 'User not found' })
      return
    }

    const existingAudioFile = await databaseClient.audioFile.findUnique({
      where: {
        userId_audioFor: {
          userId: user.id,
          audioFor: routeParams.audioFor,
        },
      },
    })

    if (!existingAudioFile) {
      console.debug('User audio file delete failed, file was not found', {
        userId: user.id,
        audioFor: routeParams.audioFor,
      })
      response.status(404).json({ error: 'Audio file not found' })
      return
    }

    await databaseClient.audioFile.delete({
      where: {
        id: existingAudioFile.id,
      },
    })

    response.status(200).json({
      deleted: true,
      audioFor: routeParams.audioFor,
    })
  }
  catch (error) {
    console.error('Failed to delete user audio file', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to delete user audio file' })
    }
  }
}
