<script lang="ts">
  import type { Railway, SourceKind, Task, TaskColumn } from '../types'

  import { goto } from '$app/navigation'
  import {
    Pause,
    Play,
    Ban,
    TrainFront,
    SquareArrowOutUpRight,
    Link as LinkIcon,
    GitPullRequestArrow,
    Hash,
    GitBranch,
    Type,
    ArrowRightLeft,
    ArrowUpToLine,
    Columns2,
    Check,
    CircleSlash,
    RotateCcw,
    Trash2,
    Search
  } from '@lucide/svelte'
  import { toast } from 'svelte-sonner'

  import { STATUS_BADGE, STATUS_LABELS, ticketStateBadge } from '../types'
  import { Badge } from './ui/badge'
  import { buttonVariants } from './ui/button'
  import * as ContextMenu from './ui/context-menu'
  import * as DropdownMenu from './ui/dropdown-menu'
  import SourceIcon from './SourceIcon.svelte'

  let {
    task,
    onchange,
    repoName,
    suggestionCount = 0,
    selectionMode = false,
    selected = false,
    selectedCount = 0,
    onselect,
    railways = [],
    recentLaneIds = [],
    onMoveToRailway,
    onMoveToColumn,
    onToggleHold,
    onReset,
    onDelete
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
    // How many cards are currently selected board-wide. When this card is part of a
    // multi-selection (issue #236) the context menu acts on the whole selection.
    selectedCount?: number
    onselect?: () => void
    // The full set of railways (swimlanes). When more than one exists and the card
    // has a repo, a "move to lane" control reassigns the card's repo's railway
    // (cross-lane is a repo move, never a card drag). Omitted when there is only
    // the single `main` lane, where the control is meaningless.
    railways?: Railway[]
    // Recently used lane move targets (railway ids), newest first, for the lane
    // submenu's shortlist (issue #236). The board owns this and persists it.
    recentLaneIds?: string[]
    onMoveToRailway?: (railwayId: string) => void
    // Move this card to another board column (context menu). "To Do" carries a
    // placement so the agent's "do this next" (top) vs "later" (bottom) intent is
    // expressible; the board computes the rank and calls moveTask.
    onMoveToColumn?: (column: TaskColumn, placement: 'top' | 'bottom') => void
    // Lifecycle actions (issue #235). The board owns the API call, toast, and (for
    // the destructive ones) the confirmation dialog; the card just signals intent.
    onToggleHold?: () => void
    onReset?: () => void
    onDelete?: () => void
  } = $props()

  // Multi-select awareness (issue #236): when this card is part of a multi-card
  // selection, the menu acts on the whole selection. The board routes each action
  // to all selected cards; here it drives the header and which items are shown
  // (per-card-only actions are hidden in multi mode; bulk-capable ones stay).
  const multi = $derived(selected && selectedCount > 1)

  // Above this many lanes, the lane submenu shows a filter box (issue #236). Below
  // it the list is short enough to scan, and bits-ui's type-ahead already covers
  // keyboard jumping.
  const LANE_FILTER_THRESHOLD = 7
  let laneFilter = $state('')

  // Recently used lanes still on the board, excluding the card's current lane in
  // single-card mode (in multi mode the selection may span lanes, so keep them).
  const recentRailways = $derived(
    recentLaneIds
      .map((id) => railways.find((railway) => railway.id === id))
      .filter((railway): railway is Railway => !!railway)
      .filter((railway) => multi || railway.id !== task.railway_id)
  )

  // The lane list narrowed by the filter box (case-insensitive name match).
  const filteredRailways = $derived(
    laneFilter.trim()
      ? railways.filter((railway) =>
          railway.name.toLowerCase().includes(laneFilter.trim().toLowerCase())
        )
      : railways
  )

  // Whether this task has actually started an attempt: it has a branch or PR, or
  // it has moved past the queue. A task that never started has nothing to reset,
  // so the destructive "Reset" action is disabled for it (issue #235).
  const hasStarted = $derived(
    !!task.branch ||
      !!task.pr_url ||
      task.board_column === 'in_progress' ||
      task.board_column === 'in_review' ||
      task.board_column === 'done'
  )

  // The board columns the context menu offers as move targets (issue #233). In
  // Progress is omitted (the agent owns it); To Do is split into top/bottom of the
  // queue, mirroring the server-side automation rank.
  const COLUMN_MOVES: { column: TaskColumn; placement: 'top' | 'bottom'; label: string }[] = [
    { column: 'available', placement: 'top', label: 'Available' },
    { column: 'todo', placement: 'top', label: 'To Do (top)' },
    { column: 'todo', placement: 'bottom', label: 'To Do (bottom)' },
    { column: 'in_review', placement: 'top', label: 'In Review' },
    { column: 'done', placement: 'top', label: 'Done' },
    { column: 'ignored', placement: 'top', label: 'Ignored' }
  ]

  // The other lanes this card's repo can move to. Empty for a tracking-only card
  // (no repo) or when `main` is the only lane, which hides the control entirely.
  const otherRailways = $derived(
    task.repo_id ? railways.filter((railway) => railway.id !== task.railway_id) : []
  )

  // Show just the repo name (after the owner); the full owner/repo is on hover.
  const repoShort = $derived(repoName ? repoName.split('/').pop() : null)

  // The source ticket's open/closed (GitHub) or workflow (Jira) state, or null.
  const ticketState = $derived(ticketStateBadge(task))

  // The label for the "open the external issue" action, per source. Internal
  // tasks have no external issue, so the item is disabled regardless of label.
  const EXTERNAL_OPEN_LABELS = {
    github: 'Open on GitHub',
    jira: 'Open in Jira',
    internal: 'Open issue'
  } as const satisfies Record<SourceKind, string>

  // A pasteable reference to the source issue: `owner/repo#number` when the task
  // has a repo (a cross-repo GitHub reference), else `#number`. Matches the card's
  // own `#{external_id}` convention.
  const issueReference = $derived(repoName ? `${repoName}#${task.external_id}` : `#${task.external_id}`)

  // A click opens the task normally, or toggles its selection in bulk mode.
  function activate() {
    if (selectionMode) {
      onselect?.()
      return
    }
    goto(`/task/${task.id}`)
  }

  // The "Open task" menu item always navigates, even in bulk-select mode (a left
  // click there toggles selection instead, so the menu needs its own opener).
  function openTask() {
    goto(`/task/${task.id}`)
  }

  // Open the task's pull request in a new tab. Only shown when the task has one.
  function openPullRequest() {
    if (task.pr_url) {
      window.open(task.pr_url, '_blank', 'noopener,noreferrer')
    }
  }

  // Open the linked external issue (GitHub/Jira) in a new tab. Disabled when there
  // is no external URL (an internal task), so `task.url` is always set here.
  function openExternalIssue() {
    if (task.url) {
      window.open(task.url, '_blank', 'noopener,noreferrer')
    }
  }

  // Write `text` to the clipboard and confirm with a small toast, or surface a
  // failure (e.g. a denied clipboard permission) rather than failing silently.
  async function copyToClipboard(text: string, confirmation: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast.success(confirmation)
    }
    catch (error) {
      console.debug('clipboard write failed', error)
      toast.error('Could not copy to the clipboard')
    }
  }

  function copyLink() {
    copyToClipboard(`${window.location.origin}/task/${task.id}`, 'Copied link')
  }

  function copyIssueReference() {
    copyToClipboard(issueReference, `Copied ${issueReference}`)
  }

  function copyBranch() {
    if (task.branch) {
      copyToClipboard(task.branch, 'Copied branch name')
    }
  }

  function copyTitle() {
    copyToClipboard(task.title, 'Copied title')
  }

  // Whether the card's right-click context menu is open. Bound to the bits-ui
  // root so we can also close it on scroll (the board scrolls, and the menu is
  // anchored to a fixed viewport point, so it would otherwise float orphaned).
  let menuOpen = $state(false)

  // Right-click and touch long-press are handled natively by the bits-ui
  // ContextMenu trigger. This adds the keyboard path: the Menu/Apps key, or
  // Shift+F10, opens the menu when the card is focused. We synthesize a
  // `contextmenu` event near the card's top-left so the same trigger logic
  // positions and opens the menu, with no pointer required.
  function onCardKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      activate()
      return
    }
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault()
      const card = event.currentTarget as HTMLElement
      const rect = card.getBoundingClientRect()
      card.dispatchEvent(
        new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          clientX: rect.left + 16,
          clientY: rect.top + 16
        })
      )
      return
    }
    // Power-user shortcuts on a focused card (issue #236): W = work this now,
    // H = toggle hold. The menu shows these hints. Skipped when a modifier is held
    // so a browser/OS shortcut is never clobbered. (When the menu is open, focus is
    // in the menu, not the card, so these never fire there.)
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return
    }
    if (event.key === 'w' || event.key === 'W') {
      event.preventDefault()
      onMoveToColumn?.('todo', 'top')
    } else if (event.key === 'h' || event.key === 'H') {
      event.preventDefault()
      onToggleHold?.()
    }
  }

  // Keep typing in the lane filter box from triggering the menu's type-ahead, while
  // still letting the menu's own navigation/dismiss keys through.
  function onLaneFilterKeydown(event: KeyboardEvent) {
    if (['Escape', 'ArrowDown', 'ArrowUp', 'Enter', 'Tab'].includes(event.key)) {
      return
    }
    event.stopPropagation()
  }

  // Close the menu when anything other than the menu itself scrolls. The menu is
  // anchored to a fixed viewport point, so a board scroll would leave it floating
  // over the wrong card; closing is simpler and clearer than repositioning.
  // Scroll events do not bubble, so we listen in the capture phase.
  $effect(() => {
    if (!menuOpen) {
      // Reset the lane filter so the next open starts fresh.
      laneFilter = ''
      return
    }
    function onScroll(event: Event) {
      const target = event.target
      if (target instanceof Element && target.closest('[data-slot="context-menu-content"]')) {
        return
      }
      menuOpen = false
    }
    window.addEventListener('scroll', onScroll, { capture: true, passive: true })
    return () => window.removeEventListener('scroll', onScroll, { capture: true })
  })
