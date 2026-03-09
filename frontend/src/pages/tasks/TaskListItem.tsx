// Copyright © 2026 Jalapeno Labs

import type { Task } from '@common/types'

// User Interface
import { ListItem } from '../data/ListItem'
import { TaskIcon } from '@frontend/elements/TaskIcon'

type Props = {
  task: Task
  onSelect: (task: Task) => void
}

function getTaskDescription(task: Task) {
  const status = task.state
  const workspaceId = task.workspaceId

  return `Status: ${status} • Workspace: ${workspaceId}`
}

export function TaskListItem(props: Props) {
  const title = props.task.name?.trim() || `Untitled task ${props.task.id.slice(0, 8)}`

  return <ListItem
    id={props.task.id}
    title={title}
    description={getTaskDescription(props.task)}
    startContent={(
      <span className='icon opacity-80'>
        <TaskIcon
          task={props.task}
          size={20}
        />
      </span>
    )}
    onSelect={() => props.onSelect(props.task)}
  />
}
