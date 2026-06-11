<script lang="ts">
  // A very tiny per-column sort control for the kanban header. Left-click cycles
  // the sort level forward, right-click cycles backward, and a press-and-hold
  // opens a dropdown to pick a level directly. The label reads "Auto" / "Id" /
  // "Created" / "Updated" with an arrow for direction, and the button is tinted
  // when a real sort (not Auto) is active.
  import { ArrowUp, ArrowDown, ArrowDownUp } from '@lucide/svelte'

  import { SORT_CYCLE, SORT_META, nextSort, prevSort, type SortKey } from '$lib/columnSort'

  let {
    value,
    onchange,
    label = ''
  }: {
    value: SortKey
    onchange: (next: SortKey) => void
    label?: string
  } = $props()

  // How long the primary button must be held before the picker opens instead of
  // the click cycling.
  const LONG_PRESS_MS = 450

  let menuOpen = $state(false)
  let wrapper = $state<HTMLDivElement>()
  let pressTimer: ReturnType<typeof setTimeout> | null = null
  // Set when a hold opens the picker, so the trailing click doesn't also cycle.
  let longPressed = false

  const meta = $derived(SORT_META[value])
  const active = $derived(value !== 'custom')

  const tooltip = $derived(
    `Sort: ${meta.label}${meta.direction ? ` ${meta.direction}` : ''}. ` +
      'Click to cycle, right-click to reverse, hold to choose.'
  )

  function onClick() {
    if (longPressed) {
      longPressed = false
      return
    }
    onchange(nextSort(value))
  }

  function onContextMenu(event: MouseEvent) {
    // Right-click reverses the cycle instead of opening the native menu.
    event.preventDefault()
    onchange(prevSort(value))
  }

  function onPointerDown(event: PointerEvent) {
    // Only the primary button arms the hold-to-open; right-click is handled above.
    if (event.button !== 0) {
      return
    }
    longPressed = false
    pressTimer = setTimeout(() => {
      longPressed = true
      menuOpen = true
    }, LONG_PRESS_MS)
  }

  function clearPress() {
    if (pressTimer) {
      clearTimeout(pressTimer)
      pressTimer = null
    }
  }

  function pick(next: SortKey) {
    onchange(next)
    menuOpen = false
  }

  function onWindowPointerDown(event: MouseEvent) {
    if (menuOpen && wrapper && !wrapper.contains(event.target as Node)) {
      menuOpen = false
    }
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      menuOpen = false
    }
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onWindowKeydown} />

<div bind:this={wrapper} class="relative">
  <button
    type="button"
    onclick={onClick}
    oncontextmenu={onContextMenu}
    onpointerdown={onPointerDown}
    onpointerup={clearPress}
    onpointerleave={clearPress}
    title={tooltip}
    aria-label={`Sort ${label}`}
    aria-haspopup="menu"
    aria-expanded={menuOpen}
    class="inline-flex select-none items-center gap-0.5 rounded px-1 py-0.5 text-[10px] font-semibold normal-case leading-none transition-colors {active
      ? 'bg-primary/15 text-primary hover:bg-primary/25'
      : 'text-muted-foreground hover:bg-secondary hover:text-foreground'}"
  >
    {meta.label}
    {#if meta.direction === 'asc'}
      <ArrowUp class="size-3" />
    {:else if meta.direction === 'desc'}
      <ArrowDown class="size-3" />
    {:else}
      <ArrowDownUp class="size-3 opacity-60" />
    {/if}
  </button>

  {#if menuOpen}
    <div
      role="menu"
      aria-label={`Sort ${label}`}
      class="absolute right-0 top-full z-50 mt-1 min-w-[9rem] overflow-hidden rounded-md border border-border bg-card py-1 text-xs font-normal normal-case shadow-lg"
    >
      {#each SORT_CYCLE as option (option)}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={option === value}
          onclick={() => pick(option)}
          class="flex w-full items-center justify-between gap-3 px-3 py-1.5 text-left transition-colors hover:bg-secondary {option ===
          value
            ? 'font-medium text-primary'
            : 'text-foreground'}"
        >
          <span>{SORT_META[option].label}</span>
          {#if SORT_META[option].direction === 'asc'}
            <ArrowUp class="size-3" />
          {:else if SORT_META[option].direction === 'desc'}
            <ArrowDown class="size-3" />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
