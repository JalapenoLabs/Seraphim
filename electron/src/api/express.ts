// Copyright © 2026 Jalapeno Labs

import type { Application, Router } from 'express'

// Core
import express from 'express'

// Lib
import cors from 'cors'
import helmet from 'helmet'
import rateLimit from 'express-rate-limit'
import hpp from 'hpp'
import expressWs from 'express-ws'

// Utility
import { createCorsOptions } from './cors'
import { protectedApiMiddleware } from './middleware/protectedApiMiddleware'
import { notFoundMiddleware } from './middleware/notFoundMiddleware'
import { errorMiddleware } from './middleware/errorMiddleware'
import { createPublicRouter } from './routes/public/publicRouter'
import { createProtectedRouter } from './routes/v1/protected/protectedRouter'

// Misc
import { API_BASE_PATH } from '../constants'

export function createApiApp(): Application {
  const apiApplication = express()

  const expressWebSocket = expressWs(apiApplication, undefined, {
    leaveRouterUntouched: true,
  })

  function applyWebSocketToRouter(router: Router) {
    expressWebSocket.applyTo(router)
  }

  apiApplication.disable('x-powered-by')
  apiApplication.set('trust proxy', 1)

  apiApplication.use(helmet())
  apiApplication.use(cors(createCorsOptions()))
  apiApplication.use(hpp())
  apiApplication.use(rateLimit({
    windowMs: 60_000,
    limit: 120,
    standardHeaders: 'draft-7',
    legacyHeaders: false,
  }))

  apiApplication.use(express.json({ limit: '10mb' }))
  apiApplication.use(express.urlencoded({ extended: true, limit: '10mb' }))

  const publicRouter = createPublicRouter()
  const protectedRouter = createProtectedRouter(applyWebSocketToRouter)
  apiApplication.use(publicRouter)
  apiApplication.use(
    `${API_BASE_PATH}/protected`,
    protectedApiMiddleware,
    protectedRouter,
  )

  apiApplication.use(notFoundMiddleware)
  apiApplication.use(errorMiddleware)

  return apiApplication
}
