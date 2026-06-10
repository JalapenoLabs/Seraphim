<script lang="ts">
  import type { Task } from '../types'

  import { goto } from '$app/navigation'

  import { setTaskHold } from '../api'
  import { STATUS_BADGE, STATUS_LABELS } from '../types'
  import { Badge } from './ui/badge'
  import { Button } from './ui/button'

  let {
    task,
    onchange,
    repoName
  }: { task: Task; onchange: () => void; repoName?: string } = $props()

  // Show just the repo name (after the owner); the full owner/repo is on hover.
  const repoShort = $derived(repoName ? repoName.split('/').pop() : null)

  async function toggleHold(event: MouseEvent) {
    event.stopPropagation()
    await setTaskHold(task.id, !task.hold)
    onchange()
  }

  function open() {
    goto(`/task/${task.id}`)
  }
</script>

<div
  role="button"
  tabindex="0"
  onclick={open}
  onkeydown={(event) => event.key === 'Enter' && open()}
  class="cursor-grab rounded-lg border bg-secondary p-3 transition-colors hover:border-primary {task.hold
    ? 'border-dashed opacity-60'
    : 'border-border'}"
>
  <div class="flex items-center justify-between gap-2">
    <span class="truncate text-xs tabular-nums text-muted-foreground">
      {#if repoShort}<span class="font-semibold text-primary" title={repoName}>{repoShort}</span> · {/if}#{task.external_id}
    </span>
    <Badge variant="outline" class={STATUS_BADGE[task.status]}>
      {STATUS_LABELS[task.status] ?? task.status}
    </Badge>
  </div>

  <div class="mt-2 text-sm leading-snug">{task.title}</div>

  <div class="mt-2 flex items-center justify-between">
    <Button variant="ghost" size="sm" class="h-6 px-2 text-xs text-muted-foreground" onclick={toggleHold}>
      {task.hold ? '⏸ held' : 'hold'}
    </Button>
    {#if task.pr_url}
      <a
        href={task.pr_url}
        target="_blank"
        rel="noreferrer"
        onclick={(event) => event.stopPropagation()}
        class="text-xs text-primary hover:underline"
      >
        PR ↗
      </a>
    {/if}
  </div>

  {#if task.error}
    <div class="mt-2 border-t border-border pt-1.5 text-xs text-destructive">{task.error}</div>
  {/if}
</div>
