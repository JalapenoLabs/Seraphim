<script lang="ts">
  import type { Repository } from '$lib/types'

  import { Check } from '@lucide/svelte'

  // `selected` is bindable so a parent can two-way bind the chosen repo ids.
  // Order is preserved in selection order: the first picked is the primary repo
  // the agent branches in. Clicking a row toggles its membership.
  let {
    repos,
    selected = $bindable([]),
    id
  }: {
    repos: Repository[]
    selected?: string[]
    id?: string
  } = $props()

  function toggle(repoId: string) {
    if (selected.includes(repoId)) {
      selected = selected.filter((entry) => entry !== repoId)
    } else {
      selected = [...selected, repoId]
    }
  }
</script>

<div {id} class="max-h-56 overflow-y-auto rounded-md border border-border">
  {#if repos.length === 0}
    <p class="px-3 py-2 text-xs text-muted-foreground">No repositories are configured yet.</p>
  {/if}
  {#each repos as repo (repo.id)}
    {@const isSelected = selected.includes(repo.id)}
    {@const primary = selected[0] === repo.id}
    <button
      type="button"
      role="checkbox"
      aria-checked={isSelected}
      onclick={() => toggle(repo.id)}
      class="flex w-full items-center gap-2 border-b border-border px-3 py-2 text-left text-sm last:border-b-0 hover:bg-secondary/50"
    >
      <span
        class="flex size-4 shrink-0 items-center justify-center rounded border {isSelected
          ? 'border-primary bg-primary text-primary-foreground'
          : 'border-border'}"
      >
        {#if isSelected}
          <Check class="size-3" />
        {/if}
      </span>
      <span class="min-w-0 flex-1 truncate">{repo.full_name}</span>
      {#if primary && selected.length > 1}
        <span class="shrink-0 rounded bg-primary/15 px-1.5 py-0.5 text-xs text-primary">Primary</span>
      {/if}
    </button>
  {/each}
</div>
