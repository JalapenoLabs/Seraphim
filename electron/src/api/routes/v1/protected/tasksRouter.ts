// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleCreateTaskRequest } from './tasks/createTaskRoute'
import { handleDeleteTaskRequest } from './tasks/deleteTaskRoute'
import { handleGetTaskRequest } from './tasks/getTaskRoute'
import { handleListTasksRequest } from './tasks/listTasksRoute'
import { handleReUpTaskGitRequest } from './tasks/reUpTaskGitRoute'
import { handleRefreshTaskGitRequest } from './tasks/refreshTaskGitRoute'
import { handleInterruptTaskRequest } from './tasks/interruptTaskRoute'
import { handleTaskNewMessageRequest } from './tasks/taskNewMessageRoute'
import { handleTaskPullRequestRequest } from './tasks/taskPullRequestRoute'
import { handleViewTaskRepositoryRequest } from './tasks/viewTaskRepositoryRoute'
import { handleStreamTaskLogsRequest } from './tasks/streamTaskLogsRoute'
import { handleStreamTaskMessagesRequest } from './tasks/streamTaskMessagesRoute'
import { handleUpdateTaskRequest } from './tasks/updateTaskRoute'

export function createTasksRouter(): Router {
  const tasksRouter = createRouter()

  // /api/v1/protected/tasks
  tasksRouter.get('/', handleListTasksRequest)
  // /api/v1/protected/tasks/:taskId/logs/stream
  tasksRouter.get('/:taskId/logs/stream', handleStreamTaskLogsRequest)
  // /api/v1/protected/tasks/:taskId/messages/stream
  tasksRouter.get('/:taskId/messages/stream', handleStreamTaskMessagesRequest)
  // /api/v1/protected/tasks/:taskId
  tasksRouter.get('/:taskId', handleGetTaskRequest)
  // /api/v1/protected/tasks
  tasksRouter.post('/', handleCreateTaskRequest)
  // /api/v1/protected/tasks/:taskId
  tasksRouter.patch('/:taskId', handleUpdateTaskRequest)
  // /api/v1/protected/tasks/:taskId
  tasksRouter.delete('/:taskId', handleDeleteTaskRequest)
  // /api/v1/protected/tasks/:taskId/git/refresh
  tasksRouter.post('/:taskId/git/refresh', handleRefreshTaskGitRequest)
  // /api/v1/protected/tasks/:taskId/git/re-up
  tasksRouter.post('/:taskId/git/re-up', handleReUpTaskGitRequest)
  // /api/v1/protected/tasks/:taskId/new-message
  tasksRouter.post('/:taskId/new-message', handleTaskNewMessageRequest)
  // /api/v1/protected/tasks/:taskId/interrupt
  tasksRouter.post('/:taskId/interrupt', handleInterruptTaskRequest)
  // /api/v1/protected/tasks/:taskId/git/pull-request
  tasksRouter.post('/:taskId/git/pull-request', handleTaskPullRequestRequest)
  // /api/v1/protected/tasks/:taskId/git/view-repository
  tasksRouter.post('/:taskId/git/view-repository', handleViewTaskRepositoryRequest)

  return tasksRouter
}
