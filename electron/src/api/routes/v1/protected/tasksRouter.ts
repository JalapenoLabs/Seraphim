// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleCreateTaskRequest } from './tasks/createTaskRoute'
import { handleDeleteTaskRequest } from './tasks/deleteTaskRoute'
import { handleGetTaskRequest } from './tasks/getTaskRoute'
import { handleListTasksRequest } from './tasks/listTasksRoute'
import { handleReUpTaskGitRequest } from './tasks/postReUpTaskGitRoute'
import { handleRefreshTaskGitRequest } from './tasks/postRefreshTaskGitRoute'
import { handleTaskPullRequestRequest } from './tasks/postTaskPullRequestRoute'
import { handleViewTaskRepositoryRequest } from './tasks/postViewTaskRepositoryRoute'
import { handleUpdateTaskRequest } from './tasks/updateTaskRoute'

export function createTasksRouter(): Router {
  const tasksRouter = createRouter()

  // /api/v1/protected/tasks
  tasksRouter.get('/', handleListTasksRequest)
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
  // /api/v1/protected/tasks/:taskId/git/pull-request
  tasksRouter.post('/:taskId/git/pull-request', handleTaskPullRequestRequest)
  // /api/v1/protected/tasks/:taskId/git/view-repository
  tasksRouter.post('/:taskId/git/view-repository', handleViewTaskRepositoryRequest)

  return tasksRouter
}
