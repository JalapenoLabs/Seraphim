// Copyright © 2026 Jalapeno Labs

import type { Request, Response } from 'express'
import type { Message, TurnWithMessages } from '@common/types'
import type { SetOptional } from 'type-fest'

// Core
import { requireDatabaseClient } from '@electron/database'
import { formatSseEvent, initializeSseResponse } from '@electron/api/sse/sseManager'
import { v7 as uuid } from 'uuid'
import { z } from 'zod'

type RouteParams = {
  taskId: string
}

const taskParamsSchema = z.object({
  taskId: z.string().trim().min(1),
})

type EventPayloadMap = {
  'turns': TurnWithMessages[]
  'new-message': Message
  'upsert-turn': TurnWithMessages
  'task-error': { message: string }
}
type Events = keyof EventPayloadMap
type EventData<Event extends Events> = EventPayloadMap[Event]
type Writer = <Event extends Events>(event: Event, data: EventData<Event>) => void
type WriterDef = {
  id: string
  writer: Writer
}

const writersByTaskId: Record<string, WriterDef[]> = {}

export async function handleStreamTaskMessagesRequest(
  request: Request<RouteParams>,
  response: Response,
): Promise<void> {
  const prisma = requireDatabaseClient('Stream task messages SSE')
  const parsedTaskParams = taskParamsSchema.safeParse(request.params)

  initializeSseResponse(response)

  function write<Event extends Events>(
    event: Event,
    data: EventData<Event>,
  ) {
    if (response.writableEnded) {
      console.debug('Attempted to write to task messages SSE after response ended', {
        event,
        data,
      })
      return
    }

    response.write(
      formatSseEvent(event,
        JSON.stringify(data),
      ),
    )
  }

  if (!parsedTaskParams.success) {
    console.debug('Task message stream requested with invalid task id', {
      taskId: request.params.taskId,
      issues: parsedTaskParams.error.issues,
    })
    write('task-error', {
      message: 'Task ID is required',
    })
    response.end()
    return
  }

  const { taskId } = parsedTaskParams.data

  try {
    const task = await prisma.task.findUnique({
      where: { id: taskId },
    })

    if (!task) {
      console.debug('Task message stream requested for missing task', { taskId })
      write('task-error', {
        message: 'Task not found',
      })
      response.end()
      return
    }

    const sessionId = uuid()
    const allTurns = await prisma.turn.findMany({
      where: {
        taskId,
      },
      include: {
        messages: {
          orderBy: {
            createdAt: 'asc',
          },
        },
      },
      orderBy: {
        createdAt: 'asc',
      },
    })

    write('turns', allTurns)

    if (writersByTaskId[taskId]) {
      writersByTaskId[taskId].push({
        id: sessionId,
        writer: write,
      })
    }
    else {
      writersByTaskId[taskId] = [{
        id: sessionId,
        writer: write,
      }]
    }

    function handleClose() {
      writersByTaskId[taskId] = writersByTaskId[taskId]?.filter((writer) => writer.id !== sessionId)

      if (!writersByTaskId[taskId]?.length) {
        delete writersByTaskId[taskId]
      }
    }

    request.once('close', handleClose)
  }
  catch (error) {
    console.debug('Task message stream query failed', {
      taskId,
      error,
    })

    if (response.headersSent || response.writableEnded) {
      return
    }

    write('task-error', {
      message: 'Something went wrong trying to stream task messages',
    })
    response.end()
  }
}

export async function broadcastTurnUpsert(
  taskId: string,
  turn: SetOptional<TurnWithMessages, 'messages'>,
) {
  const writers = writersByTaskId[taskId]
  if (!writers) {
    return
  }

  const prisma = requireDatabaseClient('Stream task messages SSE - broadcastTurnUpsert')

  try {
    const fullTurnWithMessages = await prisma.turn.findUnique({
      where: {
        id: turn.id,
      },
      include: {
        messages: {
          orderBy: {
            createdAt: 'asc',
          },
        },
      },
    })

    if (!fullTurnWithMessages) {
      console.debug('Failed to find turn for upsert broadcast', {
        turnId: turn.id,
      })
      return
    }

    for (const { writer } of writers) {
      writer('upsert-turn', fullTurnWithMessages)
    }
  }
  catch (error) {
    console.error('Failed to load turn for upsert broadcast', {
      taskId,
      turnId: turn.id,
      error,
    })
  }
}

export async function broadcastMessageUpsert(
  taskId: string,
  message: Message,
) {
  const writers = writersByTaskId[taskId]
  if (!writers) {
    return
  }

  for (const { writer } of writers) {
    writer('new-message', message)
  }
}
