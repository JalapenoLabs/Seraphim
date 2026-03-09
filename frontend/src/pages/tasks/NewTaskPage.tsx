// Copyright © 2026 Jalapeno Labs

import type { TaskCreateRequest } from '@common/schema/task'
import type { MonacoContext } from '@frontend/framework/monaco'

// Core
import { useCallback, useEffect, useRef } from 'react'
import { useForm } from 'react-hook-form'
import { useNavigate } from 'react-router'
import { useVoice } from '@frontend/hooks/useVoice'
import { getViewTaskUrl } from '@common/urls'

// Api
import { createTask } from '@frontend/routes/taskRoutes'

// UI
import { Button, Tooltip } from '@heroui/react'
import { Monaco } from '@frontend/elements/Monaco'
import { Card } from '@frontend/elements/Card'
import { SearchWorkspaces } from '@frontend/elements/SearchWorkspaces'
import { SearchGitBranches } from '@frontend/elements/SearchGitBranches'
import { SearchAuthAccounts } from '@frontend/elements/SearchAuthAccounts'
import { SearchLlmAccounts } from '@frontend/elements/SearchLlmAccounts'
import { SearchIssueTrackers } from '@frontend/elements/SearchIssueTrackers'
import { SearchIssueLinks } from '@frontend/elements/SearchIssueLinks'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { taskCreateSchema } from '@common/schema/task'
import { VoiceButton } from '@frontend/elements/VoiceButton'

const resolvedForm = zodResolver(taskCreateSchema)
const formMode = {
  shouldDirty: true,
  shouldValidate: true,
  shouldTouch: true,
}

const localStorageKey = 'new-task-default-prompt'

