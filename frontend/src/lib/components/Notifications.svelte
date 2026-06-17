<script lang="ts">
  import type { PendingQuestion } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { fly, fade } from 'svelte/transition'
  import { goto } from '$app/navigation'
  import { Bell, Check, Trash2, X } from '@lucide/svelte'

  import { Button } from '$lib/components/ui/button'
  import { getPendingQuestions, getSettings, soundUrl } from '$lib/api'

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
    // The task to open when clicked; null for a heart attack whose task is gone.
    taskId: string | null
    taskTitle: string
    prompt: string
    // 'question' is the agent asking for input; 'heart_attack' is a dead turn;
    // 'repo_sync_error' is a repo whose issue sync started failing (issue #213).
    kind: 'question' | 'heart_attack' | 'repo_sync_error'
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

  // Sound preferences, refreshed alongside the pending list. Default to on so the
  // very first sound can play before the settings have loaded.
  let attentionSoundEnabled = $state(true)
  let completionSoundEnabled = $state(true)
  let attentionSoundCustom = $state(false)
  let completionSoundCustom = $state(false)

  async function loadSoundPrefs() {
    try {
      const settings = await getSettings()
      attentionSoundEnabled = settings.attention_sound_enabled
      completionSoundEnabled = settings.completion_sound_enabled
      attentionSoundCustom = settings.attention_sound_custom
      completionSoundCustom = settings.completion_sound_custom
    } catch (error) {
      console.debug('failed to load sound settings', error)
    }
  }

  // Plays a notification sound (the custom clip if uploaded, else the bundled
  // default). Browsers block audio until the user has interacted with the page,
  // so a play() rejected before the first gesture is ignored rather than thrown.
  function playSound(kind: 'attention' | 'completion') {
    const enabled = kind === 'attention' ? attentionSoundEnabled : completionSoundEnabled
    if (!enabled || typeof Audio === 'undefined') {
      return
    }
    const custom = kind === 'attention' ? attentionSoundCustom : completionSoundCustom
    try {
      const audio = new Audio(soundUrl(kind, custom))
      audio.volume = 0.6
      void audio.play().catch((error) => console.debug('sound play blocked', error))
    } catch (error) {
      console.debug('sound playback failed', error)
    }
  }

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

  function pushToast(
    taskId: string | null,
    taskTitle: string,
    prompt: string,
    kind: Toast['kind'] = 'question'
  ) {
    const id = nextToastId++
    toasts = [...toasts, { id, taskId, taskTitle, prompt, kind }]
    setTimeout(() => dismissToast(id), TOAST_TIMEOUT_MS)
  }

  function dismissToast(id: number) {
    toasts = toasts.filter((toast) => toast.id !== id)
  }

  function goToTask(taskId: string | null) {
    open = false
    toasts = []
    // A heart attack whose task was deleted has nowhere to navigate.
    if (taskId) {
      goto(`/task/${taskId}`)
    }
  }

  function openTask(question: PendingQuestion) {
    markRead(question.id)
    goToTask(question.task_id)
  }

  function handleNotification(event: MessageEvent) {
    const data = JSON.parse(event.data) as { task_id: string; task_title: string; prompt: string }
    pushToast(data.task_id, data.task_title, data.prompt)
    notifyNatively(`Seraphim needs you: ${data.task_title}`, data.prompt)
    playSound('attention')
    refresh()
  }

  // A turn died (a "heart attack"); the defibrillator already handled recovery.
  // Surface it immediately as an alert toast and native notification; the board's
  // own banner persists the detail until the operator clears it.
  function handleHeartAttack(event: MessageEvent) {
    const data = JSON.parse(event.data) as {
      task_id: string | null
      task_title: string
      summary: string
    }
    pushToast(data.task_id, data.task_title, data.summary, 'heart_attack')
    notifyNatively(`Agent heart attack: ${data.task_title}`, data.summary)
    playSound('attention')
  }

  // A repo's issue sync started failing (issue #213). Fires once on the
  // success-to-error transition; the board's banner persists the ongoing state.
  function handleRepoSyncError(event: MessageEvent) {
    const data = JSON.parse(event.data) as { repo: string; message: string }
    pushToast(null, `Issue sync failed: ${data.repo}`, data.message, 'repo_sync_error')
    notifyNatively(`Issue sync failed: ${data.repo}`, data.message)
    playSound('attention')
  }

  // A task finished (auto-merged to Done). Sound-only: the board already reflects
  // it, so no toast, just the completion chime.
  function handleTaskFinished() {
    playSound('completion')
  }

  onMount(() => {
    refresh()
    loadSoundPrefs()
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
    eventSource.addEventListener('heart_attack', handleHeartAttack)
    eventSource.addEventListener('repo_sync_error', handleRepoSyncError)
    eventSource.addEventListener('task_finished', handleTaskFinished)
    eventSource.addEventListener('refresh', () => {
      refresh()
      // Pick up sound-preference changes the operator just saved.
      loadSoundPrefs()
    })
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
      class="pointer-events-auto flex items-start overflow-hidden rounded-lg border bg-card shadow-2xl {toast.kind ===
        'heart_attack' || toast.kind === 'repo_sync_error'
        ? 'border-destructive/60'
        : 'border-warning/50'}"
    >
      <button
        type="button"
        class="flex flex-1 flex-col gap-0.5 px-4 py-3 text-left"
        onclick={() => goToTask(toast.taskId)}
      >
        {#if toast.kind === 'heart_attack'}
          <span class="text-[10px] font-bold uppercase tracking-wide text-destructive"
            >Agent heart attack</span
          >
        {:else if toast.kind === 'repo_sync_error'}
          <span class="text-[10px] font-bold uppercase tracking-wide text-destructive"
            >Issue sync failed</span
          >
        {:else}
          <span class="text-[10px] font-bold uppercase tracking-wide text-warning"
            >Seraphim needs your input</span
          >
        {/if}
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
