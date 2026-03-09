// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleListLlmsRequest } from './llms/listLlmsRoute'
import { handleDeleteLlmRequest } from './llms/deleteLlmRoute'
import { handleGetLlmRequest } from './llms/getLlmRoute'
import { handleUpsertLlmRequest } from './llms/upsertLlmRoute'

export function createLlmsRouter(): Router {
  const llmsRouter = createRouter()

  // /api/v1/protected/llms
  llmsRouter.get('/', handleListLlmsRequest)
  llmsRouter.post('/', handleUpsertLlmRequest)
  // /api/v1/protected/llms/:llmId
  llmsRouter.get('/:llmId', handleGetLlmRequest)
  llmsRouter.post('/:llmId', handleUpsertLlmRequest)
  // /api/v1/protected/llms/:llmId
  llmsRouter.delete('/:llmId', handleDeleteLlmRequest)

  return llmsRouter
}
