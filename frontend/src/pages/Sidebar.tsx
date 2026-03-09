// Copyright © 2026 Jalapeno Labs

import type { TaskState, Color } from '@common/types'

// Core
import { Link, useLocation } from 'react-router'
import { getViewTaskUrl, UrlTree } from '@common/urls'
import { useSelector } from '@frontend/framework/store'

// UI
import { Tooltip, Button } from '@heroui/react'
import { SettingsIcon } from '@frontend/elements/graphics/IconNexus'
import { TaskIcon } from '@frontend/elements/TaskIcon'
import { GoPlus } from 'react-icons/go'

const colorByStatus = {
  Creating: 'secondary',
  SettingUp: 'secondary',
  Working: 'primary',
  AwaitingReview: 'success',
  Reviewing: 'primary',
  Validating: 'primary',
  Halting: 'secondary',
  Deleting: 'secondary',
  ContainerBroken: 'danger',
  Failed: 'danger',
} as const satisfies Record<TaskState, Color>


export function Sidebar() {
  const location = useLocation()

  return <aside
    className='flex min-h-screen w-16 flex-col items-center justify-between py-4 bg-white/20 backdrop-blur-lg'
  >
    <section className='flex w-full flex-col items-center gap-4'>
      <Link to={UrlTree.tasks}>
        <figure className='flex items-center justify-center'>
          <img
            src='/brand/logo-64.png'
            alt='Seraphim logo'
            className='h-10 w-10 rounded-full'
          />
        </figure>
      </Link>

      <article className='flex flex-col gap-3 mt-4'>
        <Tooltip key='new-task' content='New Task' placement='right'>
          <Button
            isIconOnly
            as={Link}
            id='new-task'
            to={UrlTree.newTask}
            color='primary'
            variant='ghost'
          >
            <GoPlus size={24} />
          </Button>
        </Tooltip>
        <TaskList />
      </article>
    </section>

    <section className='flex items-center justify-center'>
      <Tooltip content='Settings' placement='right'>
        <Button
          isIconOnly
          as={Link}
          to={UrlTree.settings}
          color={location.pathname.startsWith(UrlTree.settings)
            ? 'primary'
            : 'default'
          }
        >
          <SettingsIcon size={22} />
        </Button>
      </Tooltip>
    </section>
  </aside>
}

function TaskList() {
  const location = useLocation()
  const tasks = useSelector((state) => state.tasks.items)

  const isTaskScreen = location.pathname.startsWith(UrlTree.tasks)
  const recentTasks = tasks.slice(0, 5)

  if (!isTaskScreen && !recentTasks?.length) {
    return <></>
  }

  return <>{
    recentTasks.map((task) => {
      const color = colorByStatus[task.state]
      const name = task.name || 'Untitled Task'

      return (
        <Tooltip
          key={task.id}
          content={`${name} (${task.state})`}
          placement='right'
        >
          <Button
            isIconOnly
            id={task.id}
            as={Link}
            to={getViewTaskUrl(task.id)}
            color={color}
          >{
            <TaskIcon
              task={task}
              size={19}
            />
          }</Button>
        </Tooltip>
      )
    })
  }</>
}
