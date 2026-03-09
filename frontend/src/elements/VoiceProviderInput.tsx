// Copyright © 2026 Jalapeno Labs

import type { VoiceProvider } from '@prisma/client'
import type { Selection } from '@react-types/shared'

// User interface
import { Select, SelectItem } from '@heroui/react'
import { MdKeyboardVoice } from 'react-icons/md'

// Misc
import { USER_VOICE_PROVIDER_OPTIONS } from '@common/constants'

type Props = {
  value: VoiceProvider
  onChange: (value: VoiceProvider) => void
  className?: string
  description?: string
  errorMessage?: string
  isDisabled?: boolean
  label?: string
}

const voiceProviderLabels = {
  NONE: 'None (disabled)',
  NATIVE_CHROMIUM: 'Chrome voice to text (Basic accuracy, no setup required)',
  LOCAL_VOSK: 'Local Vosk server (Low accuracy)',
  OPENAI_API_KEY: 'OpenAI voice to text (Best accuracy, requires OpenAI API key)',
} as const satisfies Record<VoiceProvider, string>

const VoiceProviderSet = new Set(USER_VOICE_PROVIDER_OPTIONS)

export function VoiceProviderInput(voiceProviderInputProps: Props) {
  const {
    value,
    onChange,
    className,
    description,
    errorMessage,
    isDisabled,
    label = 'Voice provider',
  } = voiceProviderInputProps

  return <Select
    id='voice-provider'
    className={className}
    description={description}
    errorMessage={errorMessage}
    isDisabled={isDisabled}
    isInvalid={Boolean(errorMessage)}
    label={label}
    startContent={<MdKeyboardVoice className='opacity-60' />}
    selectedKeys={[ value ]}
    disallowEmptySelection
    onSelectionChange={(selection: Selection) => {
    if (selection === 'all') {
      console.debug('VoiceProviderInput received an invalid "all" selection.')
      return
    }

    const selectedKeys = Array.from(selection)
    const [ selectedKey ] = selectedKeys

    if (!VoiceProviderSet.has(selectedKey as VoiceProvider)) {
      console.debug('VoiceProviderInput received an unknown voice provider option.', { selectedKey })
      return
    }

    onChange(selectedKey as VoiceProvider)
  }}
  >{
      USER_VOICE_PROVIDER_OPTIONS.map((voiceProvider) => (
        <SelectItem key={voiceProvider}>{
          voiceProviderLabels[voiceProvider]
        }</SelectItem>
      ))
    }</Select>
}