export function NewTaskPage() {
  const navigate = useNavigate()

  const voice = useVoice()
  const monaco = useRef<MonacoContext | null>(null)
  const lockedVoiceRangeRef = useRef<{
    startOffset: number,
    currentLength: number,
    prefix: string,
  } | null>(null)

  const form = useForm<TaskCreateRequest>({
    resolver: resolvedForm,
    defaultValues: {
      workspaceId: '',
      gitAccountId: '',
      llmId: '',
      issueTrackingId: '',
      message: localStorage.getItem(localStorageKey) || '',
      branch: '',
      issueLink: '',
      archived: false,
    },
    mode: 'all',
  })

  useEffect(() => {
    if (!voice.isActive || !voice.words) {
      return
    }

    if (!monaco.current) {
      console.warn('Monaco editor is not ready yet, cannot set voice transcription words')
      return
    }

    const editor = monaco.current.editor
    const model = editor.getModel()
    if (!model) {
      console.warn('Monaco editor model is not ready yet, cannot set voice transcription words')
      return
    }

    if (!lockedVoiceRangeRef.current) {
      const selection = editor.getSelection()
      if (!selection) {
        console.warn('Monaco editor selection is not ready yet, cannot set voice transcription words')
        return
      }

      const isCursorOnly = selection.isEmpty()
      let prefix = ''
      if (isCursorOnly) {
        const position = selection.getStartPosition()
        const line = model.getLineContent(position.lineNumber)
        const charBefore = position.column > 1 ? line[position.column - 2] : ''
        if (charBefore && !/\s/.test(charBefore)) {
          prefix = ' '
        }
      }

      lockedVoiceRangeRef.current = {
        startOffset: model.getOffsetAt(selection.getStartPosition()),
        currentLength: model.getOffsetAt(selection.getEndPosition()) - model.getOffsetAt(selection.getStartPosition()),
        prefix,
      }
    }

    const lockedVoiceRange = lockedVoiceRangeRef.current
    if (!lockedVoiceRange) {
      console.warn('Voice range is not available while voice transcription is active')
      return
    }

    const textToApply = lockedVoiceRange.prefix + voice.words
    const startPosition = model.getPositionAt(lockedVoiceRange.startOffset)
    const endPosition = model.getPositionAt(lockedVoiceRange.startOffset + lockedVoiceRange.currentLength)

    editor.executeEdits('voice-transcription', [
      {
        range: new monaco.current.monaco.Range(
          startPosition.lineNumber,
          startPosition.column,
          endPosition.lineNumber,
          endPosition.column,
        ),
        text: textToApply,
        forceMoveMarkers: true,
      },
    ])

    lockedVoiceRange.currentLength = textToApply.length

    const nextPosition = model.getPositionAt(lockedVoiceRange.startOffset + lockedVoiceRange.currentLength)
    editor.setSelection(
      new monaco.current.monaco.Selection(
        nextPosition.lineNumber,
        nextPosition.column,
        nextPosition.lineNumber,
        nextPosition.column,
      ),
    )

    editor.focus()
  }, [ voice.words, voice.isActive ])

  useEffect(() => {
    if (voice.isActive) {
      return
    }

    lockedVoiceRangeRef.current = null
  }, [ voice.isActive ])

  useEffect(() => {
    form.trigger()

    const subscription = form.watch(async (_, info) => {
      if (!info.name) {
        return
      }

      await form.trigger(info.name)
    })

    return () => {
      subscription.unsubscribe()
    }
  }, [ form ])

  useEffect(() => {
    localStorage.setItem(
      localStorageKey,
      form.watch('message') || '',
    )
  }, [ form.watch('message') ])

  const submit = useCallback(async () => {
    await form.handleSubmit(
      async (values) => {
        const createdTask = await createTask(values)

        if (!createdTask?.task?.id) {
          console.error('Failed to create task, no task id returned', { createdTask })
          return
        }

        localStorage.removeItem(localStorageKey)

        navigate(
          getViewTaskUrl(createdTask.task.id),
        )
      },
    )()
  }, [ form.formState.isDirty ])

  const isDisabled = form.formState.isLoading || form.formState.isSubmitting

  return <section className='level w-full items-start h-[90vh]'>
    <article className='relaxed w-full'>
      <Monaco
        autoFocus
        height='90vh'
        minimapOverride
        readOnly={isDisabled}
        fileLanguage='markdown'
        value={form.watch('message')}
        onChange={(value) => form.setValue('message', value, formMode)}
        getMonaco={(context) => monaco.current = context}
      />
    </article>
    <article className='flex flex-col items-stretch min-w-lg max-w-lg w-full h-full'>
      <Card className='relaxed' label='Authentication'>
        <SearchAuthAccounts
          className='relaxed'
          isDisabled={isDisabled}
          onSelectionChange={(value) => form.setValue('gitAccountId', value.id, formMode)}
          // value={form.watch('gitAccountId')}
        />
        <SearchLlmAccounts
          className='relaxed'
          isDisabled={isDisabled}
          onSelectionChange={(value) => form.setValue('llmId', value.id, formMode)}
          // value={form.watch('llmId')}
        />
      </Card>
      <Card className='relaxed' label='Workspace'>
        <SearchWorkspaces
          className='relaxed'
          isDisabled={isDisabled}
          onSelectionChange={(value) => form.setValue('workspaceId', value.id, formMode)}
          // value={form.watch('workspaceId')}
        />
        <SearchGitBranches
          className='relaxed'
          isDisabled={isDisabled}
          workspaceId={form.watch('workspaceId')}
          gitAccountId={form.watch('gitAccountId')}
          onSelectionChange={(value) => form.setValue('branch', value, formMode)}
          // value={form.watch('branch')}
        />
      </Card>
      <Card className='relaxed' label='Issue (optional)'>
        <SearchIssueTrackers
          className='relaxed'
          isDisabled={isDisabled}
          onSelectionChange={(value) => form.setValue('issueTrackingId', value.id, formMode)}
          // value={form.watch('issueTrackingId')}
        />
        <SearchIssueLinks
          className='relaxed'
          issueTrackingId={form.watch('issueTrackingId')}
          isDisabled={isDisabled}
          onSelection={(value) => form.setValue('issueLink', value, formMode)}
          // value={form.watch('issueLink')}
        />
      </Card>

      <div className='flex-1' />

      <div>
        <VoiceButton
          size='lg'
          voice={voice}
          className='compact w-full'
        >
          <span>
            <strong>Voice to text</strong>
          </span>
        </VoiceButton>
        <Tooltip
          content={
            // @ts-ignore
            form.formState.errors['']
            || form.formState.errors?.message?.message
            || form.formState.errors?.workspaceId?.message
            || form.formState.errors?.gitAccountId?.message
            || form.formState.errors?.llmId?.message
            || form.formState.errors?.branch?.message
            || form.formState.errors?.issueLink?.message
            || 'Click to begin the task'
          }
        >
          <div>
            <Button
              id='start-task'
              fullWidth
              size='lg'
              color='primary'
              className='button'
              isLoading={form.formState.isLoading || form.formState.isSubmitting}
              isDisabled={!form.formState.isValid}
              onPress={submit}
            >
              <span>
                <strong>Begin Task</strong>
              </span>
            </Button>
          </div>
        </Tooltip>
      </div>
    </article>
  </section>
}
