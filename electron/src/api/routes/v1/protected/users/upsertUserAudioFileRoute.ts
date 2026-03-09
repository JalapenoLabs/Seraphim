// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'
import type { UpsertUserAudioFileRequest } from '@common/schema/audioFile'

// Utility
import { parseRequestBody } from '../../validation'
import { upsertUserAudioFileSchema } from '@common/schema/audioFile'

// Misc
import { requireDatabaseClient } from '@electron/database'

export type RequestBody = UpsertUserAudioFileRequest

export async function handleUpsertUserAudioFileRequest(
  request: Request<Record<string, never>, unknown, RequestBody>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Upsert user audio file API')

  const updateData = parseRequestBody(
    upsertUserAudioFileSchema,
    request,
    response,
    {
      context: 'Upsert user audio file API',
      errorMessage: 'Invalid request body',
    },
  )
  if (!updateData) {
    return
  }

  const decodedAudioBuffer = Buffer.from(updateData.file.dataBase64, 'base64')
  if (decodedAudioBuffer.length === 0) {
    console.debug('Audio file was empty after decoding', {
      audioFor: updateData.audioFor,
      fileName: updateData.file.name,
      fileType: updateData.file.mimeType,
      fileSize: updateData.file.sizeBytes,
    })
    response.status(400).json({ error: 'Invalid audio file provided' })
    return
  }

  try {
    const user = await databaseClient.user.findFirst({
      orderBy: { createdAt: 'asc' },
    })

    if (!user) {
      console.debug('User audio file update requested, but no users exist')
      response.status(404).json({ error: 'User not found' })
      return
    }

    const audioFile = await databaseClient.audioFile.upsert({
      where: {
        userId_audioFor: {
          userId: user.id,
          audioFor: updateData.audioFor,
        },
      },
      update: {
        fileName: updateData.file.name,
        mimeType: updateData.file.mimeType,
        sizeBytes: updateData.file.sizeBytes,
        data: decodedAudioBuffer,
      },
      create: {
        userId: user.id,
        audioFor: updateData.audioFor,
        fileName: updateData.file.name,
        mimeType: updateData.file.mimeType,
        sizeBytes: updateData.file.sizeBytes,
        data: decodedAudioBuffer,
      },
    })

    response.status(200).json({
      audioFileId: audioFile.id,
      audioFor: audioFile.audioFor,
    })
  }
  catch (error) {
    console.error('Failed to upsert user audio file', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to upsert user audio file' })
    }
  }
}
