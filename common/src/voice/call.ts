// Copyright © 2026 Jalapeno Labs

import type { BaseVoiceProvider } from './polymorphism/base'
import type { VoiceProviderContext } from './types'

// Utility
import { VoiceProvider } from '@prisma/client'

// Voice providers
import { OpenAiVoiceProvider } from './polymorphism/openai'

type VoiceProviderBuilder = (context: VoiceProviderContext) => BaseVoiceProvider

const providerBuilderByType: Partial<Record<VoiceProvider, VoiceProviderBuilder>> = {
  [VoiceProvider.OPENAI_API_KEY]: ({ llm }) => new OpenAiVoiceProvider(llm),
}

export function getVoiceProvider(context: VoiceProviderContext): BaseVoiceProvider {
  const providerBuilder = providerBuilderByType[context.provider]
  if (!providerBuilder) {
    throw new Error(`Unsupported voice provider: ${context.provider}`)
  }

  return providerBuilder(context)
}
