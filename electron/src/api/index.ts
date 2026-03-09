// Copyright © 2026 Jalapeno Labs

import type { Server } from 'node:http'

// Core
import { createServer } from 'node:http'

// Lib
import { WebSocketServer } from 'ws'

// Utility
import { handleVoiceStreamSocket, VoiceStreamWebSocketPath } from './routes/v1/protected/voice/stream'

// Misc
import { API_PORT } from '@common/constants'
import { logFailed, logSuccess, logWarning } from '../lib/logging'
import { createApiApp } from './express'

let apiServer: Server | null = null
let voiceWebSocketServer: WebSocketServer | null = null

export async function startApi(): Promise<Server | null> {
  if (apiServer) {
    logWarning('API server already running, skipping start')
    return apiServer
  }

  const apiApplication = createApiApp()
  const apiServerInstance = createServer(apiApplication)

  const websocketServer = new WebSocketServer({
    noServer: true,
    maxPayload: 10 * 1024 * 1024,
  })

  websocketServer.on('connection', (websocket) => {
    void handleVoiceStreamSocket(websocket)
  })

  apiServerInstance.on('upgrade', (request, socket, head) => {
    const requestHost = request.headers.host || 'localhost'
    const requestUrl = request.url || '/'
    const parsedUrl = new URL(requestUrl, `http://${requestHost}`)

    if (parsedUrl.pathname !== VoiceStreamWebSocketPath) {
      socket.destroy()
      return
    }

    websocketServer.handleUpgrade(request, socket, head, (websocket) => {
      websocketServer.emit('connection', websocket, request)
    })
  })

  apiServer = apiServerInstance
  voiceWebSocketServer = websocketServer

  await new Promise<void>(function waitForListen(resolve, reject) {
    function handleListening() {
      logSuccess(`API server listening on port ${API_PORT}`)
      resolve()
    }

    function handleError(error: Error) {
      logFailed('API server failed to start')
      console.error(error)
      reject(error)
    }

    apiServerInstance.once('listening', handleListening)
    apiServerInstance.once('error', handleError)
    apiServerInstance.listen(API_PORT)
  })

  return apiServerInstance
}

export async function stopApi(): Promise<void> {
  if (!apiServer) {
    logWarning('API stop requested, but no server is running')
    return
  }

  const serverToClose = apiServer
  apiServer = null

  voiceWebSocketServer?.close()
  voiceWebSocketServer = null

  await new Promise<void>(function waitForClose(resolve, reject) {
    function handleClose(error?: Error) {
      if (error) {
        logFailed('API server failed to stop cleanly')
        console.error(error)
        reject(error)
        return
      }

      logSuccess('API server stopped')
      resolve()
    }

    serverToClose.close(handleClose)
  })
}
