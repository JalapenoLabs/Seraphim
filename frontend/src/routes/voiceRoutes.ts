// Copyright © 2026 Jalapeno Labs

// Core
import { ApiRoot } from '@common/api'

const VoiceSocketPath = '/api/v1/protected/voice/stream' as const

export function getVoiceSocketUrl(): string {
  const httpUrl = new URL(ApiRoot)
  httpUrl.protocol = httpUrl.protocol === 'https:'
    ? 'wss:'
    : 'ws:'

  return `${httpUrl.origin}${VoiceSocketPath}`
}
