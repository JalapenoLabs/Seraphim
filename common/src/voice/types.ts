// Copyright © 2026 Jalapeno Labs

import type { Llm, VoiceProvider } from '@prisma/client'

export type VoiceTranscriptionInput = {
  audioBytes: Uint8Array<ArrayBuffer>
  mimeType: string
}

export type VoiceProviderContext = {
  provider: VoiceProvider
  llm: Llm
}
