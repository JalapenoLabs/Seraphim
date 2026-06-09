// Copyright © 2026 Jalapeno Labs

import type { Llm, Workspace } from '@prisma/client'
import type { TaskCreateRequest } from '@common/schema/task'
import type {
  ArchiveTaskResult,
  CreateTaskResult,
  DeleteTaskResult,
  EvaluateTaskFileDiffResult,
} from './types'

// Utility
import { broadcastSseChange } from '@electron/api/sse/sseEvents'
import { requireDatabaseClient } from '@electron/database'
import { toContainerName } from '@electron/jobs/taskNaming'
import { teardownTask } from '@electron/jobs/teardownTask'
import { updateTaskState } from '@electron/jobs/updateTaskState'
import { getCallableLLM } from '@common/llms/call'
import {
  defaultTaskIconName,
  getValidTaskIconName,
  taskIconLabelByName,
  taskIconNames,
} from '@common/taskIcons'

// Misc
import { collectTaskFileChanges } from './fileDiff'
import { selectTaskWithFullContext } from './select'
import { TaskInstance } from './taskInstance'

class TaskManager {
  private taskInstances = new Map<string, TaskInstance>()

  public getTask(taskId: string): TaskInstance | null {
    const trimmedTaskId = taskId?.trim()
    if (!trimmedTaskId) {
      console.debug('TaskManager getTask called with empty taskId', { taskId })
      return null
    }

    return this.taskInstances.get(trimmedTaskId) ?? null
  }

  public async initializeFromDatabase(): Promise<void> {
    const databaseClient = requireDatabaseClient('TaskManager initialize')
    const tasks = await databaseClient.task.findMany({
      where: { archived: false },
      include: selectTaskWithFullContext,
    })

    for (const task of tasks) {
      if (this.taskInstances.has(task.id)) {
        console.debug('TaskManager skipped duplicate task initialization', {
          taskId: task.id,
        })
        continue
      }

      const taskInstance = new TaskInstance({
        task,
        containerExists: false,
      })

      this.taskInstances.set(task.id, taskInstance)
      const containerExists = await taskInstance.refreshContainerStatus()
      if (containerExists) {
        await taskInstance.attachToContainer()
        continue
      }

      console.debug('TaskManager deleting task during initialization because container was not found', {
        taskId: task.id,
        containerId: task.container,
      })

      const deletedTask = await this.deleteTask(task.id)
      if (deletedTask.status !== 'deleted') {
        console.debug('TaskManager failed to delete task during initialization after missing container', {
          taskId: task.id,
          deleteStatus: deletedTask.status,
        })
      }
    }
  }

  public async createTask(request: TaskCreateRequest): Promise<CreateTaskResult> {
    const databaseClient = requireDatabaseClient('TaskManager createTask')

    const [ user, workspace, llm ] = await Promise.all([
      databaseClient.user.findFirst(),
      databaseClient.workspace.findUnique({
        where: {
          id: request.workspaceId,
        },
        include: {
          envEntries: true,
        },
      }),
      databaseClient.llm.findUnique({
        where: {
          id: request.llmId,
        },
      }),
    ])

    if (!workspace) {
      console.debug('Task creation failed, workspace not found', {
        workspaceId: request.workspaceId,
      })
      return {
        status: 'error',
        error: 'Workspace not found',
        httpStatus: 404,
      }
    }

    if (!llm) {
      console.debug('Task creation failed, llm not found', {
        llmId: request.llmId,
      })
      return {
        status: 'error',
        error: 'Llm not found',
        httpStatus: 404,
      }
    }

    if (!workspace.sourceRepoUrl?.trim()) {
      console.debug('Task creation failed, workspace missing source repo URL', {
        workspaceId: request.workspaceId,
      })
      return {
        status: 'error',
        error: 'Workspace source repository is required',
        httpStatus: 400,
      }
    }

    const { taskName, gitWorkBranchName, taskIconName } = await this.requestTaskName(llm, workspace, request.message)

    if (!taskName || !gitWorkBranchName) {
      return {
        status: 'error',
        error: 'Failed to generate task name or git branch name',
        httpStatus: 400,
      }
    }

    const resolvedContainerName = toContainerName(taskName)

    const createdTask = await databaseClient.task.create({
      data: {
        userId: user?.id,
        workspaceId: request.workspaceId,
        llmId: request.llmId,
        gitAccountId: request.gitAccountId,
        issueTrackingId: request.issueTrackingId,
        name: taskName,
        icon: taskIconName,
        sourceGitBranch: request.branch,
        workGitBranch: gitWorkBranchName,
        issueLink: request.issueLink?.trim() || null,
        container: 'pending',
        containerName: resolvedContainerName,
        archived: request.archived,
      },
      include: selectTaskWithFullContext,
    })

    const taskInstance = new TaskInstance({
      task: createdTask,
      containerExists: false,
    })

    this.taskInstances.set(createdTask.id, taskInstance)

    broadcastSseChange({
      type: 'create',
      kind: 'tasks',
      data: createdTask,
    })

    return {
      status: 'created',
      task: createdTask,
    }
  }

