// Copyright © 2026 Jalapeno Labs

import type { Message } from '@common/types'

// Core
import { useEffect, useState } from 'react'

// Utility
import { safeParseJson } from '@common/json'

// Misc
import { getTaskMessagesSseUrl } from '@frontend/routes/taskRoutes'

export function useTaskMessages(taskId: string) {
  const [ messages, setMessages ] = useState<Message[]>([])

  useEffect(function manageTaskMessagesSseConnection() {
    if (!taskId?.trim()) {
      console.debug('useTaskMessages received empty taskId, skipping SSE connection', {
        taskId,
      })
      setMessages([])
      return
    }

    const taskMessagesUrl = getTaskMessagesSseUrl(taskId)
    const eventSource = new EventSource(taskMessagesUrl)

    function handleMessage(event: MessageEvent) {
      const payload = safeParseJson<unknown>(event.data)
      if (!Array.isArray(payload)) {
        console.debug('Task messages SSE payload is not an array', {
          taskId,
          payload,
        })
        return
      }

      setMessages(payload)
    }

    function handleError(event: Event) {
      console.debug('Task messages SSE error received', {
        taskId,
        event,
        readyState: eventSource.readyState,
      })
    }

    eventSource.addEventListener('message', handleMessage)
    eventSource.addEventListener('error', handleError)

    return function cleanupTaskMessagesSseConnection() {
      eventSource.removeEventListener('message', handleMessage)
      eventSource.removeEventListener('error', handleError)
      eventSource.close()
    }
  }, [ taskId ])

  return {
    messages,
  }
}
