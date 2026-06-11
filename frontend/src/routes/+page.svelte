<script lang="ts">
  import type { DndEvent } from 'svelte-dnd-action'
  import type { Settings, Task, TaskColumn } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { dndzone } from 'svelte-dnd-action'
  import { NotebookPen, RefreshCw, Pause, Play, Eye, EyeOff } from '@lucide/svelte'
  import { PaneGroup } from 'paneforge'

  import { COLUMNS } from '$lib/types'
  import {
    getBoard,
    getNotepad,
    listRepos,
    moveTask,
    provisionWorkspace,
    setNotepad,
    setPaused,
    syncNow
  } from '$lib/api'
  import { isWithinSchedule } from '$lib/schedule'
  import Card from '$lib/components/Card.svelte'
  import Stats from '$lib/components/Stats.svelte'
  import { Button } from '$lib/components/ui/button'
  import { Textarea } from '$lib/components/ui/textarea'
  import * as Alert from '$lib/components/ui/alert'
  import * as Resizable from '$lib/components/ui/resizable'

  const FLIP_MS = 150

  let settings = $state<Settings | null>(null)
  let suggestionCounts = $state<Record<string, number>>({})
  // One array per lane; svelte-dnd-action mutates these during a drag.
  let columns = $state<Record<TaskColumn, Task[]>>({
    available: [],
    todo: [],
    in_progress: [],
    in_review: [],
    done: [],
    ignored: []
  })

  let eventSource: EventSource | null = null
  // Maps a task's repo_id to its full name, so each card can show its source repo.
  let repoNames = $state<Record<string, string>>({})

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

  // Recomputed on every board reload (the SSE stream keeps that frequent enough
  // that the "today" boundary stays fresh while the page is open).
  const doneTodayCount = $derived(columns.done.filter(isDoneToday).length)
  const hasHiddenDone = $derived(columns.done.length > doneTodayCount)

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

  async function load() {
    const [board, repos] = await Promise.all([getBoard(), listRepos()])
    settings = board.settings
    suggestionCounts = board.suggestion_counts
    repoNames = Object.fromEntries(repos.map((repo) => [repo.id, repo.full_name]))
    const grouped: Record<TaskColumn, Task[]> = {
      available: [],
      todo: [],
      in_progress: [],
      in_review: [],
      done: [],
      ignored: []
    }
    for (const task of board.tasks) {
      grouped[task.board_column].push(task)
    }
    for (const key of Object.keys(grouped) as TaskColumn[]) {
      grouped[key].sort((left, right) => left.position - right.position)
    }
    columns = grouped
  }

  function handleConsider(column: TaskColumn, event: CustomEvent<DndEvent<Task>>) {
    columns[column] = event.detail.items
  }

  async function handleFinalize(column: TaskColumn, event: CustomEvent<DndEvent<Task>>) {
    const items = event.detail.items
    columns[column] = items

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

  async function togglePause() {
    if (!settings) {
      return
    }
    settings = await setPaused(!settings.agent_paused)
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
    load()
    // The notepad loads once, separately from the board: the board stream below
    // reloads on every change, which must never clobber an in-progress edit.
    getNotepad()
      .then((result) => (notepad = result.content))
      .catch((error) => console.debug('failed to load notepad', error))
    // Live board: the API ticks this stream whenever anything changes.
    eventSource = new EventSource('/api/v1/board/stream')
    eventSource.addEventListener('board', () => load())
  })

  onDestroy(() => {
    eventSource?.close()
    // Flush a pending notepad edit so leaving the page doesn't drop it.
    if (notepadTimer) {
      void saveNotepad()
    }
  })
</script>

<!--
  Fill the available height (viewport minus the topbar, supplied by `<main>`) as a
  single flex column: the banners and the env-name/action row size to content,
  while the kanban grid takes the remaining space and scrolls within each lane.
  This keeps the page itself from scrolling on desktop, so there is one scrollbar
  (the lanes) instead of the page and the lanes both scrolling. The height cap is
  `lg`-only; on narrow screens the lanes stack and the page scrolls normally.
-->
<div class="flex flex-col lg:h-full lg:min-h-0">
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
    </div>
  </div>

  <!-- Full-width live statistics banner below the action buttons. -->
  <div class="flex-none px-6 pb-2">
    <Stats />
  </div>

  {#snippet kanbanColumns()}
    {#each COLUMNS as column}
      {@const isDone = column.key === 'done'}
      <!-- Done hides items finished before today; toggle to reveal them. Older
           cards are kept in the dnd list (just display:none) so drag-and-drop
           never desyncs from the rendered children. -->
      {@const collapsed = isDone && !showAllDone}
      <section class="flex max-h-full min-h-0 flex-col rounded-lg border border-border bg-card">
        <header
          class="flex items-center justify-between border-b border-border px-3 py-2.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground"
        >
          <div class="flex items-center gap-1.5">
            <span>{column.label}</span>
            {#if isDone && hasHiddenDone}
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
          <span>{collapsed ? doneTodayCount : columns[column.key].length}</span>
        </header>
        <div
          class="flex min-h-[120px] flex-1 flex-col gap-2 overflow-y-auto rounded-b-lg p-3"
          use:dndzone={{
            items: columns[column.key],
            flipDurationMs: FLIP_MS,
            dropTargetStyle: {},
            dropTargetClasses: ['drop-active']
          }}
          onconsider={(event) => handleConsider(column.key, event)}
          onfinalize={(event) => handleFinalize(column.key, event)}
        >
          {#each columns[column.key] as task (task.id)}
            <div class:hidden={collapsed && !isDoneToday(task)}>
              <Card
                {task}
                onchange={load}
                repoName={task.repo_id ? repoNames[task.repo_id] : undefined}
                suggestionCount={suggestionCounts[task.id] ?? 0}
              />
            </div>
          {/each}
        </div>
      </section>
    {/each}
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
        <div
          class="grid h-full min-h-0 grid-cols-1 items-start gap-3 p-4 lg:grid-cols-6 lg:px-6 lg:pb-6"
        >
          {@render kanbanColumns()}
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
    <div
      class="grid grid-cols-1 items-start gap-3 p-4 lg:min-h-0 lg:flex-1 lg:grid-cols-6 lg:px-6 lg:pb-6"
    >
      {@render kanbanColumns()}
    </div>
  {/if}
</div>
