// Copyright © 2026 Jalapeno Labs

import type { Message, TurnWithMessages } from '@common/types'

// Core
import { useEffect, useState } from 'react'
import { safeParseJson } from '@common/json'
import { getTaskMessagesSseUrl } from '@frontend/routes/taskRoutes'

export function useTaskMessages(taskId: string) {
  const [ turns, setTurns ] = useState<TurnWithMessages[]>([])

  useEffect(() => {
    if (!taskId?.trim()) {
      console.debug('useTaskMessages received empty taskId, skipping SSE connection', {
        taskId,
      })
      if (turns.length) {
        setTurns([])
      }
      return null
    }

    const eventSource = new EventSource(
      getTaskMessagesSseUrl(taskId),
    )

    function handleInitialTurns(event: MessageEvent) {
      const data = safeParseJson<TurnWithMessages[]>(event.data)
      setTurns(data)
    }

    function handleNewMessage(event: MessageEvent) {
      const newMessage = safeParseJson<Message>(event.data)

      if (!newMessage?.id) {
        console.debug('Received new message SSE with invalid data', event)
        return
      }

      // Needs to handle:
      // - Create message on existing turn
      // - Create message on new turn where we receive the message before the turn due to race conditions
      setTurns((previousTurns) => {
        const existingTurn = previousTurns.find((turn) => turn.id === newMessage.turnId)

        // We can guess the turn if there's a race condition...
        if (!existingTurn) {
          console.debug('Received new message SSE for missing turn, creating new in-memory turn', {
            turnId: newMessage.turnId,
            messageId: newMessage.id,
          })

          return [
            ...previousTurns,
            {
              id: newMessage.turnId,
              taskId,
              createdAt: Date.now(),
              timeTaken: null,
              finishedAt: null,
              messages: [ newMessage ],
            },
          ] as TurnWithMessages[]
        }

        // Otherwise, we know exactly which turn to put the message in
        return previousTurns
          .map((turn) => {
            if (turn.id !== newMessage.turnId) {
              return turn
            }

            return {
              ...turn,
              messages: [ ...turn.messages, newMessage ],
            }
          })
      })
    }

    function handleNewTurn(event: MessageEvent) {
      const newTurn = safeParseJson<TurnWithMessages>(event.data)

      if (!newTurn?.id) {
        console.debug('Received new turn SSE with invalid data', event)
        return
      }

      // Needs to handle:
      // - Turn Creations
      // - Turn Updates
      // - Message race conditions where we receive a message with a new turn id before the new turn is received
      setTurns((previousTurns) => {
        const existingTurn = previousTurns.find((turn) => turn.id === newTurn.id)
        if (!existingTurn) {
          return [ ...previousTurns, newTurn ]
        }

        console.debug('Received new turn SSE for existing turn, merging messages', {
          turnId: newTurn.id,
          previousMessagesCount: existingTurn.messages.length,
          newMessagesCount: newTurn.messages.length,
        })

        return previousTurns
          .map((turn) => {
            if (turn.id !== newTurn.id) {
              return turn
            }

            const mergedMessages = [
              ...turn.messages || [],
              ...newTurn.messages || [],
            ]

            return {
              ...newTurn,
              messages: mergedMessages
                .filter((message, index) =>
                  index === mergedMessages.findIndex((m) => m.id === message.id),
                )
                .sort((a, b) => new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime()),
            }
          })
      })
    }

    function handleError(event: Event) {
      console.debug('Task messages SSE error received', {
        taskId,
        event,
        readyState: eventSource.readyState,
      })
    }

    eventSource.addEventListener('turns', handleInitialTurns)
    eventSource.addEventListener('new-message', handleNewMessage)
    eventSource.addEventListener('upsert-turn', handleNewTurn)
    eventSource.addEventListener('task-error', handleError)
    eventSource.addEventListener('error', handleError)

    return () => {
      eventSource.removeEventListener('turns', handleInitialTurns)
      eventSource.removeEventListener('new-message', handleNewMessage)
      eventSource.removeEventListener('upsert-turns', handleNewTurn)
      eventSource.removeEventListener('task-error', handleError)
      eventSource.removeEventListener('error', handleError)
      eventSource.close()
    }
  }, [ taskId ])

  return {
    turns,
  } as const
}
