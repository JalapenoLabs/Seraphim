<script lang="ts">
  // The compose page (issue #181): a dedicated 50/50 workspace for drafting many
  // issues at once with a second, on-demand Claude session that is fully separate
  // from the board agent. Left: the chat with the assistant and a resizable
  // composer (or, when a draft is selected, that draft's editor). Right: the list
  // of drafts the assistant has scoped. Top-right: Reset, Target, and Bulk create.
  import type { AgentEvent, IssueDraft, ComposeTarget, Repository } from '$lib/types'

  import { onMount, onDestroy, tick } from 'svelte'
  import { toast } from 'svelte-sonner'
  import { RotateCcw, Send, Trash2, ChevronLeft } from '@lucide/svelte'

  import {
    getComposeState,
    sendComposeMessage,
    resetCompose,
    bulkCreateDrafts,
    updateDraft,
    deleteDraft,
    listRepos
  } from '$lib/api'

  import * as Select from '$lib/components/ui/select'
  import * as AlertDialog from '$lib/components/ui/alert-dialog'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { buttonVariants } from '$lib/components/ui/button'
  import Stats from '$lib/components/Stats.svelte'
  import Markdown from '$lib/components/Markdown.svelte'

  type StreamEvent = Pick<AgentEvent, 'type' | 'payload' | 'created_at'>

  const NO_REPO = '__none__'

  let events = $state<StreamEvent[]>([])
  let drafts = $state<IssueDraft[]>([])
  let repos = $state<Repository[]>([])
  let running = $state(false)
  let message = $state('')
  let target = $state<ComposeTarget>('internal')
  let creating = $state(false)
  let stream: EventSource | null = null

  // The draft currently open in the editor (replacing the chat), if any.
  let selectedId = $state<string | null>(null)
  const selectedDraft = $derived(drafts.find((draft) => draft.id === selectedId) ?? null)
  // The editor's working copy, seeded when a draft is opened and saved on demand.
  let editTitle = $state('')
  let editBody = $state('')
  let editRepo = $state(NO_REPO)

  const targetLabels = {
    internal: 'Internal',
    github: 'GitHub',
    jira: 'Jira'
  } as const satisfies Record<ComposeTarget, string>

  // The readable text of one transcript event, by type. Tool calls show the tool
  // name; everything else carries a `text` payload.
  function eventText(event: StreamEvent): string {
    const payload = event.payload as { text?: string; name?: string } | null
    if (event.type === 'tool_use') {
      return payload?.name ?? 'tool'
    }
    return payload?.text ?? ''
  }

  let logEl = $state<HTMLDivElement>()
  async function scrollToBottom() {
    await tick()
    logEl?.scrollTo({ top: logEl.scrollHeight, behavior: 'smooth' })
  }

  async function loadState() {
    const state = await getComposeState()
    events = state.events.map((event) => ({
      type: event.type,
      payload: event.payload,
      created_at: event.created_at
    }))
    drafts = state.drafts
    running = state.running
  }

  onMount(() => {
    loadState().then(scrollToBottom)
    listRepos().then((list) => (repos = list))

    stream = new EventSource('/api/v1/compose/stream')
    stream.addEventListener('compose', (raw) => {
      try {
        const event = JSON.parse((raw as MessageEvent).data) as StreamEvent
        events = [...events, event]
        // A turn's terminal `result` means the assistant is free again.
        if (event.type === 'result') {
          running = false
        }
        void scrollToBottom()
      } catch (error) {
        console.debug('failed to parse compose event', error)
      }
    })
    // Drafts or stats changed (e.g. the assistant called seraphim-draft); refetch.
    stream.addEventListener('compose_changed', () => {
      getComposeState()
        .then((state) => {
          drafts = state.drafts
          running = state.running
        })
        .catch((error) => console.debug('failed to refresh compose state', error))
    })
  })

  onDestroy(() => stream?.close())

  async function send() {
    const text = message.trim()
    if (!text || running) {
      return
    }
    message = ''
    running = true
    // The server records and streams the message back as the turn's first event
    // (so every viewer sees the same transcript), so we don't append it locally.
    try {
      await sendComposeMessage(text)
    } catch {
      toast.error('Could not reach the assistant')
      running = false
    }
  }

  function onComposerKeydown(event: KeyboardEvent) {
    // Enter sends; Shift+Enter inserts a newline (chat convention).
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      void send()
    }
  }

  function openDraft(draft: IssueDraft) {
    selectedId = draft.id
    editTitle = draft.title
    editBody = draft.body
    editRepo = draft.repo_id ?? NO_REPO
  }

  function closeDraft() {
    selectedId = null
  }

  async function saveDraft() {
    if (!selectedDraft) {
      return
    }
    try {
      const updated = await updateDraft(selectedDraft.id, {
        title: editTitle.trim(),
        body: editBody,
        repo_id: editRepo === NO_REPO ? null : editRepo
      })
      const index = drafts.findIndex((draft) => draft.id === updated.id)
      if (index !== -1) {
        drafts[index] = updated
      }
      toast.success('Draft saved')
    } catch {
      toast.error('Could not save the draft')
    }
  }

  async function removeDraft(id: string) {
    try {
      await deleteDraft(id)
      drafts = drafts.filter((draft) => draft.id !== id)
      if (selectedId === id) {
        selectedId = null
      }
    } catch {
      toast.error('Could not delete the draft')
    }
  }

  let resetOpen = $state(false)
  async function confirmReset() {
    try {
      await resetCompose()
      events = []
      drafts = []
      selectedId = null
      running = false
      resetOpen = false
      toast.success('Compose reset')
    } catch {
      toast.error('Could not reset')
    }
  }

  async function bulkCreate() {
    if (!drafts.length || creating) {
      return
    }
    creating = true
    try {
      const result = await bulkCreateDrafts(target)
      if (result.errors.length) {
        toast.error(`Created ${result.created}; ${result.errors.length} failed: ${result.errors[0]}`)
      } else {
        toast.success(`Created ${result.created} ${targetLabels[target]} issue${result.created === 1 ? '' : 's'}`)
      }
    } catch {
      toast.error('Bulk create failed')
    } finally {
      creating = false
    }
  }

  function repoName(repoId: string | null): string {
    if (!repoId) {
      return ''
    }
    return repos.find((repo) => repo.id === repoId)?.full_name ?? ''
  }

  const editRepoLabel = $derived(
    editRepo === NO_REPO ? 'No repo' : (repos.find((repo) => repo.id === editRepo)?.full_name ?? 'Select a repo')
  )
