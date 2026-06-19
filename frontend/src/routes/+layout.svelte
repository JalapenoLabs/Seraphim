<script lang="ts">
  import type { BoardResponse } from '$lib/types'

  import '../app.css'
  import { onMount, onDestroy } from 'svelte'
  import { page } from '$app/stores'
  import { Pause, Plus, TriangleAlert, Timer } from '@lucide/svelte'

  import { getBoard } from '$lib/api'
  import { isWithinSchedule } from '$lib/schedule'
  import { Toaster } from '$lib/components/ui/sonner'
  import { buttonVariants } from '$lib/components/ui/button'

  import Notifications from '$lib/components/Notifications.svelte'
  import SearchBar from '$lib/components/SearchBar.svelte'

  let { children } = $props()

  const links = [
    { href: '/', label: 'Board' },
    { href: '/suggestions', label: 'Suggestions' },
    { href: '/watch', label: 'Watch' },
    { href: '/compose', label: 'Compose' },
    { href: '/automation', label: 'Automation' },
    { href: '/repos', label: 'Repositories' },
    { href: '/railways', label: 'Railways' },
    { href: '/settings', label: 'Settings' }
  ]

  // The watch page is a full-screen, kiosk-style monitor: it owns the whole
  // viewport, so the app chrome (navbar) gets out of its way.
  const fullscreen = $derived($page.url.pathname === '/watch')

  // The board (settings + tasks) drives the navbar status. The board SSE stream
  // ticks on every change (including pause/resume), keeping the badge live.
  let board = $state<BoardResponse | null>(null)
  let eventSource: EventSource | null = null

  async function loadStatus() {
    try {
      board = await getBoard()
    } catch {
      // Transient; the next stream tick or navigation retries.
    }
  }

  // What the agent is doing right now, in priority order, so the navbar explains
  // why work is (or isn't) being picked up.
  const status = $derived.by(() => {
    const settings = board?.settings
    if (!settings) {
      return null
    }
    if (settings.agent_paused) {
      return { key: 'paused', label: 'Agent paused' } as const
    }
    if (settings.config_repo_url && settings.config_repo_error) {
      return { key: 'halted', label: 'Agent halted' } as const
    }
    // A transient rate-limit cooldown: the agent is mid-turn, waiting a few
    // seconds before retrying. Takes the place of the working badge while it lasts.
    if (settings.cooldown_until && new Date(settings.cooldown_until) > new Date()) {
      return { key: 'cooldown', label: 'Cooling down' } as const
    }
    const working = board?.tasks.some(
      (task) =>
        task.board_column === 'in_progress' ||
        ['preparing', 'working', 'opening_pr', 'merging'].includes(task.status)
    )
    if (working) {
      return { key: 'working', label: 'Working' } as const
    }
    if (settings.availability_enabled && !isWithinSchedule(settings, new Date())) {
      return { key: 'offhours', label: 'Off hours' } as const
    }
    return { key: 'idle', label: 'Idle' } as const
  })

  // Filled, loud pills for the states that mean "not working"; subtle pills with a
  // status dot otherwise.
  const PILLS = {
    paused: 'bg-warning text-warning-foreground',
    halted: 'bg-destructive text-white',
    cooldown: 'bg-warning text-warning-foreground',
    working: 'border border-border text-foreground',
    offhours: 'border border-border text-muted-foreground',
    idle: 'border border-border text-muted-foreground'
  } as const

  const DOTS = {
    working: 'bg-success animate-pulse',
    offhours: 'bg-warning',
    idle: 'bg-muted-foreground'
  } as const

  onMount(() => {
    loadStatus()
    eventSource = new EventSource('/api/v1/board/stream')
    eventSource.addEventListener('board', () => loadStatus())
  })

  onDestroy(() => eventSource?.close())
</script>

{#if fullscreen}
  <main class="h-screen w-screen overflow-hidden">
    {@render children()}
  </main>
{:else}
<div class="flex h-screen flex-col">
  <header class="flex items-center gap-4 border-b border-border bg-card px-6 py-3">
    <a href="/" class="flex items-center gap-2 text-lg font-bold tracking-tight text-foreground">
      <img
        src="/favicon.png"
        alt=""
        class="h-6 w-6"
        onerror={(event) => ((event.currentTarget as HTMLImageElement).style.display = 'none')}
      />
      Seraphim
    </a>
    <nav class="flex gap-1">
      {#each links as link}
        <a
          href={link.href}
          class="rounded-md px-3 py-1.5 text-sm transition-colors {$page.url.pathname === link.href
            ? 'bg-secondary text-foreground'
            : 'text-muted-foreground hover:bg-secondary hover:text-foreground'}"
        >
          {link.label}
        </a>
      {/each}
    </nav>

    <!-- Middle: fuzzy issue search over the live board. -->
    <div class="flex flex-1 justify-center px-2">
      <SearchBar tasks={board?.tasks ?? []} />
    </div>

    <div class="flex flex-none items-center gap-3">
      {#if status}
        <span
          class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-semibold {PILLS[
            status.key
          ]}"
          title="Agent status"
        >
          {#if status.key === 'paused'}
            <Pause class="size-3.5" />
          {:else if status.key === 'halted'}
            <TriangleAlert class="size-3.5" />
          {:else if status.key === 'cooldown'}
            <Timer class="size-3.5" />
          {:else}
            <span class="size-2 rounded-full {DOTS[status.key as keyof typeof DOTS]}"></span>
          {/if}
          {status.label}
        </span>
      {/if}
      <a href="/issues/new" class={buttonVariants({ variant: 'outline', size: 'sm' })}>
        <Plus class="size-4" /> Create issue
      </a>
      <Notifications />
    </div>
  </header>
  <main class="min-h-0 flex-1 overflow-auto">
    {@render children()}
  </main>
</div>
{/if}

<Toaster theme="dark" richColors />
