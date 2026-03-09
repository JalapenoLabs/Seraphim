// Copyright © 2026 Jalapeno Labs

// Lib
import { AudioFor } from '@prisma/client'
import { z } from 'zod'

// Misc
import { DONE_SOUND_MIME_TYPES } from '../constants'

export const audioForSchema = z.nativeEnum(AudioFor)

export const userAudioFileSchema = z
  .object({
    name: z.string().trim().min(1),
    mimeType: z.enum(DONE_SOUND_MIME_TYPES),
    sizeBytes: z.number().int().positive(),
    dataBase64: z.string().trim().min(1),
  })
  .strict()

export const upsertUserAudioFileSchema = z
  .object({
    audioFor: audioForSchema,
    file: userAudioFileSchema,
  })
  .strict()

export type UpsertUserAudioFileRequest = z.infer<typeof upsertUserAudioFileSchema>
