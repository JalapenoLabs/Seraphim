<script lang="ts">
  // A fullscreen screenshot viewer (issue #249). Opened by clicking a thumbnail in
  // the activity feed or task history; pages through a provided set of screenshots
  // with the arrow keys, closes on Escape or a click outside the image, and toggles
  // a 1:1 zoom on click. The bytes load only while the viewer is open.
  import { onMount, onDestroy } from 'svelte'
  import { ChevronLeft, ChevronRight, X } from '@lucide/svelte'

  type Item = { id: string; caption?: string; route?: string }

  let {
    items,
    index = 0,
    onClose
  }: { items: Item[]; index?: number; onClose: () => void } = $props()

  // Page offset from the initial `index`. The viewer is recreated each time it
  // opens, so the starting position comes straight from the prop and paging just
  // moves this offset (keeps `current` derived, never seeded from reactive state).
  let offset = $state(0)
  let zoomed = $state(false)

  const current = $derived(index + offset)
  const item = $derived(items[current])

  function show(next: number) {
    if (next >= 0 && next < items.length) {
      offset = next - index
      zoomed = false
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      onClose()
    } else if (event.key === 'ArrowLeft') {
      show(current - 1)
    } else if (event.key === 'ArrowRight') {
      show(current + 1)
    }
  }

  onMount(() => window.addEventListener('keydown', onKeydown))
  onDestroy(() => window.removeEventListener('keydown', onKeydown))
</script>

{#if item}
  <div class="fixed inset-0 z-50" role="dialog" aria-modal="true" aria-label="Screenshot viewer">
    <!-- The backdrop is a real button so click-to-close is keyboard accessible; the
         content layer floats above it and only its interactive bits catch clicks, so
         clicking empty space falls through here and closes. -->
    <button
      type="button"
      class="absolute inset-0 cursor-default bg-black/90 backdrop-blur-sm"
      aria-label="Close screenshot viewer"
      onclick={onClose}
    ></button>

    <div class="pointer-events-none relative flex h-full flex-col">
      <div class="flex items-center justify-between gap-3 px-4 py-2 text-sm text-white/80">
        <span class="pointer-events-auto min-w-0 truncate">
          {#if item.caption}<span class="font-medium text-white">{item.caption}</span>{/if}
          {#if item.route}<span class="ml-2 text-white/60">{item.route}</span>{/if}
        </span>
        <span class="flex items-center gap-3">
          <span class="tabular-nums text-white/60">{current + 1} / {items.length}</span>
          <button
            type="button"
            onclick={onClose}
            aria-label="Close"
            class="pointer-events-auto rounded p-1 text-white hover:bg-white/10"
          >
            <X class="size-5" />
          </button>
        </span>
      </div>

      <div class="flex min-h-0 flex-1 items-center justify-center overflow-auto px-4 pb-4">
        {#if items.length > 1}
          <button
            type="button"
            aria-label="Previous screenshot"
            disabled={current === 0}
            onclick={() => show(current - 1)}
            class="pointer-events-auto absolute left-3 top-1/2 -translate-y-1/2 rounded-full bg-white/10 p-2 text-white hover:bg-white/20 disabled:opacity-30"
          >
            <ChevronLeft class="size-6" />
          </button>
        {/if}

        <button
          type="button"
          onclick={() => (zoomed = !zoomed)}
          aria-label={zoomed ? 'Zoom out' : 'Zoom in'}
          class="pointer-events-auto m-auto"
        >
          <img
            src={`/api/v1/screenshots/${item.id}`}
            alt={item.caption || item.route || 'agent screenshot'}
            class={zoomed
              ? 'max-h-none max-w-none cursor-zoom-out'
              : 'max-h-[85vh] max-w-full cursor-zoom-in object-contain'}
          />
        </button>

        {#if items.length > 1}
          <button
            type="button"
            aria-label="Next screenshot"
            disabled={current === items.length - 1}
            onclick={() => show(current + 1)}
            class="pointer-events-auto absolute right-3 top-1/2 -translate-y-1/2 rounded-full bg-white/10 p-2 text-white hover:bg-white/20 disabled:opacity-30"
          >
            <ChevronRight class="size-6" />
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
