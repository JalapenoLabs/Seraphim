// Copyright © 2026 Jalapeno Labs

import type { Task, WorkspaceWithEnv, Llm, IssueTracking, GitAccount } from '@common/types'

// Core
import { useTaskMessages } from '@frontend/hooks/useTaskMessages'

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
        <h1 className='text-2xl font-bold'>{props.task.name || 'Untitled Task'}</h1>
      </div>
      <div className='level-right'>
      </div>
    </header>
  </>
}
