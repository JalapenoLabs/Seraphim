// Copyright © 2026 Jalapeno Labs

// Lib
import { z } from 'zod'

// Utility
import { VoiceProvider } from '@prisma/client'

// Misc
import {
  DONE_SOUND_MIME_TYPES,
  USER_LANGUAGE_OPTIONS,
  USER_THEME_OPTIONS,
  USER_VOICE_PROVIDER_OPTIONS,
} from '../constants'

export const userLanguageSchema = z.enum(USER_LANGUAGE_OPTIONS)
export const userThemeSchema = z.enum(USER_THEME_OPTIONS)
export const userVoiceProviderSchema = z.enum(USER_VOICE_PROVIDER_OPTIONS)

const userSettingsBaseSchema = z
  .object({
    language: userLanguageSchema,
    theme: userThemeSchema,
    codeEditor: z.string().trim().optional(),
    voiceEnabled: z.boolean(),
    voiceHotkey: z.string().trim().min(1),
    voiceProvider: userVoiceProviderSchema,
    voiceLlmId: z.string().uuid().nullable().optional(),
    doneSoundAudioFileId: z.string().uuid().nullable().optional(),
    customAgentInstructions: z.string().optional().default(''),
    customAgentsFile: z.string().nullable().optional(),
  })

function validateOpenAiVoiceSelection(
  settings: { voiceProvider?: VoiceProvider, voiceLlmId?: string | null },
  context: z.RefinementCtx,
) {
  if (
    settings.voiceProvider === VoiceProvider.OPENAI_API_KEY
    && !settings.voiceLlmId
  ) {
    context.addIssue({
      code: z.ZodIssueCode.custom,
      path: [ 'voiceLlmId' ],
      message: 'An OpenAI API Key LLM is required when OpenAI voice is selected',
    })
  }
}

export const userSettingsSchema = userSettingsBaseSchema
  .superRefine(validateOpenAiVoiceSelection)
export type UserSettingsRequest = z.infer<typeof userSettingsSchema>

const doneSoundFileSchema = z
  .object({
    name: z.string().trim().min(1),
    mimeType: z.enum(DONE_SOUND_MIME_TYPES),
    sizeBytes: z.number().int().positive(),
    dataBase64: z.string().trim().min(1),
  })
  .strict()

export const userSettingsUpdateFieldsSchema = userSettingsBaseSchema.partial()

export const userSettingsUpdateSchema = userSettingsUpdateFieldsSchema
  .extend({
    doneSoundFile: doneSoundFileSchema.nullable().optional(),
  })
  .strict()
  .superRefine(validateOpenAiVoiceSelection)
  .refine(
    (data: Record<string, unknown>) => Object.keys(data).length > 0,
    { message: 'No valid fields provided for update' },
  )

export type UserSettingsUpdateRequest = z.infer<typeof userSettingsUpdateSchema>
