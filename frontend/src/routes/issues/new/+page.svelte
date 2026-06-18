<script lang="ts">
  import type { Repository } from '$lib/types'

  import { onMount } from 'svelte'
  import { goto } from '$app/navigation'
  import { toast } from 'svelte-sonner'

  import { createInternalTask, listRepos, uploadTaskAttachment } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Switch } from '$lib/components/ui/switch'
  import RepoMultiSelect from '$lib/components/RepoMultiSelect.svelte'

  // Remember the last repo selection so creating many tickets back to back does
  // not mean re-picking the same repos every time (issue #189). Restored on mount.
  const REPO_SELECTION_STORAGE_KEY = 'seraphim:new-issue:repo-ids'

  let title = $state('')
  let description = $state('')
  let open = $state(true)
  let saving = $state(false)
  let repos = $state<Repository[]>([])
  let selectedRepoIds = $state<string[]>([])
  // Files the operator attached (issue #291); uploaded to the ticket once it is
  // created (attachments key on a task id, which only exists after creation).
  let attachmentFiles = $state<File[]>([])

  function onAttachmentsChange(event: Event) {
    const input = event.target as HTMLInputElement
    attachmentFiles = input.files ? Array.from(input.files) : []
  }

  onMount(async () => {
    repos = await listRepos()
    // Restore the saved selection, dropping any repos that no longer exist.
    try {
      const saved = localStorage.getItem(REPO_SELECTION_STORAGE_KEY)
      if (saved) {
        const savedIds = JSON.parse(saved) as string[]
        const known = new Set(repos.map((repo) => repo.id))
        selectedRepoIds = savedIds.filter((id) => known.has(id))
      }
    } catch (error) {
      console.debug('failed to restore saved repo selection', error)
    }
  })

  // Persist the selection on every change so it survives navigation and reloads.
  $effect(() => {
    try {
      localStorage.setItem(REPO_SELECTION_STORAGE_KEY, JSON.stringify(selectedRepoIds))
    } catch (error) {
      console.debug('failed to save repo selection', error)
    }
  })

  async function submit() {
    if (!title.trim()) {
      toast.error('A title is required')
      return
    }
    saving = true
    try {
      const task = await createInternalTask({
        title: title.trim(),
        body: description.trim(),
        state: open ? 'open' : 'closed',
        repo_ids: selectedRepoIds
      })
      // Upload any attached files to the freshly created ticket (issue #291).
      for (const file of attachmentFiles) {
        await uploadTaskAttachment(task.id, file)
      }
      toast.success('Issue created')
      goto(`/task/${task.id}`)
    } catch {
      toast.error('Could not create the issue')
      saving = false
    }
  }
</script>

<div class="mx-auto max-w-2xl space-y-5 px-6 py-6">
  <div>
    <a href="/" class="text-sm text-muted-foreground hover:text-foreground">← Board</a>
    <h1 class="mt-2 text-2xl font-semibold">Create issue</h1>
    <p class="text-sm text-muted-foreground">
      An internal ticket tracked only in Seraphim, with no GitHub or Jira backing. It lands in
      Available; add comments on the task page once it exists.
    </p>
  </div>

  <Card.Root>
    <Card.Content class="space-y-5 pt-6">
      <div class="grid gap-2">
        <Label for="title">Title</Label>
        <Input id="title" placeholder="Short summary" bind:value={title} />
      </div>

      <div class="grid gap-2">
        <Label for="description">Description</Label>
        <Textarea
          id="description"
          rows={8}
          placeholder="What needs doing? Markdown is supported."
          bind:value={description}
          class="resize-y"
        />
      </div>

      <div class="grid gap-2">
        <Label for="attachments">Attachments</Label>
        <input
          id="attachments"
          type="file"
          multiple
          onchange={onAttachmentsChange}
          class="text-sm file:mr-3 file:rounded-md file:border file:border-input file:bg-background file:px-3 file:py-1.5 file:text-sm hover:file:bg-accent"
        />
        <span class="text-xs text-muted-foreground">
          Attach screenshots or log files. The agent sees images as openable refs and inlines small
          text/log files into its brief. Uploaded when the ticket is created.
          {#if attachmentFiles.length}
            <span class="text-foreground">{attachmentFiles.length} selected.</span>
          {/if}
        </span>
      </div>

      <div class="grid gap-2">
        <div class="flex items-center justify-between">
          <Label for="repo">Target repositories</Label>
          {#if selectedRepoIds.length}
            <button
              type="button"
              class="text-xs text-muted-foreground hover:text-foreground"
              onclick={() => (selectedRepoIds = [])}
            >
              Clear ({selectedRepoIds.length})
            </button>
          {/if}
        </div>
        <RepoMultiSelect id="repo" {repos} bind:selected={selectedRepoIds} />
        <span class="text-xs text-muted-foreground">
          Pick one or more repos this ticket affects. The first is the primary one the agent
          branches in; it gets the full list as context and opens a PR in each repo it changes.
          Leave empty to keep the ticket tracking-only and assign repos later. Your selection is
          remembered for the next ticket.
        </span>
      </div>

      <div class="flex items-center gap-2">
        <Switch id="open" bind:checked={open} />
        <Label for="open">{open ? 'Open' : 'Closed'}</Label>
      </div>

      <div class="flex items-center gap-3">
        <Button disabled={saving || !title.trim()} onclick={submit}>
          {saving ? 'Creating…' : 'Create issue'}
        </Button>
        <a href="/" class={buttonVariants({ variant: 'outline' })}>Cancel</a>
      </div>
    </Card.Content>
  </Card.Root>
</div>
