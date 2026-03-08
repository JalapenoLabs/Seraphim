// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@common/types'

// Core
import { useCallback, useEffect } from 'react'
import { useHotkey } from '@frontend/hooks/useHotkey'

// Lib
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'

// User Interface
import { Input } from '@heroui/react'
import { Card } from '@frontend/elements/Card'
import { DisplayErrors } from '@frontend/elements/buttons/DisplayErrors'
import { SaveButton } from '@frontend/elements/buttons/SaveButton'
import { ResetButton } from '@frontend/elements/buttons/ResetButton'
import { CloseButton } from '@frontend/elements/buttons/CloseButton'

// Utility
import { useWatchUnsavedWork } from '@frontend/hooks/useWatchUnsavedWork'

// Misc
import { upsertIssueTrackingSchema } from '@common/schema/issueTracking'
import { upsertIssueTracking } from '@frontend/routes/issueTrackingRoutes'

type Props = {
  issueTracking?: IssueTracking
  provider: IssueTracking['provider']
  close?: () => void
}

const resolvedForm = zodResolver(upsertIssueTrackingSchema)

export function ViewIssueTrackingPage(props: Props) {
  const { issueTracking, provider } = props
  const isCreateMode = !issueTracking?.id

  const form = useForm<IssueTracking>({
    resolver: resolvedForm,
    defaultValues: {
      id: issueTracking?.id ?? '',
      userId: issueTracking?.userId ?? '',
      provider: issueTracking?.provider ?? provider,
      accessToken: '',
      baseUrl: issueTracking?.baseUrl ?? '',
      name: issueTracking?.name ?? '',
      email: issueTracking?.email ?? '',
      targetBoard: issueTracking?.targetBoard ?? '',
      lastUsedAt: issueTracking?.lastUsedAt ?? null,
      createdAt: issueTracking?.createdAt ?? new Date(0),
      updatedAt: issueTracking?.updatedAt ?? new Date(0),
    },
    mode: 'onSubmit',
  })

  useEffect(() => {
    console.debug('Resetting issue tracking form with inbound values')
    form.reset({
      id: issueTracking?.id ?? '',
      userId: issueTracking?.userId ?? '',
      provider: issueTracking?.provider ?? provider,
      accessToken: '',
      baseUrl: issueTracking?.baseUrl ?? '',
      name: issueTracking?.name ?? '',
      email: issueTracking?.email ?? '',
      targetBoard: issueTracking?.targetBoard ?? '',
      lastUsedAt: issueTracking?.lastUsedAt ?? null,
      createdAt: issueTracking?.createdAt ?? new Date(0),
      updatedAt: issueTracking?.updatedAt ?? new Date(0),
    })
  }, [ issueTracking, provider ])

  const onSave = useCallback(async () => {
    if (!form.formState.isDirty) {
      console.debug('ViewIssueTrackingPage save skipped because there are no changes')
      return false
    }

    return new Promise<boolean>(async (accept) => {
      await form.handleSubmit(
        async (values) => {
          await upsertIssueTracking(issueTracking?.id || '', {
            ...values,
            provider,
          })
          accept(true)
        },
        () => accept(false),
      )()
    })
  }, [ form.formState.isDirty, issueTracking, provider ])

  const onSaveAndCloseIfCreate = useCallback(async () => {
    const isSuccessful = await onSave()

    if (isSuccessful && isCreateMode && props.close) {
      props.close()
    }
  }, [ onSave, isCreateMode, props.close ])

  useWatchUnsavedWork(form.formState.isDirty, {
    onSave,
  })

  useHotkey([ 'Control', 's' ], onSave, {
    preventDefault: true,
    blockOtherHotkeys: true,
    enabled: !isCreateMode,
  })

  useHotkey([ 'Control', 'Enter' ], onSaveAndCloseIfCreate, {
    preventDefault: true,
    blockOtherHotkeys: true,
    enabled: isCreateMode,
  })

  const accountNamePlaceholder = provider === 'Github'
    ? 'Team GitHub'
    : 'Team Jira'
  const baseUrlPlaceholder = provider === 'Github'
    ? 'https://api.github.com'
    : 'https://company.atlassian.net'
  const accountIdentityLabel = provider === 'Github'
    ? 'Organization / owner'
    : 'Account email'
  const accountIdentityPlaceholder = provider === 'Github'
    ? 'jalapenolabs'
    : 'team@company.io'
  const targetBoardLabel = provider === 'Github'
    ? 'Repository'
    : 'Target board'
  const targetBoardPlaceholder = provider === 'Github'
    ? 'seraphim'
    : 'SER'
  const tokenPlaceholder = provider === 'Github'
    ? 'github_personal_access_token'
    : 'jira_api_token'

  return <Card className='w-full'>
    <header className='compact level'>
      <h1 className='text-2xl font-bold'>
        Issue Tracking
      </h1>
      { props.close
        ? <CloseButton onClose={props.close} />
        : <></>
      }
    </header>
    <DisplayErrors
      errors={form.formState.errors.root?.message}
      className='relaxed'
    />
    <div className='relaxed'>
      <div className='compact'>
        <Input
          fullWidth
          label='Account name'
          placeholder={accountNamePlaceholder}
          isInvalid={Boolean(form.formState.errors.name)}
          errorMessage={form.formState.errors.name?.message}
          value={form.watch('name')}
          onChange={(event) => {
            const value = event.currentTarget.value
            form.setValue('name', value, { shouldDirty: true })
          }}
        />
      </div>
      <div className='compact'>
        <Input
          fullWidth
          label='Base URL'
          placeholder={baseUrlPlaceholder}
          isInvalid={Boolean(form.formState.errors.baseUrl)}
          errorMessage={form.formState.errors.baseUrl?.message}
          value={form.watch('baseUrl')}
          onChange={(event) => {
            const value = event.currentTarget.value
            form.setValue('baseUrl', value, { shouldDirty: true })
          }}
        />
      </div>
      <div className='compact'>
        <Input
          fullWidth
          label={accountIdentityLabel}
          placeholder={accountIdentityPlaceholder}
          isInvalid={Boolean(form.formState.errors.email)}
          errorMessage={form.formState.errors.email?.message}
          value={form.watch('email')}
          onChange={(event) => {
            const value = event.currentTarget.value
            form.setValue('email', value, { shouldDirty: true })
          }}
        />
      </div>
      <div className='compact'>
        <Input
          fullWidth
          label={targetBoardLabel}
          placeholder={targetBoardPlaceholder}
          isInvalid={Boolean(form.formState.errors.targetBoard)}
          errorMessage={form.formState.errors.targetBoard?.message}
          value={form.watch('targetBoard')}
          onChange={(event) => {
            const value = event.currentTarget.value
            form.setValue('targetBoard', value, { shouldDirty: true })
          }}
        />
      </div>
      <div className='compact'>
        <Input
          label='Access token'
          placeholder={tokenPlaceholder}
          fullWidth
          type='password'
          autoComplete='off'
          isInvalid={Boolean(form.formState.errors.accessToken)}
          errorMessage={form.formState.errors.accessToken?.message}
          value={form.watch('accessToken')}
          onChange={(event) => {
            const value = event.currentTarget.value
            form.setValue('accessToken', value, { shouldDirty: true })
          }}
        />
        <p className='text-sm opacity-80'>(Leave blank for no change)</p>
      </div>
    </div>
    <div className='level-centered'>
      <ResetButton
        onReset={() => form.reset()}
        isDirty={form.formState.isDirty}
        isDisabled={form.formState.isSubmitting}
      />
      <SaveButton
        onSave={onSaveAndCloseIfCreate}
        isDirty={form.formState.isDirty}
        isLoading={form.formState.isSubmitting}
      />
    </div>
  </Card>
}