</script>

<!--
  The whole card is the context-menu trigger: right-click opens the menu at the
  cursor, touch long-press opens it (both handled natively by bits-ui), and the
  keyboard Menu key / Shift+F10 open it via `onCardKeydown`. The trigger renders
  this div itself (not a wrapper), so it stays the focusable card and focus
  returns here when the menu closes. right-click and a stationary long-press do
  not start a `svelte-dnd-action` drag (it ignores non-left buttons and only
  drags on movement).
-->
<ContextMenu.Root bind:open={menuOpen}>
  <ContextMenu.Trigger
    role="button"
    tabindex={0}
    aria-pressed={selectionMode ? selected : undefined}
    onclick={activate}
    onkeydown={onCardKeydown}
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
  </ContextMenu.Trigger>

  <!--
    Card actions (issues #232/#233/#234): quick / navigation actions on top, then
    the copy actions, then "Move to...". Closing on Escape / outside click / item
    select is handled by bits-ui; scroll close is wired in this component (see the
    `$effect` above). An action that does not apply (no external URL, no PR, no
    branch) is disabled or hidden, never shown broken.
  -->
  <ContextMenu.Content class="min-w-52">
    <!--
      Header: the single card's issue ref, or the selection count when the menu
      acts on a multi-selection (issue #236).
    -->
    {#if multi}
      <ContextMenu.Label class="text-xs">{selectedCount} cards selected</ContextMenu.Label>
    {:else}
      <ContextMenu.Label class="text-xs">#{task.external_id}</ContextMenu.Label>
    {/if}

    <!--
      Per-card-only actions (open / copy): meaningless across a selection, so they
      are hidden in multi mode (issue #236).
    -->
    {#if !multi}
      <ContextMenu.Item onclick={openTask}>
        <SquareArrowOutUpRight class="size-4" />
        Open task
      </ContextMenu.Item>
      <ContextMenu.Item
        disabled={!task.url}
        title={task.url ? undefined : 'This task has no linked external issue'}
        onclick={openExternalIssue}
      >
        <SourceIcon source={task.source_kind} class="size-4" />
        {EXTERNAL_OPEN_LABELS[task.source_kind]}
      </ContextMenu.Item>
      {#if task.pr_url}
        <ContextMenu.Item onclick={openPullRequest}>
          <GitPullRequestArrow class="size-4" />
          Open pull request
        </ContextMenu.Item>
      {/if}

      <ContextMenu.Separator />

      <ContextMenu.Item onclick={copyLink}>
        <LinkIcon class="size-4" />
        Copy link
      </ContextMenu.Item>
      <ContextMenu.Item onclick={copyIssueReference}>
        <Hash class="size-4" />
        Copy issue reference
      </ContextMenu.Item>
      {#if task.branch}
        <ContextMenu.Item onclick={copyBranch}>
          <GitBranch class="size-4" />
          Copy branch name
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onclick={copyTitle}>
        <Type class="size-4" />
        Copy title
      </ContextMenu.Item>

      <ContextMenu.Separator />
    {/if}

    <!--
      Bulk-capable actions (issues #233/#235/#236): Move to column/lane, Work this
      now, Hold/Unhold, Send to Ignored. The board applies each to the whole
      selection when one is active, otherwise just this card. A column move
      re-ranks/relocates; a lane move reassigns the REPO(s) (confirmed + toasted by
      the board). Current column/lane is checked only in single-card mode.
    -->
    <ContextMenu.Sub>
      <ContextMenu.SubTrigger>
        <ArrowRightLeft class="size-4" />
        Move to
      </ContextMenu.SubTrigger>
      <ContextMenu.SubContent class="min-w-40">
        <ContextMenu.Sub>
          <ContextMenu.SubTrigger>
            <Columns2 class="size-4" />
            Column
          </ContextMenu.SubTrigger>
          <ContextMenu.SubContent class="min-w-40">
            {#each COLUMN_MOVES as move (move.label)}
              {@const isCurrent = !multi && move.column === task.board_column}
              <ContextMenu.Item
                disabled={isCurrent && move.column !== 'todo'}
                onclick={() => onMoveToColumn?.(move.column, move.placement)}
              >
                {#if isCurrent}
                  <Check class="size-4" />
                {:else}
                  <span class="size-4"></span>
                {/if}
                {move.label}
              </ContextMenu.Item>
            {/each}
          </ContextMenu.SubContent>
        </ContextMenu.Sub>

        {#if railways.length > 1}
          {#if multi || task.repo_id}
            <ContextMenu.Sub>
              <ContextMenu.SubTrigger>
                <TrainFront class="size-4" />
                Lane
              </ContextMenu.SubTrigger>
              <ContextMenu.SubContent class="min-w-44">
                {#if railways.length > LANE_FILTER_THRESHOLD}
                  <!-- Filter box for boards with many lanes (issue #236). It keeps
                       its own keystrokes from the menu's type-ahead. -->
                  <div class="flex items-center gap-1.5 px-1.5 py-1">
                    <Search class="size-3.5 flex-none text-muted-foreground" />
                    <input
                      type="text"
                      placeholder="Filter lanes…"
                      bind:value={laneFilter}
                      onkeydown={onLaneFilterKeydown}
                      class="h-6 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
                    />
                  </div>
                  <ContextMenu.Separator />
                {/if}

                {#if recentRailways.length > 0 && !laneFilter.trim()}
                  <ContextMenu.Label class="text-xs">Recent</ContextMenu.Label>
                  {#each recentRailways as railway (railway.id)}
                    <ContextMenu.Item onclick={() => onMoveToRailway?.(railway.id)}>
                      <TrainFront class="size-4" />
                      {railway.name}
                    </ContextMenu.Item>
                  {/each}
                  <ContextMenu.Separator />
                {/if}

                {#each filteredRailways as railway (railway.id)}
                  {@const isCurrent = !multi && railway.id === task.railway_id}
                  <ContextMenu.Item
                    disabled={isCurrent}
                    onclick={() => onMoveToRailway?.(railway.id)}
                  >
                    {#if isCurrent}
                      <Check class="size-4" />
                    {:else}
                      <span class="size-4"></span>
                    {/if}
                    {railway.name}
                  </ContextMenu.Item>
                {/each}
                {#if filteredRailways.length === 0}
                  <ContextMenu.Label class="text-xs text-muted-foreground">
                    No lanes match
                  </ContextMenu.Label>
                {/if}
              </ContextMenu.SubContent>
            </ContextMenu.Sub>
          {:else}
            <!-- An internal task has no repo to follow between lanes. -->
            <ContextMenu.Item disabled title="Internal tasks have no repo to move between lanes">
              <TrainFront class="size-4" />
              Lane
            </ContextMenu.Item>
          {/if}
        {/if}
      </ContextMenu.SubContent>
    </ContextMenu.Sub>

    <ContextMenu.Separator />

    <ContextMenu.Item onclick={() => onMoveToColumn?.('todo', 'top')}>
      <ArrowUpToLine class="size-4" />
      Work this now
      <ContextMenu.Shortcut>W</ContextMenu.Shortcut>
    </ContextMenu.Item>
    <ContextMenu.Item onclick={() => onToggleHold?.()}>
      {#if task.hold}
        <Play class="size-4" />
        Release hold
      {:else}
        <Pause class="size-4" />
        Hold
      {/if}
      <ContextMenu.Shortcut>H</ContextMenu.Shortcut>
    </ContextMenu.Item>
    <ContextMenu.Item
      disabled={!multi && task.board_column === 'ignored'}
      onclick={() => onMoveToColumn?.('ignored', 'top')}
    >
      <CircleSlash class="size-4" />
      Send to Ignored
    </ContextMenu.Item>

    <!--
      Destructive actions (issue #235): per-card only, so hidden in multi mode
      (bulk reset/delete are not offered here). Visually separated, styled
      destructive, confirmed by the board. Reset is disabled when the task never
      started; Delete is internal-only (source-driven tasks are managed from their
      source).
    -->
    {#if !multi}
      <ContextMenu.Separator />

      <ContextMenu.Item
        variant="destructive"
        disabled={!hasStarted}
        title={hasStarted ? undefined : "This task hasn't started yet, so there is nothing to reset"}
        onclick={() => onReset?.()}
      >
        <RotateCcw class="size-4" />
        Reset task…
      </ContextMenu.Item>
      {#if task.source_kind === 'internal'}
        <ContextMenu.Item variant="destructive" onclick={() => onDelete?.()}>
          <Trash2 class="size-4" />
          Delete…
        </ContextMenu.Item>
      {/if}
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
