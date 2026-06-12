<script lang="ts">
  // A split button for turning an environment recommendation into a tracked
  // issue: a default action (matching where the ticket originally came from) and
  // a smushed-on dropdown arrow to pick a different target. While creating, the
  // whole control shows a loading state; on success the recommendation is marked
  // done by the server and the parent is told.
  import type { EnvSuggestion, SourceKind } from '$lib/types'
  import type { CreateIssueTarget } from '$lib/api'

  import { ChevronDown, LoaderCircle } from '@lucide/svelte'
  import { toast } from 'svelte-sonner'

  import { createIssueFromSuggestion } from '$lib/api'

  let {
    suggestion,
    source,
    repoLinked = false,
    oncreated
  }: {
    suggestion: EnvSuggestion
    // Where the ticket was originally created from; the default action mirrors it.
    source: SourceKind
    // Whether the task has a linked repo (a GitHub issue needs one).
    repoLinked?: boolean
    oncreated: (updated: EnvSuggestion) => void
  } = $props()

  const LABELS: Record<CreateIssueTarget, string> = {
    internal: 'Seraphim',
    github: 'GitHub',
    jira: 'Jira'
  }
  // Dropdown order, as listed in the issue.
  const OPTIONS: CreateIssueTarget[] = ['internal', 'jira', 'github']

  // The originating source maps 1:1 onto a target; internal sources default to a
  // Seraphim issue.
  const defaultTarget = $derived<CreateIssueTarget>(
    source === 'github' ? 'github' : source === 'jira' ? 'jira' : 'internal'
  )

  let open = $state(false)
  let loading = $state(false)
  let wrapper = $state<HTMLDivElement>()

  function enabled(target: CreateIssueTarget): boolean {
    // A GitHub issue needs a repo to open it in.
    return target !== 'github' || repoLinked
  }

  async function extractError(error: unknown): Promise<string> {
    if (error && typeof error === 'object' && 'response' in error) {
      try {
        const body = await (error as { response: Response }).response.json()
        if (body?.error) return String(body.error)
      } catch {
        // fall through to the generic message
      }
    }
    return 'Failed to create the issue.'
  }

  async function run(target: CreateIssueTarget) {
    if (loading || !enabled(target)) return
    open = false
    loading = true
    try {
      const { suggestion: updated, url } = await createIssueFromSuggestion(suggestion.id, target)
      toast.success(`Created ${LABELS[target]} issue`, {
        description: url ? 'Click to open it on the source.' : undefined,
        action: url
          ? { label: 'Open', onClick: () => window.open(url, '_blank', 'noopener') }
          : undefined
      })
      oncreated(updated)
    } catch (error) {
      toast.error(await extractError(error))
    } finally {
      loading = false
    }
  }

  function onWindowPointerDown(event: MouseEvent) {
    if (open && wrapper && !wrapper.contains(event.target as Node)) open = false
  }
  function onWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') open = false
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onWindowKeydown} />

<div bind:this={wrapper} class="relative flex flex-none">
  <!-- Default action: create in the originating source. -->
  <button
    type="button"
    onclick={() => run(defaultTarget)}
    disabled={loading}
    class="inline-flex items-center gap-1.5 rounded-l-md border border-primary bg-primary px-2.5 py-1 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-60"
  >
    {#if loading}
      <LoaderCircle class="size-3.5 animate-spin" />
    {/if}
    Create {LABELS[defaultTarget]} Issue
  </button>
  <!-- Smushed-on dropdown toggle (shares a border, only the right edge rounds). -->
  <button
    type="button"
    onclick={() => (open = !open)}
    disabled={loading}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label="Choose where to create the issue"
    class="-ml-px inline-flex items-center rounded-r-md border border-primary bg-primary px-1 py-1 text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-60"
  >
    <ChevronDown class="size-3.5" />
  </button>

  {#if open}
    <div
      role="menu"
      class="absolute right-0 top-full z-50 mt-1 min-w-[12rem] overflow-hidden rounded-md border border-border bg-card py-1 text-xs shadow-lg"
    >
      {#each OPTIONS as target (target)}
        <button
          type="button"
          role="menuitem"
          onclick={() => run(target)}
          disabled={!enabled(target)}
          title={enabled(target) ? '' : 'This task has no linked repository.'}
          class="flex w-full items-center justify-between gap-3 px-3 py-1.5 text-left text-foreground transition-colors hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-40"
        >
          <span>Create {LABELS[target]} issue</span>
          {#if target === defaultTarget}
            <span class="text-[10px] uppercase tracking-wide text-muted-foreground">default</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