  public async launchTask(taskId: string, initialMessage: string): Promise<void> {
    try {
      const taskInstance = this.taskInstances.get(taskId)
      if (!taskInstance) {
        console.debug('Task launch requested for task that does not exist in TaskManager', {
          taskId,
        })
        return
      }

      const updatedTask = await taskInstance.createContainer()
      await updatedTask.queueUserMessage({
        role: 'User',
        type: 'userMessage',
        content: initialMessage,
      })

      broadcastSseChange({
        type: 'update',
        kind: 'tasks',
        data: updatedTask.data,
      })
    }
    catch (error) {
      console.error('Failed to provision task container', error)
    }
  }

  public async evaluateFileDiff(taskId: string): Promise<EvaluateTaskFileDiffResult> {
    const trimmedTaskId = taskId?.trim()
    if (!trimmedTaskId) {
      console.debug('Task evaluateFileDiff requested without taskId', { taskId })
      return {
        status: 'error',
        error: 'Task ID is required',
        httpStatus: 400,
      }
    }

    const databaseClient = requireDatabaseClient('TaskManager evaluateFileDiff')

    let taskInstance = this.taskInstances.get(trimmedTaskId) ?? null
    let didHydrateTaskInstance = false
    if (!taskInstance) {
      const existingTask = await databaseClient.task.findUnique({
        where: { id: trimmedTaskId },
        include: selectTaskWithFullContext,
      })

      if (!existingTask) {
        console.debug('Task evaluateFileDiff failed, task not found', {
          taskId: trimmedTaskId,
        })
        return {
          status: 'error',
          error: 'Task not found',
          httpStatus: 404,
        }
      }

      taskInstance = new TaskInstance({
        task: existingTask,
        containerExists: false,
      })
      this.taskInstances.set(trimmedTaskId, taskInstance)
      didHydrateTaskInstance = true
    }

    const containerExists = await taskInstance.refreshContainerStatus()
    if (!containerExists) {
      if (didHydrateTaskInstance) {
        this.taskInstances.delete(trimmedTaskId)
      }

      console.debug('Task evaluateFileDiff failed, container not available', {
        taskId: trimmedTaskId,
        containerId: taskInstance.data.container,
      })
      return {
        status: 'error',
        error: 'Task container is not available',
        httpStatus: 409,
      }
    }

    try {
      const fileChanges = await collectTaskFileChanges(taskInstance)

      await databaseClient.$transaction(async (transaction) => {
        await transaction.fileChange.deleteMany({
          where: { taskId: trimmedTaskId },
        })

        if (!fileChanges.length) {
          return
        }

        await transaction.fileChange.createMany({
          data: fileChanges.map((fileChange) => ({
            ...fileChange,
            taskId: trimmedTaskId,
          })),
        })
      })

      return {
        status: 'evaluated',
        taskId: trimmedTaskId,
        fileChangeCount: fileChanges.length,
      }
    }
    catch (error) {
      if (didHydrateTaskInstance) {
        this.taskInstances.delete(trimmedTaskId)
      }

      console.error('Task evaluateFileDiff failed', {
        taskId: trimmedTaskId,
        error,
      })

      return {
        status: 'error',
        error: 'Failed to evaluate task file diff',
        httpStatus: 500,
      }
    }
  }

  public async archiveTask(taskId: string): Promise<ArchiveTaskResult> {
    const trimmedTaskId = taskId?.trim()
    if (!trimmedTaskId) {
      console.debug('Task archive requested without taskId', { taskId })
      return {
        status: 'error',
        error: 'Task ID is required',
        httpStatus: 400,
      }
    }

    const databaseClient = requireDatabaseClient('TaskManager archiveTask')
    const existingTask = await databaseClient.task.findUnique({
      where: { id: trimmedTaskId },
    })

    if (!existingTask) {
      console.debug('Task archive failed, task not found', {
        taskId: trimmedTaskId,
      })
      return {
        status: 'error',
        error: 'Task not found',
        httpStatus: 404,
      }
    }

    if (existingTask.archived) {
      console.debug('Task archive requested for task that is already archived', {
        taskId: trimmedTaskId,
      })
      return {
        status: 'archived',
        taskId: trimmedTaskId,
      }
    }

    const archivedTask = await databaseClient.task.update({
      where: { id: trimmedTaskId },
      data: { archived: true },
    })

    const taskInstance = this.taskInstances.get(trimmedTaskId)
    if (taskInstance) {
      await taskInstance.teardown()
      this.taskInstances.delete(trimmedTaskId)
    }
    else {
      console.debug('Task archive requested without task instance', {
        taskId: trimmedTaskId,
      })
      await teardownTask(existingTask.container)
    }

    broadcastSseChange({
      type: 'update',
      kind: 'tasks',
      data: archivedTask,
    })

    return {
      status: 'archived',
      taskId: trimmedTaskId,
    }
  }

