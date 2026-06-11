<script lang="ts">
  // Navbar issue search: a fuzzy search over the board's tasks (title + body),
  // with a dropdown of up to 10 results. The tasks are passed in from the layout
  // (which already loads the live board), so this is purely client-side, no extra
  // request, and it stays in sync with the board.
  import type { Task } from '$lib/types'

  import { goto } from '$app/navigation'
  import { Search } from '@lucide/svelte'
  import Fuse from 'fuse.js'

  import SourceIcon from './SourceIcon.svelte'

  let { tasks = [] }: { tasks?: Task[] } = $props()

  const MAX_RESULTS = 10
  const DEBOUNCE_MS = 200

  let query = $state('')
  // The query that actually drives results, updated on a typing debounce.
  let debounced = $state('')
  let open = $state(false)
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let wrapper = $state<HTMLDivElement>()

  // Future filters (by author, status, date created, ...) will narrow this set
  // before the fuzzy match; the search itself stays the same.
  const searchable = $derived(tasks)

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
    if (!term) {
      return [] as Task[]
    }
    return fuse.search(term, { limit: MAX_RESULTS }).map((match) => match.item)
  })

  function onInput() {
    open = true
    if (debounceTimer) {
      clearTimeout(debounceTimer)
    }
    debounceTimer = setTimeout(() => (debounced = query), DEBOUNCE_MS)
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
    }
  }
</script>

<svelte:window onmousedown={onWindowPointerDown} />

<div bind:this={wrapper} class="relative w-full max-w-md">
  <div class="relative">
    <Search
      class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
    />
    <input
      type="text"
      placeholder="Search issues…"
      bind:value={query}
      oninput={onInput}
      onfocus={() => (open = true)}
      onkeydown={(event) => {
        if (event.key === 'Escape') {
          open = false
        }
      }}
      class="w-full rounded-md border border-border bg-background py-1.5 pl-8 pr-3 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
    />
  </div>

  {#if open && debounced.trim()}
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
        <div class="px-3 py-2 text-sm text-muted-foreground">No matching issues</div>
      {/if}
    </div>
  {/if}
</div>
