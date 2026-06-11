<script lang="ts">
  // Navbar issue search: a fuzzy search over the board's tasks (title + body),
  // with a dropdown of up to 10 results, plus a funnel of filter conditions
  // (author, status, column, created-date range) that narrow the set first. The
  // tasks are passed in from the layout (which already loads the live board), so
  // this is purely client-side, no extra request, and it stays in sync.
  import type { Task } from '$lib/types'

  import { goto } from '$app/navigation'
  import { Search, Filter } from '@lucide/svelte'
  import Fuse from 'fuse.js'

  import { applyFilters, countActiveFilters, emptyFilters } from '$lib/search'
  import SourceIcon from './SourceIcon.svelte'
  import SearchFilterPanel from './SearchFilterPanel.svelte'

  let { tasks = [] }: { tasks?: Task[] } = $props()

  const MAX_RESULTS = 10
  const DEBOUNCE_MS = 200

  let query = $state('')
  // The query that actually drives results, updated on a typing debounce.
  let debounced = $state('')
  // The results dropdown and the funnel panel open independently.
  let open = $state(false)
  let filtersOpen = $state(false)
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let wrapper = $state<HTMLDivElement>()

  // The filter conditions, edited in the funnel panel and applied here.
  let filters = $state(emptyFilters())
  const activeFilterCount = $derived(countActiveFilters(filters))
  const filtersActive = $derived(activeFilterCount > 0)

  // The filters narrow the set before the fuzzy match; with none set this is the
  // whole board.
  const searchable = $derived(applyFilters(tasks, filters))

  const fuse = $derived(
    new Fuse(searchable, {
      keys: [
        { name: 'title', weight: 0.7 },
        { name: 'body_snapshot', weight: 0.3 }
      ],
      threshold: 0.4,
      ignoreLocation: true,
      minMatchCharLength: 2
    })
  )

  const results = $derived.by(() => {
    const term = debounced.trim()
    if (term) {
      return fuse.search(term, { limit: MAX_RESULTS }).map((match) => match.item)
    }
    // No text query but filters are set: browse the filtered issues, newest first.
    if (filtersActive) {
      return [...searchable]
        .sort((a, b) => (a.created_at < b.created_at ? 1 : -1))
        .slice(0, MAX_RESULTS)
    }
    return [] as Task[]
  })

  // Whether there is anything to show in the results dropdown right now. Hidden
  // while the funnel panel is open so the two dropdowns never overlap.
  const showResults = $derived(open && !filtersOpen && (debounced.trim().length > 0 || filtersActive))

  function onInput() {
    open = true
    if (debounceTimer) {
      clearTimeout(debounceTimer)
    }
    debounceTimer = setTimeout(() => (debounced = query), DEBOUNCE_MS)
  }

  function toggleFilters() {
    filtersOpen = !filtersOpen
    // Closing the panel with filters set reveals the now-filtered results.
    if (!filtersOpen && filtersActive) {
      open = true
    }
  }

  function select(task: Task) {
    open = false
    query = ''
    debounced = ''
    goto(`/task/${task.id}`)
  }

  // The issue body collapsed to a single line, newlines stripped, for the preview.
  function oneLine(text: string): string {
    return text.replace(/\s+/g, ' ').trim()
  }

  function onWindowPointerDown(event: MouseEvent) {
    if (wrapper && !wrapper.contains(event.target as Node)) {
      open = false
      filtersOpen = false
    }
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      open = false
      filtersOpen = false
    }
  }
</script>

<svelte:window onmousedown={onWindowPointerDown} onkeydown={onWindowKeydown} />

<div bind:this={wrapper} class="relative w-full max-w-lg">
  <div class="flex items-center gap-1.5">
    <div class="relative flex-1">
      <Search
        class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
      />
      <input
        type="text"
        placeholder="Search issues…"
        bind:value={query}
        oninput={onInput}
        onfocus={() => (open = true)}
        class="w-full rounded-md border border-border bg-background py-1.5 pl-8 pr-3 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
      />
    </div>

    <!-- Funnel: opens the filter panel. The badge counts active conditions. -->
    <button
      type="button"
      onclick={toggleFilters}
      title="Filter issues"
      aria-label="Filter issues"
      aria-expanded={filtersOpen}
      class="relative flex-none rounded-md border p-1.5 transition-colors hover:bg-secondary {filtersActive ||
      filtersOpen
        ? 'border-primary text-primary'
        : 'border-border text-muted-foreground'}"
    >
      <Filter class="size-4" />
      {#if activeFilterCount > 0}
        <span
          class="absolute -right-1.5 -top-1.5 flex size-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold leading-none text-primary-foreground"
        >
          {activeFilterCount}
        </span>
      {/if}
    </button>
  </div>

  {#if showResults}
    <div
      class="absolute left-0 right-0 top-full z-50 mt-1 overflow-hidden rounded-md border border-border bg-card shadow-lg"
    >
      {#if results.length}
        <ul class="divide-y divide-border">
          {#each results as task (task.id)}
            <li>
              <button
                type="button"
                onclick={() => select(task)}
                class="flex w-full items-start gap-2 px-3 py-2 text-left hover:bg-secondary"
              >
                {#if task.author_avatar_url}
                  <img
                    src={task.author_avatar_url}
                    alt={task.author_login ?? ''}
                    class="mt-0.5 size-5 flex-none rounded-full"
                    onerror={(event) => ((event.currentTarget as HTMLImageElement).style.display = 'none')}
                  />
                {:else}
                  <SourceIcon source={task.source_kind} class="mt-0.5 size-5 flex-none text-muted-foreground" />
                {/if}
                <div class="min-w-0 flex-1">
                  <div class="flex items-baseline gap-1.5">
                    <span class="truncate text-sm font-medium">{task.title}</span>
                    <span class="flex-none text-xs text-muted-foreground">#{task.external_id}</span>
                  </div>
                  {#if oneLine(task.body_snapshot)}
                    <div class="truncate text-xs text-muted-foreground">{oneLine(task.body_snapshot)}</div>
                  {/if}
                </div>
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="px-3 py-2 text-sm text-muted-foreground">
          {debounced.trim() ? 'No matching issues' : 'No issues match these filters'}
        </div>
      {/if}
    </div>
  {/if}

  {#if filtersOpen}
    <div class="absolute right-0 top-full z-50 mt-1">
      <SearchFilterPanel {tasks} bind:filters />
    </div>
  {/if}
</div>
