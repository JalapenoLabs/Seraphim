// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleVoiceStreamRequest } from './voice/stream'

export function createVoiceRouter(): Router {
  const voiceRouter = createRouter()

  // /api/v1/protected/voice/stream
  voiceRouter.ws('/stream', handleVoiceStreamRequest)

  return voiceRouter
}
