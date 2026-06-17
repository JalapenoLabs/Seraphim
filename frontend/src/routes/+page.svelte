<script lang="ts">
  import type { DndEvent } from 'svelte-dnd-action'
  import type { HeartAttack, Railway, RepoSyncError, Settings, SourceKind, Task, TaskColumn } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { dndzone } from 'svelte-dnd-action'
  import { toast } from 'svelte-sonner'
  import {
    HeartPulse,
    NotebookPen,
    RefreshCw,
    Pause,
    Play,
    Eye,
    EyeOff,
    X,
    ListChecks,
    Filter,
    Check
  } from '@lucide/svelte'
  import { PaneGroup } from 'paneforge'

  import { COLUMNS } from '$lib/types'
  import {
    acknowledgeHeartAttack,
    assignRepoToRailway,
    bulkDeleteTasks,
    bulkSetTaskFields,
    bulkSetTaskStatus,
    extractApiError,
    getBoard,
    getNotepad,
    listRepos,
    moveTask,
    provisionWorkspace,
    setNotepad,
    setPaused,
    setRailwayPaused,
    syncNow
  } from '$lib/api'
  import { isWithinSchedule } from '$lib/schedule'
  import { subscribeBoardStream } from '$lib/boardStream'
  import { type SortKey, sortTasks, loadSort, saveSort } from '$lib/columnSort'
  import BulkActionBar from '$lib/components/BulkActionBar.svelte'
  import Card from '$lib/components/Card.svelte'
  import ColumnSort from '$lib/components/ColumnSort.svelte'
  import RailwayLane from '$lib/components/RailwayLane.svelte'
  import SourceIcon from '$lib/components/SourceIcon.svelte'
  import Stats from '$lib/components/Stats.svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import * as Alert from '$lib/components/ui/alert'
  import * as Resizable from '$lib/components/ui/resizable'

  const FLIP_MS = 150

  let settings = $state<Settings | null>(null)
  let suggestionCounts = $state<Record<string, number>>({})
  // Unacknowledged heart attacks (dead turns) the defibrillator recorded; shown
  // as a dismissible alert banner so the operator notices and can read the logs.
  let heartAttacks = $state<HeartAttack[]>([])
  // Repos whose last issue sync failed (issue #213). The banner persists while a
  // repo is failing; `dismissedSyncRepos` hides one the operator has acknowledged,
  // and a repo is un-dismissed automatically once it recovers (so a later failure
  // surfaces again). The one-time toast is driven separately by the SSE transition.
  let repoSyncErrors = $state<RepoSyncError[]>([])
  let dismissedSyncRepos = new SvelteSet<string>()
  const visibleSyncErrors = $derived(
    repoSyncErrors.filter((repo) => !dismissedSyncRepos.has(repo.full_name))
  )
  // Every railway (swimlane), already ordered `main` first then by rank.
  let railways = $state<Railway[]>([])
  // The board cards, grouped first by railway id, then by column. One array per
  // (railway, column); svelte-dnd-action mutates these arrays during a drag, so a
  // column move stays inside its lane (cross-lane moves are repo reassignments,
  // never a card drag).
  let columnsByRailway = $state<Record<string, Record<TaskColumn, Task[]>>>({})

  // A fresh, empty set of column buckets for one lane.
  function emptyColumns(): Record<TaskColumn, Task[]> {
    return {
      available: [],
      todo: [],
      in_progress: [],
      in_review: [],
      done: [],
      ignored: []
    }
  }

  // The buckets for a lane, creating an empty set on first access so a brand-new
  // railway (with no cards yet) still renders all its columns.
  function laneColumns(railwayId: string): Record<TaskColumn, Task[]> {
    return columnsByRailway[railwayId] ?? emptyColumns()
  }

  // Unsubscribe from the shared board stream (the single EventSource the page, its
  // stats banner, and every lane stats strip all fan out from).
  let unsubscribeBoard: (() => void) | null = null
  // Maps a task's repo_id to its full name, so each card can show its source repo.
  let repoNames = $state<Record<string, string>>({})

  // --- Board filters (repo + created-date range) -----------------------------
  // A view-only filter over the whole board: non-matching cards are hidden (kept
  // in the dnd lists, like the Done collapse, so drag-and-drop never desyncs).
  // Lives only in the browser; it never changes what the agent picks up.
  let filtersOpen = $state(false)
  let filterRepoIds = new SvelteSet<string>()
  let filterSourceKinds = new SvelteSet<SourceKind>()
  let filterCreatedAfter = $state('') // inclusive YYYY-MM-DD, or '' for unset
  let filterCreatedBefore = $state('') // inclusive YYYY-MM-DD, or '' for unset

  // Repo choices for the drawer, sorted by name.
  const repoOptions = $derived(
    Object.entries(repoNames)
      .map(([id, full_name]) => ({ id, full_name }))
      .sort((left, right) => left.full_name.localeCompare(right.full_name))
  )

  // The source kinds, in a stable display order, restricted to those actually
  // present on the board so we never offer (say) Jira before any Jira card exists.
  const SOURCE_LABELS = { github: 'GitHub', jira: 'Jira', internal: 'Internal' } as const
  const sourceOptions = $derived.by(() => {
    const present = new Set<SourceKind>()
    for (const buckets of Object.values(columnsByRailway)) {
      for (const tasks of Object.values(buckets)) {
        for (const task of tasks) {
          present.add(task.source_kind)
        }
      }
    }
    return (['github', 'jira', 'internal'] as const)
      .filter((kind) => present.has(kind))
      .map((kind) => ({ kind, label: SOURCE_LABELS[kind] }))
  })

  const activeFilterCount = $derived(
    filterRepoIds.size +
      filterSourceKinds.size +
      (filterCreatedAfter ? 1 : 0) +
      (filterCreatedBefore ? 1 : 0)
  )
  const filterActive = $derived(activeFilterCount > 0)

  // Whether a task passes the active filters. The repo and source filters are each
  // an OR within their own selected values and an AND across dimensions; the date
  // bounds are inclusive of the chosen calendar days.
  function matchesFilter(task: Task): boolean {
    if (filterRepoIds.size > 0 && !(task.repo_id && filterRepoIds.has(task.repo_id))) {
      return false
    }
    if (filterSourceKinds.size > 0 && !filterSourceKinds.has(task.source_kind)) {
      return false
    }
    if (filterCreatedAfter && new Date(task.created_at) < new Date(`${filterCreatedAfter}T00:00:00`)) {
      return false
    }
    if (filterCreatedBefore && new Date(task.created_at) > new Date(`${filterCreatedBefore}T23:59:59.999`)) {
      return false
    }
    return true
  }

  function toggleRepoFilter(id: string) {
    if (filterRepoIds.has(id)) {
      filterRepoIds.delete(id)
    } else {
      filterRepoIds.add(id)
    }
  }

  function toggleSourceFilter(kind: SourceKind) {
    if (filterSourceKinds.has(kind)) {
      filterSourceKinds.delete(kind)
    } else {
      filterSourceKinds.add(kind)
    }
  }

  function clearFilters() {
    filterRepoIds.clear()
    filterSourceKinds.clear()
    filterCreatedAfter = ''
    filterCreatedBefore = ''
  }

  // --- Multi-select (bulk edit) ----------------------------------------------
  // In bulk mode a click selects a card instead of opening it; the floating
  // BulkActionBar then edits the whole selection at once.
  let bulkMode = $state(false)
  let selected = new SvelteSet<string>()
  // True while the bulk bar has a modal/menu open, so Escape closes that first
  // rather than exiting bulk mode out from under it.
  let bulkDialogOpen = $state(false)

  function enterBulkMode() {
    bulkMode = true
  }

  // Clear the selection and leave bulk mode (the bar's X and the Escape key).
  function exitBulkMode() {
    bulkMode = false
    selected.clear()
  }

  function toggleSelected(id: string) {
    if (selected.has(id)) {
      selected.delete(id)
    } else {
      selected.add(id)
    }
  }

  // Clicking a column header in bulk mode selects every card in that lane's
  // column, or clears them when all are already selected (a partial fills in).
  function toggleColumnSelected(railwayId: string, column: TaskColumn) {
    const ids = laneColumns(railwayId)[column].map((task) => task.id)
    const allSelected = ids.length > 0 && ids.every((id) => selected.has(id))
    for (const id of ids) {
      if (allSelected) {
        selected.delete(id)
      } else {
        selected.add(id)
      }
    }
  }

  async function applyBulkFields(fields: { hold?: boolean; blocking?: boolean }) {
    const ids = [...selected]
    await bulkSetTaskFields(ids, fields)
    await load()
    toast.success(`Updated ${ids.length} ${ids.length === 1 ? 'task' : 'tasks'}`)
  }

  async function applyBulkStatus(column: TaskColumn) {
    const ids = [...selected]
    await bulkSetTaskStatus(ids, column)
    await load()
    toast.success(`Moved ${ids.length} ${ids.length === 1 ? 'task' : 'tasks'}`)
  }

  async function applyBulkDelete() {
    const ids = [...selected]
    await bulkDeleteTasks(ids)
    selected.clear()
    await load()
    toast.success(`Deleted ${ids.length} ${ids.length === 1 ? 'task' : 'tasks'}`)
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && filtersOpen) {
      filtersOpen = false
      return
    }
    if (event.key === 'Escape' && bulkMode && !bulkDialogOpen) {
      exitBulkMode()
    }
  }

  // Per-column sort level (default "custom" = the board's manual order), shared
  // across every lane's instance of that column. Hydrated from session storage on
  // mount; the header sort button changes it.
  let sortState = $state<Record<TaskColumn, SortKey>>({
    available: 'custom',
    todo: 'custom',
    in_progress: 'custom',
    in_review: 'custom',
    done: 'custom',
    ignored: 'custom'
  })

  // Re-sorts that column in every lane when its sort level changes, and persists
  // the choice. The sort is applied imperatively (here and in `load`), never
  // reactively, so it never fights svelte-dnd-action mid-drag.
  function changeSort(column: TaskColumn, next: SortKey) {
    sortState[column] = next
    saveSort(column, next)
    for (const railwayId of Object.keys(columnsByRailway)) {
      columnsByRailway[railwayId][column] = sortTasks(columnsByRailway[railwayId][column], next)
    }
  }

  // The Done column hides older items by default so it doesn't grow without
  // bound: only tasks finished today are shown until the user reveals the rest
  // with the eyeball toggle in the column header.
  let showAllDone = $state(false)

  // A task counts as "done today" if its completion stamp falls on the current
  // local calendar day. `finished_at` is set on auto-completion; a card dragged
  // to Done manually has none, so fall back to `updated_at` (the move time).
  function isDoneToday(task: Task): boolean {
    const stamp = task.finished_at ?? task.updated_at
    return !!stamp && new Date(stamp).toDateString() === new Date().toDateString()
  }

  // The Done-column counts for one lane, honoring the active board filter so the
  // header + reveal toggle reflect what's actually shown.
  function laneMatchingDone(railwayId: string): Task[] {
    return laneColumns(railwayId).done.filter(matchesFilter)
  }
  function laneDoneTodayCount(railwayId: string): number {
    return laneMatchingDone(railwayId).filter(isDoneToday).length
  }
  function laneHasHiddenDone(railwayId: string): boolean {
    return laneMatchingDone(railwayId).length > laneDoneTodayCount(railwayId)
  }

  // How many cards a lane shows (across all columns) under the active filter, for
  // the lane header's card count.
  function laneVisibleCount(railwayId: string): number {
    const buckets = laneColumns(railwayId)
    let total = 0
    for (const column of COLUMNS) {
      total += buckets[column.key].filter(matchesFilter).length
    }
    return total
  }

  // --- Global notepad --------------------------------------------------------
  // A scratchpad in a resizable pane beside the board. Default collapsed; the
  // user's open/closed choice and the text both persist.
  const NOTEPAD_OPEN_KEY = 'seraphim.notepadOpen'

  function readNotepadOpen(): boolean {
    return typeof localStorage !== 'undefined' && localStorage.getItem(NOTEPAD_OPEN_KEY) === 'true'
  }

  let notesOpen = $state(readNotepadOpen())
  let notepad = $state('')
  let notepadStatus = $state<'idle' | 'saving' | 'saved'>('idle')
  let notepadTimer: ReturnType<typeof setTimeout> | null = null

  function setNotesOpen(open: boolean) {
    notesOpen = open
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(NOTEPAD_OPEN_KEY, String(open))
    }
  }

  function scheduleNotepadSave() {
    notepadStatus = 'saving'
    if (notepadTimer) {
      clearTimeout(notepadTimer)
    }
    notepadTimer = setTimeout(saveNotepad, 700)
  }

  async function saveNotepad() {
    if (notepadTimer) {
      clearTimeout(notepadTimer)
      notepadTimer = null
    }
    try {
      await setNotepad(notepad)
      notepadStatus = 'saved'
    } catch (error) {
      console.debug('failed to save notepad', error)
      notepadStatus = 'idle'
    }
  }

  // The display name of the railway a card / incident belongs to, for the lane
  // banners. Falls back to "main" when the lane is not (yet) known.
  function railwayName(railwayId: string | null | undefined): string {
    if (!railwayId) return 'main'
    return railways.find((railway) => railway.id === railwayId)?.name ?? 'main'
  }

  // The railway one heart-attack belongs to, looked up via its task. Null when the
  // task is gone or the lane is unknown, so the banner simply omits the tag.
  function heartAttackRailwayName(incident: HeartAttack): string | null {
    if (!incident.task_id) return null
    for (const railway of railways) {
      const buckets = columnsByRailway[railway.id]
      if (!buckets) continue
      for (const column of COLUMNS) {
        if (buckets[column.key].some((task) => task.id === incident.task_id)) {
          return railway.name
        }
      }
    }
    return null
  }

  async function load() {
    const [board, repos] = await Promise.all([getBoard(), listRepos()])
    settings = board.settings
    railways = board.railways
    suggestionCounts = board.suggestion_counts
    heartAttacks = board.heart_attacks
    repoSyncErrors = board.repo_sync_errors
    // Forget a dismissal once its repo is no longer failing, so a fresh failure on
    // that repo later raises the banner again instead of staying hidden.
    const failing = new Set(repoSyncErrors.map((repo) => repo.full_name))
    for (const name of dismissedSyncRepos) {
      if (!failing.has(name)) {
        dismissedSyncRepos.delete(name)
      }
    }
    repoNames = Object.fromEntries(repos.map((repo) => [repo.id, repo.full_name]))

    // Group every card by railway, then by column. A lane with no cards still gets
    // an empty bucket set so all its swimlane columns render.
    const grouped: Record<string, Record<TaskColumn, Task[]>> = {}
    for (const railway of board.railways) {
      grouped[railway.id] = emptyColumns()
    }
    for (const task of board.tasks) {
      // Defensive: a card whose lane is not in the railway list (mid-migration)
      // still needs a home, so create its bucket on the fly.
      const buckets = grouped[task.railway_id] ?? (grouped[task.railway_id] = emptyColumns())
      buckets[task.board_column].push(task)
    }
    for (const buckets of Object.values(grouped)) {
      for (const column of COLUMNS) {
        // Base order is the manual (position) order; then apply the column's sort
        // on top (a no-op for "custom").
        buckets[column.key].sort((left, right) => left.position - right.position)
        buckets[column.key] = sortTasks(buckets[column.key], sortState[column.key])
      }
    }
    columnsByRailway = grouped

    // Drop any selected ids that no longer exist (deleted or closed elsewhere),
    // so the bulk bar's count stays truthful across live board refreshes.
    if (bulkMode && selected.size > 0) {
      const present = new Set(board.tasks.map((task) => task.id))
      for (const id of selected) {
        if (!present.has(id)) {
          selected.delete(id)
        }
      }
    }
  }

  // Clears a heart-attack alert once the operator has read it. Optimistic: drop
  // it locally first so the banner feels instant, then persist.
  async function dismissHeartAttack(id: string) {
    heartAttacks = heartAttacks.filter((incident) => incident.id !== id)
    try {
      await acknowledgeHeartAttack(id)
    } catch (error) {
      console.debug('failed to acknowledge heart attack', error)
    }
  }

  function handleConsider(railwayId: string, column: TaskColumn, event: CustomEvent<DndEvent<Task>>) {
    columnsByRailway[railwayId][column] = event.detail.items
  }

  async function handleFinalize(
    railwayId: string,
    column: TaskColumn,
    event: CustomEvent<DndEvent<Task>>
  ) {
    const items = event.detail.items
    columnsByRailway[railwayId][column] = items

    const movedId = event.detail.info.id
    const index = items.findIndex((task) => task.id === movedId)
    // Finalize fires on both zones; only the destination contains the card.
    if (index === -1) {
      return
    }

    await moveTask(movedId, column, computePosition(items, index))
    await load()
  }

  // Fractional rank: drop between neighbors by taking their midpoint.
  function computePosition(items: Task[], index: number): number {
    const previous = items[index - 1]?.position
    const next = items[index + 1]?.position
    if (previous !== undefined && next !== undefined) {
      return (previous + next) / 2
    }
    if (next !== undefined) {
      return next - 1
    }
    if (previous !== undefined) {
      return previous + 1
    }
    return 1
  }

  // Reassign a card's repo (and all its tasks) to another railway. Cross-lane is a
  // repo move, not a card drag, so this is the explicit control behind the card's
  // "move lane" menu. The backend blocks it while a live turn is working the repo
  // on its current lane and returns the reason, which we surface in a toast.
  async function moveTaskToRailway(task: Task, railwayId: string) {
    if (!task.repo_id) {
      return
    }
    const repoLabel = repoNames[task.repo_id] ?? 'repo'
    try {
      await assignRepoToRailway(railwayId, task.repo_id)
      await load()
      toast.success(`Moved ${repoLabel} to ${railwayName(railwayId)}`)
    } catch (error) {
      const message = await extractApiError(error, 'Failed to move the repo to that railway.')
      toast.error(message)
    }
  }

  async function togglePause() {
    if (!settings) {
      return
    }
    settings = await setPaused(!settings.agent_paused)
  }

  // Toggle one lane's per-railway pause (independent of the global master pause).
  async function toggleRailwayPause(railway: Railway) {
    try {
      const updated = await setRailwayPaused(railway.id, !railway.paused)
      railways = railways.map((existing) => (existing.id === updated.id ? updated : existing))
    } catch (error) {
      console.debug('failed to toggle railway pause', error)
      toast.error('Failed to update the railway pause.')
    }
  }

  // True when the agent is enabled but the schedule currently holds it idle, so
  // the board can explain why nothing is being picked up. Recomputed on every
  // board reload (the SSE stream keeps that frequent enough).
  const outsideSchedule = $derived(
    !!settings &&
      !settings.agent_paused &&
      settings.availability_enabled &&
      !isWithinSchedule(settings, new Date())
  )

  let checking = $state(false)
  async function checkIssues() {
    checking = true
    try {
      await syncNow()
      await load()
    } finally {
      checking = false
    }
  }

  let retrying = $state(false)
  async function retryProvision() {
    retrying = true
    try {
      await provisionWorkspace()
    } catch {
      // The error is reflected back via settings.config_repo_error on reload.
    } finally {
      await load()
      retrying = false
    }
  }

  onMount(() => {
    // Hydrate each column's saved sort before the first load so it applies at once.
    for (const column of COLUMNS) {
      sortState[column.key] = loadSort(column.key)
    }
    load()
    // The notepad loads once, separately from the board: the board stream below
    // reloads on every change, which must never clobber an in-progress edit.
    getNotepad()
      .then((result) => (notepad = result.content))
      .catch((error) => console.debug('failed to load notepad', error))
    // Live board: the API ticks this stream whenever anything changes. The same
    // single shared connection feeds the stats banner and lane stats strips.
    unsubscribeBoard = subscribeBoardStream({ board: () => load() })
  })

  onDestroy(() => {
    unsubscribeBoard?.()
    // Flush a pending notepad edit so leaving the page doesn't drop it.
    if (notepadTimer) {
      void saveNotepad()
    }
  })
