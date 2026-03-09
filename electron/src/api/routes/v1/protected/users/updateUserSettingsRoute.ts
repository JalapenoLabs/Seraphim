// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'
import type { UserSettingsUpdateRequest } from '@common/schema/userSettings'

// Lib
import { LlmType, VoiceProvider } from '@prisma/client'

// Utility
import { parseRequestBody } from '../../validation'
import { userSettingsUpdateSchema } from '@common/schema/userSettings'

// Misc
import { requireDatabaseClient } from '@electron/database'
import { broadcastSseChange } from '@electron/api/sse/sseEvents'
import { UserSettings } from '@common/types'

export type RequestBody = UserSettingsUpdateRequest

export async function handleUpdateUserSettingsRequest(
  request: Request<Record<string, never>, unknown, RequestBody>,
  response: Response,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Update user settings API')

  const settingsUpdates = parseRequestBody(
    userSettingsUpdateSchema,
    request,
    response,
    {
      context: 'Update user settings API',
      errorMessage: 'Invalid request body',
    },
  )
  if (!settingsUpdates) {
    return
  }

  try {
    const user = await databaseClient.user.findFirst({
      orderBy: { createdAt: 'asc' },
    })

    if (!user) {
      console.debug('User settings update requested, but no users exist')
      response.status(404).json({ error: 'User not found' })
      return
    }

    const normalizedSettingsUpdates: Record<string, unknown> = {
      ...settingsUpdates,
    }

    const shouldValidateVoiceSettings = (
      settingsUpdates.voiceProvider !== undefined
      || settingsUpdates.voiceLlmId !== undefined
    )

    if (shouldValidateVoiceSettings) {
      const existingSettings = await databaseClient.userSettings.findUnique({
        where: { userId: user.id },
      })

      const nextVoiceProvider = settingsUpdates.voiceProvider
        ?? existingSettings?.voiceProvider
        ?? VoiceProvider.NONE
      let nextVoiceLlmId = settingsUpdates.voiceLlmId !== undefined
        ? settingsUpdates.voiceLlmId
        : existingSettings?.voiceLlmId ?? null

      if (nextVoiceProvider !== VoiceProvider.OPENAI_API_KEY) {
        nextVoiceLlmId = null
      }

      if (nextVoiceProvider === VoiceProvider.OPENAI_API_KEY) {
        if (!nextVoiceLlmId) {
          console.debug('OpenAI voice provider requested without selecting an OpenAI API key LLM account', {
            settingsUpdates,
          })
          response.status(400).json({
            error: 'An OpenAI API Key LLM account is required for OpenAI voice provider',
          })
          return
        }

        const voiceLlm = await databaseClient.llm.findFirst({
          where: {
            id: nextVoiceLlmId,
            userId: user.id,
            type: LlmType.OPENAI_API_KEY,
          },
        })

        if (!voiceLlm || !voiceLlm.apiKey?.trim()) {
          console.debug('OpenAI voice provider requested with invalid LLM account', {
            voiceLlmId: nextVoiceLlmId,
          })
          response.status(400).json({
            error: 'OpenAI voice provider requires an OpenAI API Key LLM account with a valid API key',
          })
          return
        }
      }

      normalizedSettingsUpdates.voiceProvider = nextVoiceProvider
      normalizedSettingsUpdates.voiceLlmId = nextVoiceLlmId
    }

    const {
      voiceLlmId: nextVoiceLlmId,
      ...baseSettingsUpdateData
    } = normalizedSettingsUpdates as Record<string, unknown> & {
      voiceLlmId?: string | null
    }

    const settingsUpdateData: Record<string, unknown> = {
      ...baseSettingsUpdateData,
    }
    const settingsCreateData: Record<string, unknown> = {
      ...baseSettingsUpdateData,
    }

    if (nextVoiceLlmId === null) {
      settingsUpdateData.voiceLlm = {
        disconnect: true,
      }
    }
    else if (typeof nextVoiceLlmId === 'string' && nextVoiceLlmId.trim().length) {
      settingsUpdateData.voiceLlm = {
        connect: {
          id: nextVoiceLlmId,
        },
      }
      settingsCreateData.voiceLlm = {
        connect: {
          id: nextVoiceLlmId,
        },
      }
    }

    const settings = await databaseClient.userSettings.upsert({
      where: { userId: user.id },
      update: settingsUpdateData,
      create: {
        user: {
          connect: {
            id: user.id,
          },
        },
        ...settingsCreateData,
      },
    }) as UserSettings

    broadcastSseChange({
      type: 'update',
      kind: 'settings',
      data: settings,
    })

    response.status(200).json({ settings })
  }
  catch (error) {
    console.error('Failed to update user settings', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to update user settings' })
    }
  }
}
