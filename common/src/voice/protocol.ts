// Copyright © 2026 Jalapeno Labs

import { z } from 'zod'

export const voiceChunkMessageSchema = z
  .object({
    type: z.literal('audio-chunk'),
    mimeType: z.string().trim().min(1),
    dataBase64: z.string().trim().min(1),
  })
  .strict()

export const voiceClientMessageSchema = z
  .discriminatedUnion('type', [
    voiceChunkMessageSchema,
  ])

export type VoiceClientMessage = z.infer<typeof voiceClientMessageSchema>

export type VoiceServerMessage =
  | {
    type: 'ready'
  }
  | {
    type: 'words'
    words: string
  }
  | {
    type: 'error'
    message: string
  }
