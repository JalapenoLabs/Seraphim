// Copyright © 2026 Jalapeno Labs

import type { Task } from '@common/types'

// Core
import { Navigate, useParams } from 'react-router'
import { useSelector } from '@frontend/framework/store'

// Misc
import { UrlTree } from '@common/urls'
import { EditTask } from './components/EditTask'

export function ViewTaskPage() {
  const { taskId } = useParams<{ taskId: string }>()
  const task = useSelector((state) => state.tasks.items.find((item) => item.id === taskId))

  if (!taskId || !task) {
    console.debug('ViewTaskPage task not found in redux, redirecting to tasks list', { taskId })
    return <Navigate to={UrlTree.tasks} replace />
  }

  return <TaskWithFullContext
    task={task as any}
  />
}

type TaskWithFullContextProps = {
  task: Task
}

function TaskWithFullContext(props: TaskWithFullContextProps) {
  const workspace = useSelector((state) => state.workspaces.items
    .find((item) => item.id === props.task.workspaceId),
  )
  const gitAccount = useSelector((state) => state.accounts.items
    .find((item) => item.id === props.task.gitAccountId),
  )
  const llm = useSelector((state) => state.llms.items
    .find((item) => item.id === props.task.llmId),
  )
  const issueTracker = useSelector((state) => state.issueTracking.items
    .find((item) => item.id === props.task.issueTrackingId),
  )

  return <EditTask
    task={props.task}
    workspace={workspace}
    gitAccount={gitAccount}
    llm={llm}
    issueTracker={issueTracker}
  />
}
