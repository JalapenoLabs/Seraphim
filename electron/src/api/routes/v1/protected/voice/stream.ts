// Copyright © 2026 Jalapeno Labs

import type { WebsocketRequestHandler } from 'express-ws'

// Utility
import { requireDatabaseClient } from '@electron/database'
import { getVoiceProvider } from '@common/voice/call'
import { voiceClientMessageSchema } from '@common/voice/protocol'
import { LlmType, VoiceProvider } from '@prisma/client'

export const handleVoiceStreamRequest: WebsocketRequestHandler = async (websocket) => {
  const databaseClient = requireDatabaseClient('Voice stream route')

  const user = await databaseClient.user.findFirst({
    orderBy: { createdAt: 'asc' },
    include: {
      settings: true,
    },
  })

  if (!user?.settings) {
    websocket.send(JSON.stringify({
      type: 'error',
      message: 'User settings were not found for voice streaming',
    }))
    websocket.close()
    return
  }

  if (user.settings.voiceProvider !== VoiceProvider.OPENAI_API_KEY) {
    websocket.send(JSON.stringify({
      type: 'error',
      message: `Unsupported voice provider: ${user.settings.voiceProvider}`,
    }))
    websocket.close()
    return
  }

  if (!user.settings.voiceLlmId) {
    websocket.send(JSON.stringify({
      type: 'error',
      message: 'Voice provider requires an OpenAI API Key LLM selection',
    }))
    websocket.close()
    return
  }

  const llm = await databaseClient.llm.findFirst({
    where: {
      id: user.settings.voiceLlmId,
      userId: user.id,
      type: LlmType.OPENAI_API_KEY,
    },
  })

  if (!llm?.apiKey?.trim()) {
    websocket.send(JSON.stringify({
      type: 'error',
      message: 'Selected OpenAI API Key LLM account is invalid or missing an API key',
    }))
    websocket.close()
    return
  }

  const provider = getVoiceProvider({
    provider: user.settings.voiceProvider,
    llm,
  })

  websocket.send(JSON.stringify({ type: 'ready' }))

  let queue = Promise.resolve()

  websocket.on('message', (payload) => {
    const payloadAsString
      = typeof payload === 'string'
        ? payload
        : Buffer.isBuffer(payload)
          ? payload.toString('utf8')
          : Array.isArray(payload)
            ? Buffer.concat(payload).toString('utf8')
            : payload instanceof ArrayBuffer
              ? Buffer.from(payload).toString('utf8')
              : null

    if (!payloadAsString) {
      websocket.send(JSON.stringify({
        type: 'error',
        message: 'Voice stream expected JSON text payloads',
      }))
      return
    }

    let parsedJson: unknown = null
    try {
      parsedJson = JSON.parse(payloadAsString)
    }
    catch (error) {
      console.debug('Voice stream received malformed JSON payload.', {
        payloadAsString,
        error,
      })
      websocket.send(JSON.stringify({
        type: 'error',
        message: 'Voice stream received malformed JSON payload',
      }))
      return
    }

    const parsedMessage = voiceClientMessageSchema.safeParse(parsedJson)
    if (!parsedMessage.success) {
      websocket.send(JSON.stringify({
        type: 'error',
        message: 'Voice stream received an invalid message payload',
      }))
      return
    }

    queue = queue
      .then(async () => {
        const decodedBuffer = Buffer.from(parsedMessage.data.dataBase64, 'base64')
        const audioBytes = new Uint8Array(new ArrayBuffer(decodedBuffer.length))
        audioBytes.set(decodedBuffer)

        if (!audioBytes.length) {
          console.debug('Voice stream ignored an empty decoded audio chunk.')
          return
        }

        const words = await provider.transcribe({
          audioBytes,
          mimeType: parsedMessage.data.mimeType,
        })

        const normalizedWords = words.trim()
        if (!normalizedWords.length) {
          return
        }

        websocket.send(JSON.stringify({
          type: 'words',
          words: normalizedWords,
        }))
      })
      .catch((error) => {
        console.error('Voice stream failed while transcribing chunk', error)
        websocket.send(JSON.stringify({
          type: 'error',
          message: 'Voice transcription failed for one or more chunks',
        }))
      })
  })
}
