<script lang="ts">
  import type { TaskColumn } from '../types'

  import {
    SlidersHorizontal,
    ArrowLeftRight,
    Trash2,
    X,
    Circle,
    ListTodo,
    CheckCircle2,
    CircleSlash,
    ChevronUp
  } from '@lucide/svelte'

  import { Badge } from './ui/badge'
  import { Button, buttonVariants } from './ui/button'
  import * as AlertDialog from './ui/alert-dialog'
  import * as DropdownMenu from './ui/dropdown-menu'

  // A floating bottom bar (Jira-style) for the board's multi-select mode. It owns
  // its own modals/menus and reports whether any is open via `dialogOpen`, so the
  // page can let Escape close a modal first rather than exiting bulk mode.
  let {
    count,
    onClear,
    onEditFields,
    onChangeStatus,
    onDelete,
    dialogOpen = $bindable(false)
  }: {
    count: number
    onClear: () => void
    onEditFields: (fields: { hold?: boolean; blocking?: boolean }) => Promise<void>
    onChangeStatus: (column: TaskColumn) => Promise<void>
    onDelete: () => Promise<void>
    dialogOpen?: boolean
  } = $props()

  // The destinations the operator may bulk-move into (never In Progress / In
  // Review, which the agent owns). "Done" closes the linked tickets.
  const STATUS_OPTIONS: { column: TaskColumn; label: string; icon: typeof Circle }[] = [
    { column: 'available', label: 'Available', icon: Circle },
    { column: 'todo', label: 'To Do', icon: ListTodo },
    { column: 'done', label: 'Done (closes ticket)', icon: CheckCircle2 },
    { column: 'ignored', label: 'Ignored', icon: CircleSlash }
  ]

  // A three-way per-field choice in the Edit fields modal.
  type FieldChoice = 'keep' | 'true' | 'false'

  let editOpen = $state(false)
  let deleteOpen = $state(false)
  let statusOpen = $state(false)
  let busy = $state(false)

  let holdChoice = $state<FieldChoice>('keep')
  let blockingChoice = $state<FieldChoice>('keep')

  // Let the page suppress its Escape-to-exit while one of our overlays is open.
  $effect(() => {
    dialogOpen = editOpen || deleteOpen || statusOpen
  })

  const noneSelected = $derived(count === 0)

  function openEdit() {
    holdChoice = 'keep'
    blockingChoice = 'keep'
    editOpen = true
  }

  function choiceToBool(choice: FieldChoice): boolean | undefined {
    if (choice === 'keep') {
      return undefined
    }
    return choice === 'true'
  }

  async function saveEdit() {
    const fields: { hold?: boolean; blocking?: boolean } = {}
    const hold = choiceToBool(holdChoice)
    const blocking = choiceToBool(blockingChoice)
    if (hold !== undefined) {
      fields.hold = hold
    }
    if (blocking !== undefined) {
      fields.blocking = blocking
    }
    // Nothing chosen: just close, don't hit the API for a no-op.
    if (fields.hold === undefined && fields.blocking === undefined) {
      editOpen = false
      return
    }
    busy = true
    try {
      await onEditFields(fields)
      editOpen = false
    } finally {
      busy = false
    }
  }

  async function pickStatus(column: TaskColumn) {
    statusOpen = false
    busy = true
    try {
      await onChangeStatus(column)
    } finally {
      busy = false
    }
  }

  async function confirmDelete() {
    busy = true
    try {
      await onDelete()
      deleteOpen = false
    } finally {
      busy = false
    }
  }
</script>

<div
  class="fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl border border-border bg-card px-3 py-2 shadow-2xl"
  role="toolbar"
  aria-label="Bulk actions"
>
  <!-- Far left: count badge + the word "selected". -->
  <div class="flex items-center gap-2 pl-1 pr-1">
    <Badge variant="default" class="tabular-nums">{count}</Badge>
    <span class="text-sm text-muted-foreground">selected</span>
  </div>

  <div class="mx-1 h-6 w-px bg-border" aria-hidden="true"></div>

  <!-- Middle: the three actions. -->
  <Button variant="ghost" size="sm" disabled={noneSelected || busy} onclick={openEdit}>
    <SlidersHorizontal class="size-4" />
    Edit fields
  </Button>

  <DropdownMenu.Root bind:open={statusOpen}>
    <DropdownMenu.Trigger
      disabled={noneSelected || busy}
      class={buttonVariants({ variant: 'ghost', size: 'sm' })}
    >
      <ArrowLeftRight class="size-4" />
      Change status
      <ChevronUp class="size-4 opacity-60" />
    </DropdownMenu.Trigger>
    <DropdownMenu.Content side="top" align="center" class="min-w-44">
      {#each STATUS_OPTIONS as option (option.column)}
        {@const Icon = option.icon}
        <DropdownMenu.Item onclick={() => pickStatus(option.column)}>
          <Icon class="size-4" />
          {option.label}
        </DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>

  <Button
    variant="ghost"
    size="sm"
    class="text-destructive hover:bg-destructive/10 hover:text-destructive"
    disabled={noneSelected || busy}
    onclick={() => (deleteOpen = true)}
  >
    <Trash2 class="size-4" />
    Delete
  </Button>

  <div class="mx-1 h-6 w-px bg-border" aria-hidden="true"></div>

  <!-- Far right: clear selection and exit. -->
  <Button
    variant="ghost"
    size="icon"
    title="Clear all"
    aria-label="Clear all and exit multi-select"
    onclick={onClear}
  >
    <X class="size-4" />
  </Button>
</div>

<!-- Edit fields modal: one row per editable field, each a "keep as is / true /
     false" dropdown. Native selects render outside the dialog's focus scope, so
     they never fight the modal. -->
<AlertDialog.Root bind:open={editOpen}>
  <AlertDialog.Content class="sm:max-w-md">
    <AlertDialog.Header>
      <AlertDialog.Title>Edit fields</AlertDialog.Title>
      <AlertDialog.Description>
        Apply to {count} selected {count === 1 ? 'task' : 'tasks'}. Leave a field on "Keep as is" to
        not change it.
      </AlertDialog.Description>
    </AlertDialog.Header>

    <div class="grid grid-cols-2 items-center gap-x-4 gap-y-3 py-2">
      <label for="bulk-hold" class="text-sm font-medium">On hold</label>
      <select
        id="bulk-hold"
        bind:value={holdChoice}
        class="h-9 rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <option value="keep">Keep as is</option>
        <option value="true">True</option>
        <option value="false">False</option>
      </select>

      <label for="bulk-blocking" class="text-sm font-medium">Is blocking</label>
      <select
        id="bulk-blocking"
        bind:value={blockingChoice}
        class="h-9 rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <option value="keep">Keep as is</option>
        <option value="true">True</option>
        <option value="false">False</option>
      </select>
    </div>

    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={busy}>Cancel</AlertDialog.Cancel>
      <Button onclick={saveEdit} disabled={busy}>{busy ? 'Saving…' : 'Save'}</Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<!-- Delete confirmation. -->
<AlertDialog.Root bind:open={deleteOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>
        Delete {count} {count === 1 ? 'task' : 'tasks'}?
      </AlertDialog.Title>
      <AlertDialog.Description>
        This permanently removes the selected {count === 1 ? 'card' : 'cards'} and their activity
        history from Seraphim. The source issues on GitHub/Jira are not affected. This cannot be
        undone.
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={busy}>Cancel</AlertDialog.Cancel>
      <Button variant="destructive" onclick={confirmDelete} disabled={busy}>
        {busy ? 'Deleting…' : 'Delete'}
      </Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