</script>

<div class="flex h-full flex-col gap-3 p-4">
  <!-- Top: the assistant's own stats bar, plus the three action buttons. -->
  <header class="flex items-start justify-between gap-4">
    <div class="min-w-0 flex-1">
      <Stats compose />
    </div>
    <div class="flex shrink-0 items-center gap-2">
      <button
        type="button"
        onclick={() => (resetOpen = true)}
        class={buttonVariants({ variant: 'outline', size: 'sm' })}
      >
        <RotateCcw class="mr-1.5 size-4" /> Reset
      </button>

      <Select.Root type="single" value={target} onValueChange={(value) => (target = value as ComposeTarget)}>
        <Select.Trigger class="w-32">Target: {targetLabels[target]}</Select.Trigger>
        <Select.Content>
          <Select.Item value="internal" label="Internal">Internal</Select.Item>
          <Select.Item value="github" label="GitHub">GitHub</Select.Item>
          <Select.Item value="jira" label="Jira">Jira</Select.Item>
        </Select.Content>
      </Select.Root>

      <button
        type="button"
        onclick={bulkCreate}
        disabled={!drafts.length || creating}
        class={buttonVariants({ variant: 'default', size: 'sm' })}
      >
        {creating ? 'Creating…' : `Create ${drafts.length || ''}`}
      </button>
    </div>
  </header>

  <!-- 50/50 split: chat / draft editor on the left, draft list on the right. -->
  <div class="flex min-h-0 flex-1 gap-3">
    <!-- Left panel -->
    <section class="flex min-h-0 w-1/2 flex-col rounded-lg border border-border bg-card">
      {#if selectedDraft}
        <!-- Draft editor: replaces the chat, writes to the stored draft. -->
        <div class="flex items-center justify-between border-b border-border px-3 py-2">
          <button type="button" onclick={closeDraft} class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground">
            <ChevronLeft class="size-4" /> Back to chat
          </button>
          <button
            type="button"
            onclick={() => removeDraft(selectedDraft.id)}
            class="flex items-center gap-1 text-sm text-destructive hover:opacity-80"
          >
            <Trash2 class="size-4" /> Delete
          </button>
        </div>
        <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
          <div class="grid gap-1.5">
            <Label for="draft-title">Title</Label>
            <Input id="draft-title" bind:value={editTitle} />
          </div>
          <div class="grid gap-1.5">
            <Label for="draft-repo">Target repository</Label>
            <Select.Root type="single" value={editRepo} onValueChange={(value) => (editRepo = value)}>
              <Select.Trigger id="draft-repo" class="w-full">{editRepoLabel}</Select.Trigger>
              <Select.Content>
                <Select.Item value={NO_REPO} label="No repo">No repo</Select.Item>
                {#each repos as repo (repo.id)}
                  <Select.Item value={repo.id} label={repo.full_name}>{repo.full_name}</Select.Item>
                {/each}
              </Select.Content>
            </Select.Root>
          </div>
          <div class="grid gap-1.5">
            <Label for="draft-body">Body (Markdown)</Label>
            <Textarea id="draft-body" bind:value={editBody} rows={12} class="resize-y font-mono text-sm" />
          </div>
          {#if editBody.trim()}
            <div class="rounded-md border border-border p-3">
              <div class="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Preview</div>
              <Markdown source={editBody} />
            </div>
          {/if}
        </div>
        <div class="border-t border-border p-3">
          <button type="button" onclick={saveDraft} class={buttonVariants({ variant: 'default' }) + ' w-full'}>
            Save draft
          </button>
        </div>
      {:else}
        <!-- Chat transcript + composer. -->
        <div bind:this={logEl} class="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          {#if events.length === 0}
            <p class="text-sm text-muted-foreground">
              Talk to the assistant to scope out a batch of issues. It drafts them on the right; tweak
              and Create when you are ready.
            </p>
          {/if}
          {#each events as event, index (index)}
            {#if event.type === 'prompt'}
              <div class="ml-auto max-w-[85%] rounded-lg bg-primary/10 px-3 py-2 text-sm">
                {eventText(event)}
              </div>
            {:else if event.type === 'assistant_text'}
              <div class="max-w-[90%] text-sm">
                <Markdown source={eventText(event)} />
              </div>
            {:else if event.type === 'thinking'}
              <div class="max-w-[90%] text-xs italic text-muted-foreground">{eventText(event)}</div>
            {:else if event.type === 'tool_use'}
              <div class="font-mono text-xs text-muted-foreground">⚙ {eventText(event)}</div>
            {/if}
          {/each}
          {#if running}
            <div class="text-xs italic text-muted-foreground">The assistant is thinking…</div>
          {/if}
        </div>
        <div class="border-t border-border p-3">
          <Textarea
            bind:value={message}
            onkeydown={onComposerKeydown}
            placeholder="Describe the work, or ask the assistant to help scope it. Enter to send, Shift+Enter for a new line."
            rows={3}
            disabled={running}
            class="resize-y"
          />
          <div class="mt-2 flex justify-end">
            <button
              type="button"
              onclick={send}
              disabled={running || !message.trim()}
              class={buttonVariants({ variant: 'default', size: 'sm' })}
            >
              <Send class="mr-1.5 size-4" /> Send
            </button>
          </div>
        </div>
      {/if}
    </section>

    <!-- Right panel: the draft list. -->
    <section class="flex min-h-0 w-1/2 flex-col rounded-lg border border-border bg-card">
      <div class="border-b border-border px-4 py-2 text-sm font-semibold">
        Drafts {drafts.length ? `(${drafts.length})` : ''}
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto p-2">
        {#if drafts.length === 0}
          <p class="px-2 py-3 text-sm text-muted-foreground">No drafts yet.</p>
        {/if}
        {#each drafts as draft (draft.id)}
          <button
            type="button"
            onclick={() => openDraft(draft)}
            class="flex w-full flex-col gap-0.5 rounded-md px-3 py-2 text-left hover:bg-secondary/50 {selectedId === draft.id ? 'bg-secondary/60' : ''}"
          >
            <span class="truncate text-sm font-medium">{draft.title}</span>
            <span class="flex items-center gap-2 text-xs text-muted-foreground">
              {#if repoName(draft.repo_id)}
                <span class="truncate">{repoName(draft.repo_id)}</span>
              {:else}
                <span>No repo</span>
              {/if}
            </span>
          </button>
        {/each}
      </div>
    </section>
  </div>
</div>

<AlertDialog.Root bind:open={resetOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Reset compose?</AlertDialog.Title>
      <AlertDialog.Description>
        This clears every draft and wipes the assistant's conversation history. It does not touch
        the main agent or anything on the board. This cannot be undone.
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action onclick={confirmReset}>Reset</AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
