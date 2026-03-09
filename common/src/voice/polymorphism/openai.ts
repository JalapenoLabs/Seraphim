// Copyright © 2026 Jalapeno Labs

import type { Llm } from '@prisma/client'
import type { VoiceTranscriptionInput } from '../types'

// Lib
import OpenAI from 'openai'

// Utility
import { BaseVoiceProvider } from './base'

const FallbackModel = 'gpt-4o-mini-transcribe' as const

export class OpenAiVoiceProvider extends BaseVoiceProvider {
  private readonly llm: Llm
  private readonly client: OpenAI

  constructor(llm: Llm) {
    super()

    this.llm = llm
    this.client = new OpenAI({ apiKey: llm.apiKey })
  }

  public async transcribe(input: VoiceTranscriptionInput): Promise<string> {
    const file = new File([
      input.audioBytes,
    ], `voice-${Date.now()}.webm`, { type: input.mimeType })

    const response = await this.client.audio.transcriptions.create({
      file,
      model: this.llm.preferredModel || FallbackModel,
    })

    return response.text || ''
  }
}
