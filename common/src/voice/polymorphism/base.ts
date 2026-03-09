// Copyright © 2026 Jalapeno Labs

import type { VoiceTranscriptionInput } from '../types'

export abstract class BaseVoiceProvider {
  public abstract transcribe(input: VoiceTranscriptionInput): Promise<string>
}
