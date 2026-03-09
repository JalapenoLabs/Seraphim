// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleVoiceStreamRequest } from './voice/stream'

type ApplyWebSocketToRouter = (router: Router) => void

export function createVoiceRouter(applyWebSocketToRouter: ApplyWebSocketToRouter): Router {
  const voiceRouter = createRouter()
  applyWebSocketToRouter(voiceRouter)

  // /api/v1/protected/voice/stream
  voiceRouter.ws('/stream', handleVoiceStreamRequest)

  return voiceRouter
}
