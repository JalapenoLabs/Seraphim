<script lang="ts">
  // One railway swimlane: a header (name, description, lifecycle indicator, the
  // per-railway pause, and that lane's compact stats) above the lane's body. The
  // body is the kanban columns row, rendered by the board via the `children`
  // snippet so the page keeps owning the drag-and-drop, sort, filter, and bulk
  // wiring; this component only frames one lane.
  import type { Railway, RailwayState } from '$lib/types'
  import type { Snippet } from 'svelte'

  import { Pause, Play } from '@lucide/svelte'

  import { Button } from '$lib/components/ui/button'
  import RailwayStats from '$lib/components/RailwayStats.svelte'

  let {
    railway,
    taskCount,
    onTogglePause,
    children
  }: {
    railway: Railway
    // How many cards sit in this lane (honoring the board filter), shown by the name.
    taskCount: number
    onTogglePause: () => void
    children: Snippet
  } = $props()

  // The lifecycle dot's color and label. `running` is live, `stopped` idle, and
  // the two transitions amber. A const map keeps the mapping in one place.
  const stateStyle = {
    stopped: { dot: 'bg-muted-foreground/50', label: 'Stopped' },
    starting: { dot: 'bg-warning', label: 'Starting' },
    running: { dot: 'bg-success', label: 'Running' },
    stopping: { dot: 'bg-warning', label: 'Stopping' }
  } as const satisfies Record<RailwayState, { dot: string; label: string }>

  const lifecycle = $derived(stateStyle[railway.lifecycle_state])
</script>

<!-- On lg the lane fills the board's height so its columns scroll individually
     (issue #273); multiple lanes each take full height and scroll between them via
     the swimlanes container. Below lg it sizes to content and the page scrolls. -->
<section
  class="flex min-h-0 flex-col rounded-lg border border-border bg-card/40 lg:h-full lg:flex-none"
>
  <header
    class="flex flex-none flex-wrap items-center justify-between gap-x-4 gap-y-2 border-b border-border px-4 py-2.5"
  >
    <div class="flex min-w-0 items-center gap-2.5">
      <span
        class="size-2 flex-none rounded-full {lifecycle.dot} {railway.lifecycle_state === 'running'
          ? 'animate-pulse motion-reduce:animate-none'
          : ''}"
        title={`Container ${lifecycle.label.toLowerCase()}`}
      ></span>
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <strong class="truncate text-sm">{railway.name}</strong>
          {#if railway.is_main}
            <span class="rounded border border-border px-1.5 py-0 text-[10px] text-muted-foreground">
              main
            </span>
          {/if}
          {#if railway.paused}
            <span class="rounded border border-warning/40 px-1.5 py-0 text-[10px] text-warning">
              paused
            </span>
          {/if}
          <span class="text-xs tabular-nums text-muted-foreground">
            {taskCount} {taskCount === 1 ? 'card' : 'cards'}
          </span>
        </div>
        {#if railway.description}
          <p class="truncate text-xs text-muted-foreground" title={railway.description}>
            {railway.description}
          </p>
        {/if}
      </div>
    </div>

    <div class="flex items-center gap-3">
      <RailwayStats railwayId={railway.id} />
      <Button
        variant={railway.paused ? 'default' : 'outline'}
        size="sm"
        onclick={onTogglePause}
        title={railway.paused ? 'Resume this railway' : 'Pause this railway'}
      >
        {#if railway.paused}
          <Play class="size-4" /> Resume
        {:else}
          <Pause class="size-4" /> Pause
        {/if}
      </Button>
    </div>
  </header>

  <div class="min-h-0 flex-1 p-3">
    {@render children()}
  </div>
</section>
