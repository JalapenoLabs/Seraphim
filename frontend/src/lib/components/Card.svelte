<script lang="ts">
  import type { Task } from '../types'

  import { goto } from '$app/navigation'
  import { Pause } from '@lucide/svelte'

  import { STATUS_BADGE, STATUS_LABELS } from '../types'
  import { Badge } from './ui/badge'

  let {
    task,
    onchange,
    repoName
  }: { task: Task; onchange: () => void; repoName?: string } = $props()

  // Show just the repo name (after the owner); the full owner/repo is on hover.
  const repoShort = $derived(repoName ? repoName.split('/').pop() : null)

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
    <span class="flex min-w-0 items-center gap-1 truncate text-xs tabular-nums text-muted-foreground">
      {#if task.hold}<Pause class="size-3 flex-none" aria-label="On hold" />{/if}
      {#if repoShort}<span class="font-semibold text-primary" title={repoName}>{repoShort}</span> · {/if}#{task.external_id}
    </span>
    <Badge variant="outline" class={STATUS_BADGE[task.status]}>
      {STATUS_LABELS[task.status] ?? task.status}
    </Badge>
  </div>

  <div class="mt-2 text-sm leading-snug">{task.title}</div>

  {#if task.pr_url}
    <div class="mt-2 flex justify-end">
      <a
        href={task.pr_url}
        target="_blank"
        rel="noreferrer"
        onclick={(event) => event.stopPropagation()}
        class="text-xs text-primary hover:underline"
      >
        PR ↗
      </a>
    </div>
  {/if}

  {#if task.error}
    <div class="mt-2 border-t border-border pt-1.5 text-xs text-destructive">{task.error}</div>
  {/if}
</div>
