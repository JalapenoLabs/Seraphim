// Copyright © 2026 Jalapeno Labs

import type { UserSettings } from '@common/types'

// Core
import { AudioFor } from '@prisma/client'
import { useEffect } from 'react'
import { useSelector } from '@frontend/framework/store'
import { useWatchUnsavedWork } from '@frontend/hooks/useWatchUnsavedWork'

// Form
import { useForm } from 'react-hook-form'
import { userSettingsUpdateFieldsSchema } from '@common/schema/userSettings'
import { zodResolver } from '@hookform/resolvers/zod'
import { updateCurrentUserSettings } from '@frontend/routes/userRoutes'

// UI
import { Switch } from '@heroui/react'
import { Card } from '@frontend/elements/Card'
import { ThemeInput } from '@frontend/elements/ThemeInput'
import { LanguageInput } from '@frontend/elements/LanguageInput'
import { DisplayErrors } from '@frontend/elements/buttons/DisplayErrors'
import { SaveButton } from '@frontend/elements/buttons/SaveButton'
import { ResetButton } from '@frontend/elements/buttons/ResetButton'
import { AudioFileUploadButton } from '@frontend/elements/buttons/AudioFileUploadButton'
import { SelectVoiceSettings } from './SelectVoiceSettings'

const resolvedForm = zodResolver(userSettingsUpdateFieldsSchema)

export function GeneralSettingsPage() {
  // Inputs:
  const currentSettings = useSelector((state) => state.settings.current)

  // State:
  const form = useForm<UserSettings>({
    resolver: resolvedForm,
    defaultValues: currentSettings,
    mode: 'onSubmit',
  })

  useEffect(() => {
    form.reset(currentSettings)
  }, [ currentSettings ])

  // Actions:
  const onSave = form.handleSubmit(
    (values) => updateCurrentUserSettings(values),
  )

  useWatchUnsavedWork(form.formState.isDirty, {
    onSave: () => onSave(),
  })

  return <article>
    <header className='compact level'>
      <h1 className='text-2xl font-bold'>General settings</h1>
      <div className='level-right'>
        <ResetButton
          onReset={() => form.reset(currentSettings)}
          isDirty={form.formState.isDirty}
          isDisabled={form.formState.isSubmitting}
        />
        <SaveButton
          onSave={onSave}
          isDirty={form.formState.isDirty}
          isDisabled={form.formState.isSubmitting}
        />
      </div>
    </header>
    <DisplayErrors
      // @ts-ignore
      errors={form.formState.errors['']?.message}
      className='relaxed'
    />
    <Card className='relaxed'>
      <div className='level-centered'>
        <ThemeInput
          className='w-full'
          value={form.watch('theme')}
          onChange={(value) => {
            form.setValue('theme', value, { shouldDirty: true })
          }}
          errorMessage={form.formState.errors.theme?.message}
        />
        <LanguageInput
          className='w-full'
          value={form.watch('language')}
          onChange={(value) => {
            form.setValue('language', value, { shouldDirty: true })
          }}
          errorMessage={form.formState.errors.language?.message}
        />
      </div>
    </Card>
    <Card className='relaxed' label='Voice Settings'>
      <SelectVoiceSettings
        selectedVoiceType={form.watch('voiceProvider')}
        voiceTypeError={form.formState.errors.voiceProvider?.message}
        onVoiceTypeChange={(voiceProvider) => {
          form.setValue('voiceProvider', voiceProvider, {
            shouldDirty: true,
            shouldValidate: true,
          })

          if (voiceProvider !== 'OPENAI_API_KEY') {
            form.setValue('voiceLlmId', null, { shouldDirty: true, shouldValidate: true })
          }
        }}
        selectedLlmId={form.watch('voiceLlmId') || ''}
        llmIdError={form.formState.errors.voiceLlmId?.message}
        onLlmIdChange={(llmId) => {
          form.setValue('voiceLlmId', llmId, {
            shouldDirty: true,
            shouldValidate: true,
          })
        }}
      />
    </Card>
    <Card className='relaxed' label='Audio Settings'>
      <div className='relaxed'>

        <Switch
          isSelected={form.watch('playSounds')}
          onValueChange={(checked: boolean) => {
            form.setValue('playSounds', checked, { shouldDirty: true })
          }}
        >
          <span>Enable sound playback</span>
        </Switch>
      </div>

      <div className='relaxed'>
        <h3 className='mb-1'>Customize sounds that play</h3>
        <AudioFileUploadButton
          audioFor={AudioFor.DONE_SOUND}
          label='Done Sound'
        />
      </div>
    </Card>
  </article>
}