  public async deleteTask(taskId: string): Promise<DeleteTaskResult> {
    const trimmedTaskId = taskId?.trim()
    if (!trimmedTaskId) {
      console.debug('Task delete requested without taskId', { taskId })
      return {
        status: 'error',
        error: 'Task ID is required',
        httpStatus: 400,
      }
    }

    const databaseClient = requireDatabaseClient('TaskManager deleteTask')
    const existingTask = await databaseClient.task.findUnique({
      where: { id: trimmedTaskId },
    })

    if (!existingTask) {
      console.debug('Task delete failed, task not found', {
        taskId: trimmedTaskId,
      })
      return {
        status: 'error',
        error: 'Task not found',
        httpStatus: 404,
      }
    }

    await updateTaskState(trimmedTaskId, 'Deleting')

    const taskInstance = this.taskInstances.get(trimmedTaskId)
    if (taskInstance) {
      await taskInstance.teardown()
    }
    else {
      console.debug('Task delete requested without task instance', {
        taskId: trimmedTaskId,
      })
      await teardownTask(existingTask.container)

      console.debug('Deleting task and its messages')
      await databaseClient.task.delete({
        where: { id: trimmedTaskId },
      })
    }

    this.taskInstances.delete(trimmedTaskId)

    broadcastSseChange({
      type: 'delete',
      kind: 'tasks',
      data: existingTask,
    })

    return {
      status: 'deleted',
      taskId: trimmedTaskId,
    }
  }

  private async requestTaskName(llm: Llm, workspace: Workspace, userMessage: string) {
    try {
      const callableLlm = getCallableLLM(llm)

      const shortHash1 = `${Math.floor(Date.now() / 1000).toString(36)}`
      const iconCatalogPrompt = taskIconNames
        .map((iconName) => `- ${iconName}: ${taskIconLabelByName[iconName]}`)
        .join('\n')

      const [
        taskName = 'Unnamed task',
        gitWorkBranchName = shortHash1,
        taskIconNameRaw = defaultTaskIconName,
      ] = await Promise.all([
        callableLlm.query(
          userMessage,
          (`Below the user will provide an initial request for a given agentic task.`
          + ` Your job is to give it a clean and short & friendly PR name for when the task is completed.`
          + ` Use 3-6 words when possible, and no more than 10 words.`
          + ` Return the task name only, no punctuation or quotes.`),
        ),
        callableLlm.query(
          (`Git branch template: '${workspace.gitBranchTemplate}'`
          + `\n\nTask:\n\`\`\`\n${userMessage}\n\`\`\``),
          (`You're an assistant helping to prepare an automated git job,`
            + ` and you're responsible for naming the git branch for this automated task.`
            + ` Below the user will provide an initial task request AND a give preferred git work branch template.`
          + ` Your job is transform the user's task request into their preferred git work branch name.`
          + ` This will be used to name the git branch that the task will be created with.`
          + ` Never start a git branch name with a dash.`
          + ` Return the formed git branch name only, no punctuation or quotes.`),
        ),
        callableLlm.query(
          userMessage,
          (`Below the user will provide an initial task request.`
            + ` Your job is to choose the best matching icon name from the allowed react-icons list.`
            + ` Pick exactly one icon and return only the icon name.`
            + `\n\nAllowed icons:\n${iconCatalogPrompt}`),
        ),
      ])

      const shortHash2 = `${Math.floor(Date.now() / 1000).toString(36)}`
      const taskIconName = getValidTaskIconName(taskIconNameRaw?.trim())

      return {
        taskName,
        gitWorkBranchName: `${gitWorkBranchName}--${shortHash2}`,
        taskIconName,
      } as const
    }
    catch (error) {
      console.error('Failed to generate task name, git branch name, or icon from LLM', error)
      return {
        taskName: null,
        gitWorkBranchName: null,
        taskIconName: defaultTaskIconName,
      }
    }
  }
}

const taskManager: TaskManager = new TaskManager()
export function getTaskManager(): TaskManager {
  return taskManager
}
