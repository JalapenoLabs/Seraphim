// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleGetCurrentUserRequest } from './users/getCurrentUserRoute'
import { handleGetUserAudioFileRequest } from './users/getUserAudioFileRoute'
import { handleUpdateUserSettingsRequest } from './users/updateUserSettingsRoute'
import { handleUpsertUserAudioFileRequest } from './users/upsertUserAudioFileRoute'
import { handleDeleteUserAudioFileRequest } from './users/deleteUserAudioFileRoute'

export function createUsersRouter(): Router {
  const usersRouter = createRouter()

  // /api/v1/protected/users/me
  usersRouter.get('/me', handleGetCurrentUserRequest)

  // /api/v1/protected/users/me/settings
  usersRouter.patch('/me/settings', handleUpdateUserSettingsRequest)

  // /api/v1/protected/users/me/audio-files
  usersRouter.get('/me/audio-files/:audioFor', handleGetUserAudioFileRequest)
  usersRouter.post('/me/audio-files', handleUpsertUserAudioFileRequest)
  usersRouter.delete('/me/audio-files/:audioFor', handleDeleteUserAudioFileRequest)

  return usersRouter
}
