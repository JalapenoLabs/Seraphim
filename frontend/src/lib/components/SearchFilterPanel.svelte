<script lang="ts">
  // The funnel dropdown: the filter conditions for the navbar search, grouped
  // into sections (authors, statuses, columns, created-date range). It owns no
  // search logic; it just edits the bound `filters`, which the SearchBar applies.
  import type { Task, TaskColumn, TaskStatus } from '$lib/types'

  import { COLUMNS, STATUS_LABELS } from '$lib/types'
  import { countActiveFilters, distinctAuthors, emptyFilters, type SearchFilters } from '$lib/search'
  import { Input } from './ui/input'

  let {
    tasks = [],
    filters = $bindable()
  }: {
    tasks?: Task[]
    filters: SearchFilters
  } = $props()

  // Authors are data-driven (only those who actually opened an issue); statuses
  // and columns are the fixed enums, shown in full so the choices are stable.
  const authors = $derived(distinctAuthors(tasks))
  const statusOptions = Object.entries(STATUS_LABELS) as [TaskStatus, string][]

  const activeCount = $derived(countActiveFilters(filters))

  // Toggle a value in one of the multi-select arrays. Always reassigns `filters`
  // with fresh arrays so the change propagates through the bound prop.
  function toggleAuthor(login: string) {
    const authorsNext = filters.authors.includes(login)
      ? filters.authors.filter((value) => value !== login)
      : [...filters.authors, login]
    filters = { ...filters, authors: authorsNext }
  }

  function toggleStatus(status: TaskStatus) {
    const statusesNext = filters.statuses.includes(status)
      ? filters.statuses.filter((value) => value !== status)
      : [...filters.statuses, status]
    filters = { ...filters, statuses: statusesNext }
  }

  function toggleColumn(column: TaskColumn) {
    const columnsNext = filters.columns.includes(column)
      ? filters.columns.filter((value) => value !== column)
      : [...filters.columns, column]
    filters = { ...filters, columns: columnsNext }
  }

  function clearAll() {
    filters = emptyFilters()
  }
</script>

<div class="flex max-h-[70vh] w-80 flex-col overflow-hidden rounded-md border border-border bg-card shadow-lg">
  <div class="flex items-center justify-between border-b border-border px-3 py-2">
    <span class="text-sm font-semibold text-foreground">Filters</span>
    <button
      type="button"
      onclick={clearAll}
      disabled={activeCount === 0}
      class="text-xs text-muted-foreground hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
    >
      Clear all
    </button>
  </div>

  <div class="flex-1 overflow-y-auto px-3 py-2">
    <!-- Author(s) -->
    <section class="py-1">
      <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Author</h3>
      {#if authors.length}
        <div class="flex flex-col gap-0.5">
          {#each authors as author (author.login)}
            <label class="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-secondary">
              <input
                type="checkbox"
                class="accent-primary"
                checked={filters.authors.includes(author.login)}
                onchange={() => toggleAuthor(author.login)}
              />
              {#if author.avatarUrl}
                <img
                  src={author.avatarUrl}
                  alt=""
                  class="size-4 flex-none rounded-full"
                  onerror={(event) => ((event.currentTarget as HTMLImageElement).style.display = 'none')}
                />
              {/if}
              <span class="truncate text-sm text-foreground">{author.login}</span>
            </label>
          {/each}
        </div>
      {:else}
        <p class="text-xs text-muted-foreground">No authors yet</p>
      {/if}
    </section>

    <!-- Status -->
    <section class="border-t border-border py-1 pt-2">
      <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Status</h3>
      <div class="flex flex-col gap-0.5">
        {#each statusOptions as [status, label] (status)}
          <label class="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-secondary">
            <input
              type="checkbox"
              class="accent-primary"
              checked={filters.statuses.includes(status)}
              onchange={() => toggleStatus(status)}
            />
            <span class="text-sm text-foreground">{label}</span>
          </label>
        {/each}
      </div>
    </section>

    <!-- Board column -->
    <section class="border-t border-border py-1 pt-2">
      <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Column</h3>
      <div class="flex flex-col gap-0.5">
        {#each COLUMNS as column (column.key)}
          <label class="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-secondary">
            <input
              type="checkbox"
              class="accent-primary"
              checked={filters.columns.includes(column.key)}
              onchange={() => toggleColumn(column.key)}
            />
            <span class="text-sm text-foreground">{column.label}</span>
          </label>
        {/each}
      </div>
    </section>

    <!-- Created date range -->
    <section class="border-t border-border py-1 pt-2">
      <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Created</h3>
      <div class="flex flex-col gap-1.5">
        <label class="flex items-center justify-between gap-2 text-sm text-foreground">
          <span class="text-muted-foreground">From</span>
          <Input type="date" class="h-8 w-40" bind:value={filters.createdFrom} />
        </label>
        <label class="flex items-center justify-between gap-2 text-sm text-foreground">
          <span class="text-muted-foreground">To</span>
          <Input type="date" class="h-8 w-40" bind:value={filters.createdTo} />
        </label>
      </div>
    </section>
  </div>
</div>
