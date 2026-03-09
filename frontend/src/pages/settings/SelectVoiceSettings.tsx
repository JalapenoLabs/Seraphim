// Copyright © 2026 Jalapeno Labs

// Core
import { LlmType, VoiceProvider } from '@prisma/client'
import { useSelector } from '@frontend/framework/store'

// UI
import { SearchLlmAccounts } from '@frontend/elements/SearchLlmAccounts'
import { VoiceProviderInput } from '@frontend/elements/VoiceProviderInput'

type Props = {
  selectedVoiceType: VoiceProvider | null
  onVoiceTypeChange: (voiceProvider: VoiceProvider) => void
  voiceTypeError?: string

  selectedLlmId: string | null
  onLlmIdChange: (llmId: string | null) => void
  llmIdError?: string
}

export function SelectVoiceSettings(props: Props) {
  const llms = useSelector((state) => state.llms.items)
  const hasOpenAiApiKeyLlm = llms.some((llm) => llm.type === LlmType.OPENAI_API_KEY)
  const llmIdError = !hasOpenAiApiKeyLlm
    ? 'Create an OpenAI API Key LLM account to enable OpenAI voice transcription.'
    : props.llmIdError

  return <div className='compact flex gap-3'>
    <VoiceProviderInput
      className='w-full'
      value={props.selectedVoiceType}
      onChange={(voiceProvider) => props.onVoiceTypeChange(voiceProvider)}
      errorMessage={props.voiceTypeError}
    />
    { props.selectedVoiceType === VoiceProvider.OPENAI_API_KEY
      ? <SearchLlmAccounts
        className='w-full'
        selectedLlmId={props.selectedLlmId}
        allowedLlmTypes={[ LlmType.OPENAI_API_KEY ]}
        selectionStorageKey='voice-openai-llm-selection'
        onSelectionChange={(llm) => props.onLlmIdChange(llm.id)}
        errorMessage={llmIdError}
      />
      : <></>
    }
  </div>
}
