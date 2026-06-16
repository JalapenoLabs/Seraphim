<script lang="ts">
  import type { Railway, Task } from '../types'

  import { goto } from '$app/navigation'
  import { Pause, Ban, TrainFront } from '@lucide/svelte'

  import { STATUS_BADGE, STATUS_LABELS, ticketStateBadge } from '../types'
  import { Badge } from './ui/badge'
  import { buttonVariants } from './ui/button'
  import * as DropdownMenu from './ui/dropdown-menu'
  import SourceIcon from './SourceIcon.svelte'

  let {
    task,
    onchange,
    repoName,
    suggestionCount = 0,
    selectionMode = false,
    selected = false,
    onselect,
    railways = [],
    onMoveToRailway
  }: {
    task: Task
    onchange: () => void
    repoName?: string
    suggestionCount?: number
    // Multi-select (bulk edit) mode: a click toggles selection instead of opening
    // the task, the card shows a loud highlight when selected, and unselected
    // cards dim so it is obvious which are picked.
    selectionMode?: boolean
    selected?: boolean
    onselect?: () => void
    // The full set of railways (swimlanes). When more than one exists and the card
    // has a repo, a "move to lane" control reassigns the card's repo's railway
    // (cross-lane is a repo move, never a card drag). Omitted when there is only
    // the single `main` lane, where the control is meaningless.
    railways?: Railway[]
    onMoveToRailway?: (railwayId: string) => void
  } = $props()

  // The other lanes this card's repo can move to. Empty for a tracking-only card
  // (no repo) or when `main` is the only lane, which hides the control entirely.
  const otherRailways = $derived(
    task.repo_id ? railways.filter((railway) => railway.id !== task.railway_id) : []
  )

  // Show just the repo name (after the owner); the full owner/repo is on hover.
  const repoShort = $derived(repoName ? repoName.split('/').pop() : null)

  // The source ticket's open/closed (GitHub) or workflow (Jira) state, or null.
  const ticketState = $derived(ticketStateBadge(task))

  // A click opens the task normally, or toggles its selection in bulk mode.
  function activate() {
    if (selectionMode) {
      onselect?.()
      return
    }
    goto(`/task/${task.id}`)
  }
</script>

<div
  role="button"
  tabindex="0"
  aria-pressed={selectionMode ? selected : undefined}
  onclick={activate}
  onkeydown={(event) => event.key === 'Enter' && activate()}
  class="rounded-lg border bg-secondary p-3 transition-all {selectionMode
    ? 'cursor-pointer'
    : 'cursor-grab'} {selected
    ? 'border-primary ring-2 ring-primary bg-primary/10'
    : selectionMode
      ? 'border-border opacity-50 hover:opacity-100 hover:border-primary'
      : task.hold
        ? 'border-dashed border-border opacity-60 hover:border-primary'
        : task.blocking
          ? 'border-warning hover:border-primary'
          : 'border-border hover:border-primary'}"
>
  <div class="flex items-center justify-between gap-2">
    <span class="flex min-w-0 items-center gap-1 text-xs tabular-nums text-muted-foreground">
      {#if task.hold}<Pause class="size-3 flex-none" aria-label="On hold" />{/if}
      {#if task.blocking}<Ban
          class="size-3 flex-none text-warning"
          aria-label="Blocking: holds the queue until finished"
        />{/if}
      <SourceIcon source={task.source_kind} class="size-3.5 flex-none" />
      {#if repoShort}<span class="truncate font-semibold text-primary" title={repoName}>{repoShort}</span>{/if}
      <span class="flex-none">{#if repoShort} · {/if}#{task.external_id}</span>
      {#if ticketState}
        <Badge variant="outline" class="flex-none px-1.5 py-0 text-[10px] {ticketState.class}">
          {ticketState.label}
        </Badge>
      {/if}
    </span>
    <Badge variant="outline" class={STATUS_BADGE[task.status]}>
      {STATUS_LABELS[task.status] ?? task.status}
    </Badge>
  </div>

  <div class="mt-2 flex items-start gap-2">
    {#if task.author_avatar_url}
      <img
        src={task.author_avatar_url}
        alt={task.author_login ?? 'issue author'}
        title={task.author_login ? `Opened by ${task.author_login}` : 'Issue author'}
        class="mt-0.5 size-5 flex-none rounded-full"
        onerror={(event) => ((event.currentTarget as HTMLImageElement).style.display = 'none')}
      />
    {/if}
    <div class="min-w-0 text-sm leading-snug">{task.title}</div>
  </div>

  <!-- Loud on purpose: pulses until the user acknowledges the suggestions on the task. -->
  {#if suggestionCount > 0}
    <div
      class="mt-2 animate-pulse rounded-md bg-warning px-2 py-1 text-center text-xs font-bold text-background motion-reduce:animate-none"
      title="The agent recommended environment changes"
    >
      💡 {suggestionCount} setup {suggestionCount === 1 ? 'suggestion' : 'suggestions'}
    </div>
  {/if}

  {#if (otherRailways.length > 0 && !selectionMode) || task.pr_url}
    <div class="mt-2 flex items-center justify-end gap-3">
      {#if otherRailways.length > 0 && !selectionMode}
        <!-- Reassign the card's repo to another swimlane. This moves the repo (and
             all its tasks), so it is a repo action, not a single-card drag. The
             backend blocks it while a live turn is working the repo. -->
        <DropdownMenu.Root>
          <DropdownMenu.Trigger
            onclick={(event: MouseEvent) => event.stopPropagation()}
            class={buttonVariants({ variant: 'ghost', size: 'sm' }) +
              ' h-6 gap-1 px-1.5 text-xs text-muted-foreground'}
            title="Move this repo to another railway"
          >
            <TrainFront class="size-3.5" />
            Move lane
          </DropdownMenu.Trigger>
          <DropdownMenu.Content align="end" class="min-w-44">
            <DropdownMenu.Label class="text-xs">Move repo to railway</DropdownMenu.Label>
            {#each otherRailways as railway (railway.id)}
              <DropdownMenu.Item
                onclick={(event: MouseEvent) => {
                  event.stopPropagation()
                  onMoveToRailway?.(railway.id)
                }}
              >
                {railway.name}
              </DropdownMenu.Item>
            {/each}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      {/if}
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
  {/if}

  {#if task.error}
    <div class="mt-2 border-t border-border pt-1.5 text-xs text-destructive">{task.error}</div>
  {/if}
</div>
