<script lang="ts">
  import type { PendingQuestion } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { fly, fade } from 'svelte/transition'
  import { goto } from '$app/navigation'
  import { Bell, Check, Trash2, X } from '@lucide/svelte'

  import { Button } from '$lib/components/ui/button'
  import { getPendingQuestions } from '$lib/api'

  // How long a toast stays on screen before auto-dismissing.
  const TOAST_TIMEOUT_MS = 8000

  // Notifications are the agent's pending questions, which have no server-side
  // "read"/"dismissed" state. We track that client-side, keyed by question id,
  // and prune to the live set on each refresh so it never grows unbounded and a
  // re-asked question reads as unread again.
  const READ_KEY = 'seraphim.notif.read'
  const DISMISSED_KEY = 'seraphim.notif.dismissed'

  type Toast = {
    id: number
    taskId: string
    taskTitle: string
    prompt: string
  }

  function loadIds(key: string): Set<string> {
    if (typeof localStorage === 'undefined') {
      return new Set()
    }
    try {
      const raw = localStorage.getItem(key)
      return new Set(raw ? (JSON.parse(raw) as string[]) : [])
    } catch {
      return new Set()
    }
  }

  function saveIds(key: string, ids: Set<string>) {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(key, JSON.stringify([...ids]))
    }
  }

  let pending = $state<PendingQuestion[]>([])
  let readIds = $state<Set<string>>(loadIds(READ_KEY))
  let dismissedIds = $state<Set<string>>(loadIds(DISMISSED_KEY))
  let open = $state(false)
  let toasts = $state<Toast[]>([])
  let nextToastId = 0

  let eventSource: EventSource | null = null

  // What the bell surfaces: pending questions the user hasn't cleared away. The
  // unread count drives the badge.
  const visible = $derived(pending.filter((question) => !dismissedIds.has(question.id)))
  const unreadCount = $derived(visible.filter((question) => !readIds.has(question.id)).length)

  // If everything is cleared or answered while the drawer is open, close it.
  $effect(() => {
    if (open && visible.length === 0) {
      open = false
    }
  })

  async function refresh() {
    try {
      const response = await getPendingQuestions()
      pending = response.questions
      pruneState()
    } catch (error) {
      console.debug('failed to refresh pending questions', error)
    }
  }

  // Drop read/dismissed ids for questions that are no longer pending.
  function pruneState() {
    const live = new Set(pending.map((question) => question.id))
    readIds = new Set([...readIds].filter((id) => live.has(id)))
    dismissedIds = new Set([...dismissedIds].filter((id) => live.has(id)))
    saveIds(READ_KEY, readIds)
    saveIds(DISMISSED_KEY, dismissedIds)
  }

  function markRead(id: string) {
    readIds = new Set([...readIds, id])
    saveIds(READ_KEY, readIds)
  }

  function markAllRead() {
    readIds = new Set([...readIds, ...visible.map((question) => question.id)])
    saveIds(READ_KEY, readIds)
  }

  function clearAll() {
    dismissedIds = new Set([...dismissedIds, ...visible.map((question) => question.id)])
    saveIds(DISMISSED_KEY, dismissedIds)
    open = false
  }

  // Fires a native desktop notification when the browser has granted permission.
  function notifyNatively(title: string, body: string) {
    if (typeof Notification === 'undefined' || Notification.permission !== 'granted') {
      return
    }
    try {
      new Notification(title, { body })
    } catch (error) {
      console.debug('native notification failed', error)
    }
  }

  function pushToast(taskId: string, taskTitle: string, prompt: string) {
    const id = nextToastId++
    toasts = [...toasts, { id, taskId, taskTitle, prompt }]
    setTimeout(() => dismissToast(id), TOAST_TIMEOUT_MS)
  }

  function dismissToast(id: number) {
    toasts = toasts.filter((toast) => toast.id !== id)
  }

  function goToTask(taskId: string) {
    open = false
    toasts = []
    goto(`/task/${taskId}`)
  }

  function openTask(question: PendingQuestion) {
    markRead(question.id)
    goToTask(question.task_id)
  }

  function handleNotification(event: MessageEvent) {
    const data = JSON.parse(event.data) as { task_id: string; task_title: string; prompt: string }
    pushToast(data.task_id, data.task_title, data.prompt)
    notifyNatively(`Seraphim needs you: ${data.task_title}`, data.prompt)
    refresh()
  }

  onMount(() => {
    refresh()
    // Prompt for native desktop notifications on first load, as the issue asks.
    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
      Notification.requestPermission().catch((error) =>
        console.debug('notification permission request failed', error)
      )
    }
    // Live updates: a new question toasts and notifies; any board change (e.g. a
    // question answered elsewhere) refreshes the pending list.
    eventSource = new EventSource('/api/v1/notifications/stream')
    eventSource.addEventListener('notification', handleNotification)
    eventSource.addEventListener('refresh', () => refresh())
  })

  onDestroy(() => eventSource?.close())
