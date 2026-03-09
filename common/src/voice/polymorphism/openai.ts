// Copyright © 2026 Jalapeno Labs

import type { Llm } from '@prisma/client'
import type { VoiceTranscriptionInput } from '../types'

// Utility
import { BaseVoiceProvider } from './base'

const TranscriptionModel = 'gpt-4o-mini-transcribe' as const
const OpenAiTranscriptionUrl = 'https://api.openai.com/v1/audio/transcriptions' as const

export class OpenAiVoiceProvider extends BaseVoiceProvider {
  private readonly llm: Llm

  constructor(llm: Llm) {
    super()

    this.llm = llm
  }

  public async transcribe(input: VoiceTranscriptionInput): Promise<string> {
    const formData = new FormData()
    const audioFile = new File(
      [ input.audioBytes ],
      `voice-${Date.now()}.webm`,
      { type: input.mimeType || 'audio/webm' },
    )

    formData.append('model', TranscriptionModel)
    formData.append('file', audioFile)

    const response = await fetch(OpenAiTranscriptionUrl, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${this.llm.apiKey}`,
      },
      body: formData,
    })

    if (!response.ok) {
      const errorPayload = await response.text()
      console.error('OpenAiVoiceProvider transcription request failed', {
        status: response.status,
        statusText: response.statusText,
        errorPayload,
      })
      throw new Error(`OpenAiVoiceProvider transcription failed with status ${response.status}`)
    }

    const payload = await response.json() as {
      text?: string
    }

    if (!payload.text?.trim()) {
      console.debug('OpenAiVoiceProvider received empty transcription text.', {
        payload,
      })
      return ''
    }

    return payload.text
  }
}
