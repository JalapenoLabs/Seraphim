<script lang="ts">
  // A searchable IANA time-zone picker. The native <select> was unsearchable,
  // and IANA ids (e.g. `America/Denver`) don't contain everyday names like
  // "Mountain" or the abbreviations "MST"/"MDT", so those were impossible to
  // find (issue #176). Here each zone is indexed by its id PLUS its computed
  // standard/daylight abbreviations PLUS a few friendly aliases, and the list is
  // explicitly sorted alphabetically.
  import { tick } from 'svelte'
  import { Check, ChevronsUpDown, Search } from '@lucide/svelte'

  let {
    value = $bindable(''),
    id
  }: {
    value?: string
    id?: string
  } = $props()

  type Zone = { value: string; label: string; search: string }

  // Everyday names / abbreviations for common regions, since the IANA id alone
  // doesn't contain words like "Mountain", "Pacific", or "ET". Lowercased; the
  // computed MST/MDT-style abbreviations below are added on top of these.
  const ALIASES: Record<string, string> = {
    'America/New_York': 'eastern time et',
    'America/Detroit': 'eastern time et',
    'America/Toronto': 'eastern time et',
    'America/Chicago': 'central time ct',
    'America/Denver': 'mountain time mt',
    'America/Boise': 'mountain time mt',
    'America/Phoenix': 'mountain time arizona no dst',
    'America/Los_Angeles': 'pacific time pt',
    'America/Vancouver': 'pacific time pt',
    'America/Anchorage': 'alaska time akt',
    'Pacific/Honolulu': 'hawaii time ht',
    'Europe/London': 'gmt bst uk britain'
  }

  // The short time-zone abbreviation for `zone` in a given month, via Intl.
  // e.g. `America/Denver` → "MST" in January, "MDT" in July. Zones without a
  // named abbreviation return a "GMT+X" form, which is still useful.
  function shortName(zone: string, month: number): string {
    try {
      const date = new Date(Date.UTC(2026, month, 15, 12))
      const parts = new Intl.DateTimeFormat('en-US', {
        timeZone: zone,
        timeZoneName: 'short'
      }).formatToParts(date)
      return parts.find((part) => part.type === 'timeZoneName')?.value ?? ''
    } catch {
      return ''
    }
  }

  // Built once. `Intl.supportedValuesOf` already returns ids sorted, but we sort
  // again so the order is guaranteed regardless of engine.
  function buildZones(): Zone[] {
    const ids = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : []
    const zones = ids.map((zone) => {
      // Winter (Jan) + summer (Jul) abbreviations, so both the standard and
      // daylight names (e.g. MST *and* MDT) are searchable year-round.
      const abbrs = [ ...new Set([ shortName(zone, 0), shortName(zone, 6) ].filter(Boolean)) ]
      const abbrLabel = abbrs.length ? ` (${abbrs.join('/')})` : ''
      return {
        value: zone,
        label: `${zone}${abbrLabel}`,
        search: `${zone} ${abbrs.join(' ')} ${ALIASES[zone] ?? ''}`.toLowerCase()
      }
    })
    zones.sort((left, right) => left.value.localeCompare(right.value))
    return zones
  }

  const zones = buildZones()

  let open = $state(false)
  let query = $state('')
  let wrapper = $state<HTMLDivElement>()
  let inputEl = $state<HTMLInputElement>()
  let highlighted = $state(0)

  const filtered = $derived(
    query.trim().length === 0
      ? zones
      : zones.filter((zone) => zone.search.includes(query.trim().toLowerCase()))
  )

  const selectedLabel = $derived(zones.find((zone) => zone.value === value)?.label ?? value ?? 'Select a time zone')

  async function openMenu() {
    open = true
    query = ''
    highlighted = 0
    await tick()
    inputEl?.focus()
  }

  function choose(zone: Zone) {
    value = zone.value
    open = false
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      highlighted = Math.min(highlighted + 1, filtered.length - 1)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      highlighted = Math.max(highlighted - 1, 0)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      const choice = filtered[highlighted]
      if (choice) {
        choose(choice)
      }
    } else if (event.key === 'Escape') {
      open = false
    }
  }

  function onWindowPointerDown(event: MouseEvent) {
    if (open && wrapper && !wrapper.contains(event.target as Node)) {
      open = false
    }
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<div bind:this={wrapper} class="relative">
  <button
    type="button"
    {id}
    onclick={() => (open ? (open = false) : openMenu())}
    aria-haspopup="listbox"
    aria-expanded={open}
    class="flex h-9 w-full items-center justify-between gap-2 rounded-md border border-input bg-transparent px-3 text-left text-sm shadow-xs focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
  >
    <span class="truncate">{selectedLabel}</span>
    <ChevronsUpDown class="size-4 shrink-0 opacity-60" />
  </button>

  {#if open}
    <div
      class="absolute z-50 mt-1 w-full overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-lg"
    >
      <div class="flex items-center gap-2 border-b border-border px-2.5">
        <Search class="size-4 shrink-0 opacity-60" />
        <input
          bind:this={inputEl}
          bind:value={query}
          onkeydown={onKeydown}
          oninput={() => (highlighted = 0)}
          placeholder="Search (e.g. Denver, Mountain, MST)…"
          class="h-9 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>
      <ul role="listbox" class="max-h-64 overflow-y-auto py-1">
        {#if filtered.length === 0}
          <li class="px-3 py-2 text-sm text-muted-foreground">No matching time zone.</li>
        {:else}
          {#each filtered as zone, index (zone.value)}
            <li>
              <button
                type="button"
                role="option"
                aria-selected={zone.value === value}
                onclick={() => choose(zone)}
                onmousemove={() => (highlighted = index)}
                class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors hover:bg-secondary {index ===
                highlighted
                  ? 'bg-secondary'
                  : ''} {zone.value === value ? 'text-primary' : 'text-foreground'}"
              >
                <Check class="size-3.5 shrink-0 {zone.value === value ? 'opacity-100' : 'opacity-0'}" />
                <span class="truncate">{zone.label}</span>
              </button>
            </li>
          {/each}
        {/if}
      </ul>
    </div>
  {/if}
</div>