</script>

<!--
  Fill the available height (viewport minus the topbar, supplied by `<main>`) as a
  single flex column: the banners and the env-name/action row size to content,
  while the swimlane stack takes the remaining space and scrolls. Each lane holds
  its own kanban columns row; the lanes are stacked vertically (the board scrolls
  as a whole rather than each column scrolling independently).
-->
<svelte:window onkeydown={onWindowKeydown} />

<div class="flex flex-col lg:h-full lg:min-h-0">
  {#each heartAttacks as incident (incident.id)}
    {@const lane = heartAttackRailwayName(incident)}
    <!-- A turn died and the defibrillator handled it. Keep the diagnostic detail
         visible (monospaced) so the operator can patch the underlying cause, with
         a dismiss once they have read it. The banner stays global but is tagged
         with the railway it belongs to. -->
    <Alert.Root variant="destructive" class="mx-6 mt-4 flex items-start justify-between gap-4">
      <div class="min-w-0">
        <Alert.Title class="flex items-center gap-1.5">
          <HeartPulse class="size-4 flex-none" />
          Agent heart attack: "{incident.task_title}"
          {#if lane}
            <span class="rounded border border-current/40 px-1.5 py-0 text-[10px] font-normal opacity-80">
              {lane}
            </span>
          {/if}
        </Alert.Title>
        <Alert.Description class="break-words">
          <span class="font-mono text-xs break-words">{incident.detail}</span>
          {#if incident.recovery}
            <span class="mt-1 block text-xs opacity-80">{incident.recovery}</span>
          {/if}
          {#if incident.task_id}
            <a href={`/task/${incident.task_id}`} class="mt-1 inline-block text-xs underline">
              Open the task to see the full activity log
            </a>
          {/if}
        </Alert.Description>
      </div>
      <Button
        variant="outline"
        size="icon"
        class="flex-none"
        title="Dismiss"
        aria-label="Dismiss heart attack"
        onclick={() => dismissHeartAttack(incident.id)}
      >
        <X class="size-4" />
      </Button>
    </Alert.Root>
  {/each}

  {#if settings?.config_repo_error}
    <Alert.Root variant="destructive" class="mx-6 mt-4 flex items-center justify-between gap-4">
      <div>
        <Alert.Title>Config repo (~/.claude) failed to set up — the agent is halted.</Alert.Title>
        <Alert.Description class="font-mono text-xs break-words">
          {settings.config_repo_error}
        </Alert.Description>
      </div>
      <Button variant="outline" size="sm" disabled={retrying} onclick={retryProvision}>
        {retrying ? 'Retrying…' : 'Retry'}
      </Button>
    </Alert.Root>
  {/if}

  {#each visibleSyncErrors as repo (repo.full_name)}
    <!-- A repo's issue sync is failing (issue #213). Persist the reason until it
         recovers (it clears itself on the next successful sync), with a dismiss for
         operators who have read it. -->
    <Alert.Root variant="destructive" class="mx-6 mt-4 flex items-start justify-between gap-4">
      <div class="min-w-0">
        <Alert.Title class="flex items-center gap-1.5">
          <RefreshCw class="size-4 flex-none" />
          Issue sync failed: {repo.full_name}
        </Alert.Title>
        <Alert.Description class="break-words">
          {repo.sync_error}
        </Alert.Description>
      </div>
      <Button
        variant="outline"
        size="icon"
        class="flex-none"
        title="Dismiss"
        aria-label="Dismiss sync error"
        onclick={() => dismissedSyncRepos.add(repo.full_name)}
      >
        <X class="size-4" />
      </Button>
    </Alert.Root>
  {/each}

  {#if settings?.usage_paused_until && new Date(settings.usage_paused_until).getTime() > Date.now()}
    <Alert.Root class="mx-6 mt-4 border-warning/40">
      <Alert.Title>Paused: subscription usage limit reached.</Alert.Title>
      <Alert.Description>
        New work is on hold until the usage window resets at
        {new Date(settings.usage_paused_until).toLocaleString()}. The agent resumes automatically.
      </Alert.Description>
    </Alert.Root>
  {/if}

  <div class="flex items-center justify-between px-6 pb-1 pt-4">
    <div class="flex items-baseline gap-2">
      {#if settings}
        <strong class="text-base">{settings.org_name}</strong>
        {#if outsideSchedule}
          <span
            class="rounded-full border border-warning/40 px-2 py-0.5 text-xs text-warning"
            title="Outside the availability schedule"
          >
            ⏰ Outside scheduled hours
          </span>
        {/if}
      {/if}
    </div>
    <div class="flex gap-2">
      <Button
        variant={notesOpen ? 'default' : 'outline'}
        size="icon"
        title={notesOpen ? 'Close notes' : 'Show notes'}
        aria-label={notesOpen ? 'Close notes' : 'Show notes'}
        onclick={() => setNotesOpen(!notesOpen)}
      >
        <NotebookPen class="size-4" />
      </Button>
      <Button variant="outline" size="sm" disabled={checking} onclick={checkIssues}>
        <RefreshCw class="size-4 {checking ? 'animate-spin' : ''}" />
        {checking ? 'Checking…' : 'Check issues'}
      </Button>
      <Button
        variant={filterActive ? 'default' : 'outline'}
        size="sm"
        title="Filter tasks"
        aria-label="Filter tasks"
        onclick={() => (filtersOpen = true)}
      >
        <Filter class="size-4" />
        Filters{#if filterActive}
          <span class="ml-0.5">({activeFilterCount})</span>
        {/if}
      </Button>
      {#if settings}
        <Button
          variant={settings.agent_paused ? 'default' : 'outline'}
          size="sm"
          onclick={togglePause}
        >
          {#if settings.agent_paused}
            <Play class="size-4" /> Resume agent
          {:else}
            <Pause class="size-4" /> Pause agent
          {/if}
        </Button>
      {/if}
      <Button
        variant={bulkMode ? 'default' : 'outline'}
        size="sm"
        onclick={() => (bulkMode ? exitBulkMode() : enterBulkMode())}
      >
        <ListChecks class="size-4" />
        {bulkMode ? 'Done' : 'Bulk edit'}
      </Button>
    </div>
  </div>

  <!-- Full-width live statistics banner: the shared subscription usage gauge plus
       the global aggregate cost / tokens / time rollup across every railway. -->
  <div class="flex-none px-6 pb-2">
    <Stats />
  </div>

  <!-- The kanban columns for one lane. Parameterized by railway id so each lane's
       dnd zones hold only that lane's cards: a column drop stays in the lane, and
       cross-lane moves go through the card's "move lane" reassign control. -->
  {#snippet kanbanColumns(railwayId: string)}
    {@const buckets = laneColumns(railwayId)}
    {#each COLUMNS as column}
      {@const isDone = column.key === 'done'}
      <!-- Done hides items finished before today; toggle to reveal them. Older
           cards are kept in the dnd list (just display:none) so drag-and-drop
           never desyncs from the rendered children. -->
      {@const collapsed = isDone && !showAllDone}
      {@const columnSelected = buckets[column.key].filter((task) => selected.has(task.id)).length}
      <section class="flex max-h-full min-h-0 flex-col rounded-lg border border-border bg-card">
        <header
          class="flex items-center justify-between border-b border-border px-3 py-2.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground"
        >
          <div class="flex items-center gap-1.5">
            {#if bulkMode}
              <!-- In bulk mode the lane title selects (or clears) the whole column. -->
              <button
                type="button"
                onclick={() => toggleColumnSelected(railwayId, column.key)}
                title={`Select all in ${column.label}`}
                class="-mx-1 rounded px-1 uppercase tracking-wide transition-colors hover:bg-secondary hover:text-foreground {columnSelected >
                0
                  ? 'text-primary'
                  : ''}"
              >
                {column.label}{#if columnSelected > 0}<span class="ml-1 normal-case"
                    >({columnSelected})</span
                  >{/if}
              </button>
            {:else}
              <span>{column.label}</span>
            {/if}
            {#if isDone && laneHasHiddenDone(railwayId)}
              <button
                type="button"
                onclick={() => (showAllDone = !showAllDone)}
                title={showAllDone ? 'Show only today' : 'View all'}
                aria-label={showAllDone ? 'Show only today' : 'View all'}
                aria-pressed={showAllDone}
                class="rounded p-0.5 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
              >
                {#if showAllDone}
                  <EyeOff class="size-3.5" />
                {:else}
                  <Eye class="size-3.5" />
                {/if}
              </button>
            {/if}
          </div>
          <div class="flex items-center gap-1.5">
            <ColumnSort
              value={sortState[column.key]}
              onchange={(next) => changeSort(column.key, next)}
              label={column.label}
            />
            <span>
              {collapsed
                ? laneDoneTodayCount(railwayId)
                : buckets[column.key].filter(matchesFilter).length}
            </span>
          </div>
        </header>
        <div
          class="flex min-h-[120px] flex-1 flex-col gap-2 overflow-y-auto rounded-b-lg p-3 {bulkMode
            ? 'pb-24'
            : ''}"
          use:dndzone={{
            items: buckets[column.key],
            flipDurationMs: FLIP_MS,
            dropTargetStyle: {},
            dropTargetClasses: ['drop-active'],
            dragDisabled: bulkMode
          }}
          onconsider={(event) => handleConsider(railwayId, column.key, event)}
          onfinalize={(event) => handleFinalize(railwayId, column.key, event)}
        >
          {#each buckets[column.key] as task (task.id)}
            <div class:hidden={(collapsed && !isDoneToday(task)) || !matchesFilter(task)}>
              <Card
                {task}
                onchange={load}
                repoName={task.repo_id ? repoNames[task.repo_id] : undefined}
                suggestionCount={suggestionCounts[task.id] ?? 0}
                selectionMode={bulkMode}
                selected={selected.has(task.id)}
                onselect={() => toggleSelected(task.id)}
                {railways}
                onMoveToRailway={(targetRailwayId) => moveTaskToRailway(task, targetRailwayId)}
              />
            </div>
          {/each}
        </div>
      </section>
    {/each}
  {/snippet}

  <!-- The swimlane stack: one lane per railway, `main` first then by rank. Each
       lane frames its own kanban columns row. A board with only `main` renders as
       a single lane, so the layout degrades to today's board through one lane. -->
  {#snippet swimlanes()}
    <div
      class="flex flex-col gap-4 p-4 lg:px-6 lg:pb-6 {bulkMode
        ? 'rounded-lg ring-1 ring-inset ring-primary/40'
        : ''}"
    >
      {#each railways as railway (railway.id)}
        <RailwayLane
          {railway}
          taskCount={laneVisibleCount(railway.id)}
          onTogglePause={() => toggleRailwayPause(railway)}
        >
          <div class="grid grid-cols-1 items-start gap-3 lg:grid-cols-6">
            {@render kanbanColumns(railway.id)}
          </div>
        </RailwayLane>
      {/each}
    </div>
  {/snippet}

  {#if notesOpen}
    <!--
      Board + notepad, split by a drag bar. Dragging the notepad fully to the
      right collapses it past its min size, which hides it entirely and hands the
      whole width back to the board (same as clicking "Close notes").
    -->
    <PaneGroup
      direction="horizontal"
      class="flex h-[70vh] w-full overflow-hidden lg:h-auto lg:min-h-0 lg:flex-1"
    >
      <Resizable.Pane defaultSize={72} minSize={40} class="min-w-0">
        <div class="h-full min-h-0 overflow-y-auto">
          {@render swimlanes()}
        </div>
      </Resizable.Pane>

      <Resizable.Handle
        withHandle
        class="w-1.5 bg-border transition-colors hover:bg-primary data-[active]:bg-primary"
      />

      <Resizable.Pane
        defaultSize={28}
        minSize={18}
        collapsible
        collapsedSize={0}
        onCollapse={() => setNotesOpen(false)}
        class="min-w-0"
      >
        <div class="h-full py-4 pl-2 pr-6">
          <div class="flex h-full min-w-0 flex-col rounded-lg border border-border bg-card">
            <header
              class="flex flex-none items-center justify-between gap-2 border-b border-border px-4 py-2.5"
            >
              <span class="text-xs uppercase tracking-wide text-muted-foreground">Notepad</span>
              <span class="text-xs text-muted-foreground">
                {#if notepadStatus === 'saving'}Saving…{:else if notepadStatus === 'saved'}Saved{/if}
              </span>
            </header>
            <Textarea
              bind:value={notepad}
              oninput={scheduleNotepadSave}
              onblur={saveNotepad}
              placeholder="A global scratchpad for anything…"
              class="min-h-0 flex-1 resize-none rounded-none rounded-b-lg border-0 bg-transparent text-sm focus-visible:ring-0 focus-visible:ring-offset-0"
            />
          </div>
        </div>
      </Resizable.Pane>
    </PaneGroup>
  {:else}
    <div class="lg:min-h-0 lg:flex-1 lg:overflow-y-auto">
      {@render swimlanes()}
    </div>
  {/if}

  {#if bulkMode}
    <BulkActionBar
      count={selected.size}
      bind:dialogOpen={bulkDialogOpen}
      onClear={exitBulkMode}
      onEditFields={applyBulkFields}
      onChangeStatus={applyBulkStatus}
      onDelete={applyBulkDelete}
    />
  {/if}

  {#if filtersOpen}
    <!-- Filters drawer: a right-side modal over a dimmed backdrop. View-only;
         it hides non-matching cards rather than changing the board itself. -->
    <div
      class="fixed inset-0 z-40 bg-black/40"
      role="presentation"
      onclick={() => (filtersOpen = false)}
    ></div>
    <aside
      class="fixed right-0 top-0 z-50 flex h-full w-80 max-w-[90vw] flex-col border-l border-border bg-card shadow-xl"
      aria-label="Board filters"
    >
      <header class="flex flex-none items-center justify-between border-b border-border px-4 py-3">
        <strong class="text-sm">Filters</strong>
        <Button variant="ghost" size="icon" title="Close filters" aria-label="Close filters" onclick={() => (filtersOpen = false)}>
          <X class="size-4" />
        </Button>
      </header>

      <div class="flex-1 space-y-5 overflow-y-auto p-4">
        <Button
          variant="outline"
          size="sm"
          class="w-full"
          disabled={!filterActive}
          onclick={clearFilters}
        >
          Clear filters
        </Button>

        <div>
          <Label class="text-xs uppercase tracking-wide text-muted-foreground">Repositories</Label>
          <div class="mt-2 space-y-1">
            {#each repoOptions as repo (repo.id)}
              {@const checked = filterRepoIds.has(repo.id)}
              <button
                type="button"
                onclick={() => toggleRepoFilter(repo.id)}
                aria-pressed={checked}
                class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-secondary"
              >
                <span
                  class="flex size-4 flex-none items-center justify-center rounded border {checked
                    ? 'border-primary bg-primary text-primary-foreground'
                    : 'border-input'}"
                >
                  {#if checked}<Check class="size-3" />{/if}
                </span>
                <span class="truncate">{repo.full_name}</span>
              </button>
            {/each}
            {#if repoOptions.length === 0}
              <p class="text-sm text-muted-foreground">No repositories.</p>
            {/if}
          </div>
        </div>

        <div>
          <Label class="text-xs uppercase tracking-wide text-muted-foreground">Source</Label>
          <div class="mt-2 space-y-1">
            {#each sourceOptions as source (source.kind)}
              {@const checked = filterSourceKinds.has(source.kind)}
              <button
                type="button"
                onclick={() => toggleSourceFilter(source.kind)}
                aria-pressed={checked}
                class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-secondary"
              >
                <span
                  class="flex size-4 flex-none items-center justify-center rounded border {checked
                    ? 'border-primary bg-primary text-primary-foreground'
                    : 'border-input'}"
                >
                  {#if checked}<Check class="size-3" />{/if}
                </span>
                <SourceIcon source={source.kind} class="size-4 flex-none" />
                <span class="truncate">{source.label}</span>
              </button>
            {/each}
            {#if sourceOptions.length === 0}
              <p class="text-sm text-muted-foreground">No cards.</p>
            {/if}
          </div>
        </div>

        <div class="space-y-1.5">
          <Label for="filter-created-after" class="text-xs uppercase tracking-wide text-muted-foreground">
            Created after
          </Label>
          <Input
            id="filter-created-after"
            type="date"
            bind:value={filterCreatedAfter}
            max={filterCreatedBefore || undefined}
          />
        </div>

        <div class="space-y-1.5">
          <Label for="filter-created-before" class="text-xs uppercase tracking-wide text-muted-foreground">
            Created before
          </Label>
          <Input
            id="filter-created-before"
            type="date"
            bind:value={filterCreatedBefore}
            min={filterCreatedAfter || undefined}
          />
        </div>
      </div>
    </aside>
  {/if}
</div>
