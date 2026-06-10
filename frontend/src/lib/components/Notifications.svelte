<script lang="ts">
  import type { PendingQuestion } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { goto } from '$app/navigation'

  import { getPendingQuestions } from '$lib/api'

  // How long a toast stays on screen before auto-dismissing.
  const TOAST_TIMEOUT_MS = 8000

  type Toast = {
    id: number
    taskId: string
    taskTitle: string
    prompt: string
  }

  let pending = $state<PendingQuestion[]>([])
  let toasts = $state<Toast[]>([])
  let open = $state(false)
  let nextToastId = 0

  let eventSource: EventSource | null = null

  async function refresh() {
    try {
      const response = await getPendingQuestions()
      pending = response.questions
    } catch (error) {
      console.debug('failed to refresh pending questions', error)
    }
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

  function openTask(taskId: string) {
    open = false
    dismissAll()
    goto(`/task/${taskId}`)
  }

  function dismissAll() {
    toasts = []
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

<div class="notifications">
  <button class="bell" title="Notifications" onclick={() => (open = !open)}>
    🔔
    {#if pending.length}
      <span class="badge">{pending.length}</span>
    {/if}
  </button>

  {#if open}
    <div class="panel">
      <header>
        <strong>Notifications</strong>
        <button class="close" title="Close" onclick={() => (open = false)}>✕</button>
      </header>
      {#if pending.length === 0}
        <p class="empty">Nothing needs your attention.</p>
      {:else}
        {#each pending as question}
          <button class="item" onclick={() => openTask(question.task_id)}>
            <span class="item-task">{question.task_title}</span>
            <span class="item-prompt">{question.prompt}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<!-- Toasts float at the bottom-right, independent of the bell dropdown. -->
<div class="toasts">
  {#each toasts as toast (toast.id)}
    <div class="toast">
      <button class="toast-body" onclick={() => openTask(toast.taskId)}>
        <span class="toast-title">Seraphim needs your input</span>
        <span class="toast-task">{toast.taskTitle}</span>
        <span class="toast-prompt">{toast.prompt}</span>
      </button>
      <button class="toast-close" title="Dismiss" onclick={() => dismissToast(toast.id)}>✕</button>
    </div>
  {/each}
</div>

<style>
  .notifications {
    position: relative;
  }

  .bell {
    position: relative;
    background: transparent;
    border: none;
    font-size: 1.2rem;
    cursor: pointer;
    padding: 0.2rem 0.4rem;
  }

  .badge {
    position: absolute;
    top: -0.1rem;
    right: -0.1rem;
    background: var(--danger);
    color: #fff;
    border-radius: 999px;
    font-size: 0.65rem;
    font-weight: 700;
    min-width: 1rem;
    padding: 0 0.25rem;
    line-height: 1rem;
    text-align: center;
  }

  .panel {
    position: absolute;
    right: 0;
    top: 2.2rem;
    width: 320px;
    max-height: 70vh;
    overflow-y: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 20;
  }

  .panel header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.7rem 0.9rem;
    border-bottom: 1px solid var(--border);
  }

  .close {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }

  .empty {
    color: var(--muted);
    font-size: 0.85rem;
    padding: 1rem 0.9rem;
    margin: 0;
  }

  .item {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    padding: 0.7rem 0.9rem;
    cursor: pointer;
  }

  .item:hover {
    background: var(--panel-2);
  }

  .item-task {
    font-size: 0.75rem;
    color: var(--muted);
  }

  .item-prompt {
    color: var(--text);
    font-size: 0.85rem;
  }

  .toasts {
    position: fixed;
    bottom: 1.2rem;
    right: 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    z-index: 50;
    max-width: 360px;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    background: var(--panel);
    border: 1px solid var(--warn);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }

  .toast-body {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text);
    padding: 0.7rem 0.9rem;
    cursor: pointer;
    flex: 1;
  }

  .toast-title {
    font-size: 0.7rem;
    font-weight: 700;
    color: var(--warn);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .toast-task {
    font-size: 0.78rem;
    color: var(--muted);
  }

  .toast-prompt {
    font-size: 0.88rem;
  }

  .toast-close {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0.5rem 0.6rem;
  }
</style>
