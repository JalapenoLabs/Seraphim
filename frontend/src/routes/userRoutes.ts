// Copyright © 2026 Jalapeno Labs

import type { UserSettings, UserWithSettings } from '@common/types'
import type { UserSettingsUpdateRequest } from '@common/schema/userSettings'
import type { UpsertUserAudioFileRequest } from '@common/schema/audioFile'

// Lib
import { AudioFor } from '@prisma/client'

// Core
import { parseRequestBeforeSend } from '@common/api'
import { frontendClient } from '@frontend/framework/api'

// Redux
import { settingsActions } from '@frontend/framework/redux/stores/settings'
import { dispatch } from '@frontend/framework/store'

// Schema
import { userSettingsUpdateSchema } from '@common/schema/userSettings'
import { upsertUserAudioFileSchema } from '@common/schema/audioFile'

type GetCurrentUserResponse = {
  user: UserWithSettings
}

export async function getCurrentUser() {
  const response = await frontendClient
    .get('v1/protected/users/me')
    .json<GetCurrentUserResponse>()

  dispatch(
    settingsActions.setSettings(response.user.settings),
  )

  return response
}

type UpdateCurrentUserSettingsResponse = {
  settings: UserSettings
}

export async function updateCurrentUserSettings(raw: UserSettingsUpdateRequest) {
  const json = parseRequestBeforeSend(userSettingsUpdateSchema, raw)

  const response = await frontendClient
    .patch('v1/protected/users/me/settings', { json })
    .json<UpdateCurrentUserSettingsResponse>()

  dispatch(
    settingsActions.setSettings(response.settings),
  )

  return response
}

type UpsertCurrentUserAudioFileResponse = {
  audioFileId: string
  audioFor: AudioFor
}

export type UserAudioFile = {
  id: string
  audioFor: AudioFor
  fileName: string
  mimeType: string
  sizeBytes: number
  updatedAt: string
}

type GetCurrentUserAudioFileResponse = {
  audioFile: UserAudioFile | null
}

export async function getCurrentUserAudioFile(audioFor: AudioFor) {
  const response = await frontendClient
    .get(`v1/protected/users/me/audio-files/${audioFor}`)
    .json<GetCurrentUserAudioFileResponse>()

  return response
}

export async function upsertCurrentUserAudioFile(raw: UpsertUserAudioFileRequest) {
  const json = parseRequestBeforeSend(upsertUserAudioFileSchema, raw)

  const response = await frontendClient
    .post('v1/protected/users/me/audio-files', { json })
    .json<UpsertCurrentUserAudioFileResponse>()

  return response
}

type DeleteCurrentUserAudioFileResponse = {
  deleted: true
  audioFor: AudioFor
}

export async function deleteCurrentUserAudioFile(audioFor: AudioFor) {
  const response = await frontendClient
    .delete(`v1/protected/users/me/audio-files/${audioFor}`)
    .json<DeleteCurrentUserAudioFileResponse>()

  return response
}