</script>

<svelte:window
  onkeydown={(event) => {
    if (open && event.key === 'Escape') {
      open = false
    }
  }}
/>

<!-- The bell only appears when there's something to show. -->
{#if visible.length > 0}
  <button
    type="button"
    class="relative rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
    title="Notifications"
    aria-label="Notifications"
    onclick={() => (open = true)}
  >
    <Bell class="size-5" />
    {#if unreadCount > 0}
      <span
        class="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold leading-none text-destructive-foreground"
      >
        {unreadCount}
      </span>
    {/if}
  </button>
{/if}

<!-- Right-side drawer (slides in over a dimmed backdrop). -->
{#if open}
  <div class="fixed inset-0 z-50">
    <button
      type="button"
      class="absolute inset-0 bg-black/50"
      aria-label="Close notifications"
      transition:fade={{ duration: 150 }}
      onclick={() => (open = false)}
    ></button>
    <aside
      class="absolute right-0 top-0 flex h-full w-[360px] max-w-[90vw] flex-col border-l border-border bg-card shadow-2xl"
      transition:fly={{ x: 360, duration: 200 }}
    >
      <header class="flex items-center gap-2 border-b border-border px-4 py-3">
        <strong class="text-sm">Notifications</strong>
        <div class="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="sm" disabled={unreadCount === 0} onclick={markAllRead}>
            <Check class="size-3.5" /> Mark all read
          </Button>
          <Button variant="ghost" size="sm" onclick={clearAll}>
            <Trash2 class="size-3.5" /> Clear all
          </Button>
          <Button variant="ghost" size="icon" aria-label="Close" onclick={() => (open = false)}>
            <X class="size-4" />
          </Button>
        </div>
      </header>
      <div class="min-h-0 flex-1 overflow-y-auto">
        {#each visible as question (question.id)}
          <button
            type="button"
            class="flex w-full flex-col gap-0.5 border-b border-border px-4 py-3 text-left transition-colors hover:bg-secondary {readIds.has(
              question.id
            )
              ? 'opacity-60'
              : ''}"
            onclick={() => openTask(question)}
          >
            <span class="flex items-center gap-2 text-xs text-muted-foreground">
              {#if !readIds.has(question.id)}
                <span class="size-1.5 flex-none rounded-full bg-destructive"></span>
              {/if}
              {question.task_title}
            </span>
            <span class="text-sm">{question.prompt}</span>
          </button>
        {/each}
      </div>
    </aside>
  </div>
{/if}

<!-- Toasts float at the bottom-right, independent of the bell drawer. -->
<div class="pointer-events-none fixed bottom-5 right-5 z-50 flex max-w-sm flex-col gap-2">
  {#each toasts as toast (toast.id)}
    <div
      class="pointer-events-auto flex items-start overflow-hidden rounded-lg border border-warning/50 bg-card shadow-2xl"
    >
      <button
        type="button"
        class="flex flex-1 flex-col gap-0.5 px-4 py-3 text-left"
        onclick={() => goToTask(toast.taskId)}
      >
        <span class="text-[10px] font-bold uppercase tracking-wide text-warning"
          >Seraphim needs your input</span
        >
        <span class="text-xs text-muted-foreground">{toast.taskTitle}</span>
        <span class="text-sm">{toast.prompt}</span>
      </button>
      <button
        type="button"
        class="px-2.5 py-2 text-muted-foreground transition-colors hover:text-foreground"
        title="Dismiss"
        aria-label="Dismiss"
        onclick={() => dismissToast(toast.id)}
      >
        <X class="size-4" />
      </button>
    </div>
  {/each}
</div>
