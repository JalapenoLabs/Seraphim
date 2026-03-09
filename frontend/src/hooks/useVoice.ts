// Copyright © 2026 Jalapeno Labs

// Utility
import { VoiceProvider } from '@prisma/client'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useSelector } from '@frontend/framework/store'

// Misc
import { FrontendVoiceSession } from './lib/voiceSession'
import { getVoiceSocketUrl } from '@frontend/routes/voiceRoutes'
import { doToast } from '@frontend/framework/toast'

export function useVoice() {
  const settings = useSelector((state) => state.settings.current)

  const [ isActive, setIsActive ] = useState(false)
  const [ words, setWords ] = useState('')

  const sessionRef = useRef<FrontendVoiceSession | null>(null)

  const isDisabled = !settings
    || !settings.voiceProvider
    || settings.voiceProvider === VoiceProvider.NONE
    || settings.voiceProvider !== VoiceProvider.OPENAI_API_KEY
    || !settings.voiceLlmId

  const stop = useCallback(() => {
    sessionRef.current?.stop()
    sessionRef.current = null
    setIsActive(false)
  }, [])

  const start = useCallback(async () => {
    if (isDisabled || !settings) {
      doToast({
        title: 'Voice is not configured',
        description: 'Set your voice provider to OpenAI and choose an OpenAI API key LLM account first.',
        color: 'warning',
      })
      return
    }

    if (isActive) {
      return
    }

    setWords('')

    const voiceSession = new FrontendVoiceSession({
      websocketUrl: getVoiceSocketUrl(),
      onActiveChange: setIsActive,
      onWords: setWords,
      onError: (errorMessage) => {
        doToast({
          title: 'Voice transcription error',
          description: errorMessage,
          color: 'danger',
        })
      },
    })

    sessionRef.current = voiceSession

    try {
      console.log('Starting voice session with settings', {
        voiceProvider: settings.voiceProvider,
        voiceLlmId: settings.voiceLlmId,
      })
      await voiceSession.start()
    }
    catch (error) {
      console.error('useVoice failed to start voice streaming session', error)
      stop()
      doToast({
        title: 'Voice connection failed',
        description: 'Could not connect to local voice streaming API.',
        color: 'danger',
      })
    }
  }, [ isActive, isDisabled, settings, stop ])

  useEffect(() => {
    if (!isDisabled || !isActive) {
      return
    }

    stop()
  }, [ isActive, isDisabled, stop ])

  useEffect(() => {
    return () => {
      stop()
    }
  }, [ stop ])

  return {
    isTurnedOff: settings?.voiceProvider === VoiceProvider.NONE,
    isDisabled,
    isActive,
    words,
    start,
    stop,
  } as const
}
