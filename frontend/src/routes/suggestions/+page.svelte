<script lang="ts">
  // The Suggestions hub (issue #324): every recommendation the agent has made,
  // across every task, in one aggregated list so the operator manages them in bulk
  // instead of jumping ticket to ticket. Each row keeps the same one-click controls
  // as the task view: check it off as done, or turn it into a tracked issue. Done
  // items drop to a greyed-out bottom section (revealed on hover) and all are shown.
  import type { AggregatedSuggestion } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'

  import { acknowledgeSuggestion, listAllSuggestions } from '$lib/api'
  import { Switch } from '$lib/components/ui/switch'
  import SuggestionCreateButton from '$lib/components/SuggestionCreateButton.svelte'

  let suggestions = $state<AggregatedSuggestion[]>([])
  let loading = $state(true)
  let eventSource: EventSource | null = null

  // Open recommendations first (newest first), the done ones in their own section
  // (most recently done first). Acting on a row flips `acknowledged`, which moves it
  // between the two derived lists automatically.
  const open = $derived(
    suggestions
      .filter((suggestion) => !suggestion.acknowledged)
      .sort((a, b) => b.created_at.localeCompare(a.created_at))
  )
  const done = $derived(
    suggestions
      .filter((suggestion) => suggestion.acknowledged)
      .sort((a, b) =>
        (b.acknowledged_at ?? b.created_at).localeCompare(a.acknowledged_at ?? a.created_at)
      )
  )

  async function load() {
    try {
      suggestions = await listAllSuggestions()
    } catch (error) {
      console.debug('failed to load suggestions', error)
    } finally {
      loading = false
    }
  }

  async function toggle(suggestion: AggregatedSuggestion) {
    // Optimistically flip so the switch feels instant, then persist; revert on
    // failure so the UI never lies about what was actually saved (mirrors the task
    // view). Toggling `acknowledged` re-sorts the row into the right section.
    const next = !suggestion.acknowledged
    suggestion.acknowledged = next
    suggestion.acknowledged_at = next ? new Date().toISOString() : null
    try {
      await acknowledgeSuggestion(suggestion.id, next)
    } catch (error) {
      console.debug('failed to update suggestion, reverting', error)
      suggestion.acknowledged = !next
      suggestion.acknowledged_at = !next ? new Date().toISOString() : null
    }
  }

  // The create-issue button marks the suggestion done on the server; reflect that
  // locally so the row drops to the done section without waiting for a refetch.
  function onCreated(updated: { id: string; acknowledged_at: string | null }) {
    const match = suggestions.find((suggestion) => suggestion.id === updated.id)
    if (match) {
      match.acknowledged = true
      match.acknowledged_at = updated.acknowledged_at
    }
  }

  onMount(() => {
    load()
    // Every suggestion action calls notify_board on the server, and the agent posts
    // new ones as it works, so refetch on each board tick to stay current.
    eventSource = new EventSource('/api/v1/board/stream')
    eventSource.addEventListener('board', () => load())
  })

  onDestroy(() => eventSource?.close())
</script>

{#snippet row(suggestion: AggregatedSuggestion)}
  <li
    class="flex items-start justify-between gap-3 py-2 {suggestion.acknowledged
      ? 'opacity-60 transition-opacity hover:opacity-100'
      : ''}"
  >
    <button
      type="button"
      role="switch"
      aria-checked={suggestion.acknowledged}
      onclick={() => toggle(suggestion)}
      class="flex min-w-0 flex-1 cursor-pointer items-start gap-2 text-left"
    >
      <Switch
        checked={suggestion.acknowledged}
        tabindex={-1}
        aria-hidden="true"
        class="mt-0.5 pointer-events-none"
      />
      <span class="flex min-w-0 flex-col gap-0.5">
        <span class="flex flex-wrap items-center gap-2">
          <span
            class="text-sm font-medium {suggestion.acknowledged
              ? 'text-muted-foreground line-through'
              : ''}"
          >
            {suggestion.title}
          </span>
          <span class="rounded border border-border px-1.5 py-0 text-[10px] text-muted-foreground">
            {suggestion.kind === 'follow_up' ? '🧹 Follow-up' : '💡 Environment'}
          </span>
        </span>
        {#if suggestion.detail}
          <span class="whitespace-pre-wrap text-xs text-muted-foreground">{suggestion.detail}</span>
        {/if}
      </span>
    </button>
    <div class="flex flex-none flex-col items-end gap-1.5">
      <a
        href={`/task/${suggestion.task_id}`}
        title={suggestion.task_title}
        class="max-w-[10rem] truncate text-xs text-muted-foreground underline hover:text-foreground"
      >
        {suggestion.task_title}
      </a>
      {#if !suggestion.acknowledged}
        <SuggestionCreateButton
          {suggestion}
          source={suggestion.task_source}
          repoLinked={suggestion.task_repo_linked}
          oncreated={onCreated}
        />
      {/if}
    </div>
  </li>
{/snippet}

<div class="mx-auto flex max-w-3xl flex-col gap-4 p-6">
  <header>
    <h1 class="text-xl font-bold tracking-tight">Suggestions</h1>
    <p class="mt-0.5 text-sm text-muted-foreground">
      Every recommendation the agent has made across all tasks. Check one off once you have handled
      it, or one-click it into a tracked issue.
    </p>
  </header>

  {#if loading}
    <p class="text-sm text-muted-foreground">Loading…</p>
  {:else if suggestions.length === 0}
    <p class="rounded-lg border border-border bg-card p-6 text-center text-sm text-muted-foreground">
      No suggestions yet. The agent records environment tips and follow-up work as it runs.
    </p>
  {:else}
    <section class="rounded-lg border border-warning/50 bg-card p-3">
      <h2 class="text-sm font-semibold">Open ({open.length})</h2>
      {#if open.length === 0}
        <p class="mt-1 text-xs text-muted-foreground">Nothing open. Everything has been handled.</p>
      {:else}
        <ul class="mt-2 divide-y divide-border">
          {#each open as suggestion (suggestion.id)}
            {@render row(suggestion)}
          {/each}
        </ul>
      {/if}
    </section>

    {#if done.length}
      <section class="rounded-lg border border-border bg-card p-3">
        <h2 class="text-sm font-semibold text-muted-foreground">Done ({done.length})</h2>
        <ul class="mt-2 divide-y divide-border">
          {#each done as suggestion (suggestion.id)}
            {@render row(suggestion)}
          {/each}
        </ul>
      </section>
    {/if}
  {/if}
</div>
