// Copyright © 2026 Jalapeno Labs

import type { Task, WorkspaceWithEnv, Llm, IssueTracking, GitAccount } from '@common/types'

// Core
import { useTaskMessages } from '@frontend/hooks/useTaskMessages'

// UI
import { TaskIcon } from '@frontend/elements/TaskIcon'

type Props = {
  task: Task
  llm: Llm
  workspace: WorkspaceWithEnv
  gitAccount: GitAccount
  issueTracker: IssueTracking
}

export function EditTask(props: Props) {
  const { turns } = useTaskMessages(props.task.id)

  console.log(props, turns)
  return <>
    <header className='compact level w-full'>
      <div className='level-left'>
        <h1 className='level-left text-2xl font-bold'>
          <span className='icon'>
            <TaskIcon
              task={props.task}
              size={24}
            />
          </span>
          {props.task.name || 'Untitled Task'}
        </h1>
      </div>
      <div className='level-right'>
      </div>
    </header>
  </>
}
