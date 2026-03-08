// Copyright © 2026 Jalapeno Labs

import type { Message, Task } from '@common/types'
import type { TaskCreateRequest, TaskUpdateRequest } from '@common/schema/task'

// Core
import { ApiRoot, parseRequestBeforeSend } from '@common/api'
import { frontendClient } from '@frontend/framework/api'

// Redux
import { taskActions } from '@frontend/framework/redux/stores/tasks'
import { dispatch } from '@frontend/framework/store'

// Schema
import { taskCreateSchema, taskUpdateSchema } from '@common/schema/task'

// /////////////////////////////// //
//           List Tasks            //
// /////////////////////////////// //

type ListTasksResponse = {
  tasks: Task[]
}

export async function listTasks() {
  const response = await frontendClient
    .get('v1/protected/tasks')
    .json<ListTasksResponse>()

  dispatch(
    taskActions.setTasks(response.tasks),
  )

  return response
}

// /////////////////////////////// //
//            Get Task             //
// /////////////////////////////// //

type GetTaskResponse = {
  task: Task & { messages: Message[] }
}

export function getTask(taskId: string) {
  return frontendClient
    .get(`v1/protected/tasks/${taskId}`)
    .json<GetTaskResponse>()
}

export function getTaskMessagesSseUrl(taskId: string) {
  return `${ApiRoot}/api/v1/tasks/${taskId}/messages`
}

// /////////////////////////////// //
//           Create Task           //
// /////////////////////////////// //

type CreateTaskResponse = {
  task: Task
}

export async function createTask(raw: TaskCreateRequest) {
  const json = parseRequestBeforeSend(taskCreateSchema, raw)

  const response = await frontendClient
    .post('v1/protected/tasks', { json })
    .json<CreateTaskResponse>()

  dispatch(
    taskActions.upsertTask(response.task),
  )

  return response
}

// /////////////////////////////// //
//           Update Task           //
// /////////////////////////////// //

type UpdateTaskResponse = {
  task: Task
}

export async function updateTask(taskId: string, raw: TaskUpdateRequest) {
  const json = parseRequestBeforeSend(taskUpdateSchema, raw)

  const response = await frontendClient
    .patch(`v1/protected/tasks/${taskId}`, { json })
    .json<UpdateTaskResponse>()

  dispatch(
    taskActions.upsertTask(response.task),
  )

  return response
}

// /////////////////////////////// //
//           Delete Task           //
// /////////////////////////////// //

type DeleteTaskRequest = {
  id: string
}

type DeleteTaskResponse = {
  deleted: boolean
  task: Task
}

export async function deleteTask(request: DeleteTaskRequest) {
  const response = await frontendClient
    .delete(`v1/protected/tasks/${request.id}`)
    .json<DeleteTaskResponse>()

  dispatch(
    taskActions.removeTask(response.task),
  )

  return response
}

// /////////////////////////////// //
//         Task Git Actions        //
// /////////////////////////////// //

type TaskGitActionResponse = {
  message: string
}

function postTaskGitAction(taskId: string, actionPath: string) {
  return frontendClient
    .post(`v1/protected/tasks/${taskId}/git/${actionPath}`)
    .json<TaskGitActionResponse>()
}

export function refreshTaskGit(taskId: string) {
  return postTaskGitAction(taskId, 'refresh')
}

export function reUpTaskGit(taskId: string) {
  return postTaskGitAction(taskId, 're-up')
}

export function createOrUpdateTaskPullRequest(taskId: string) {
  return postTaskGitAction(taskId, 'pull-request')
}

export function viewTaskRepository(taskId: string) {
  return postTaskGitAction(taskId, 'view-repository')
}
