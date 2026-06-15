<script lang="ts">
  import type { Repository } from '$lib/types'

  import { onMount } from 'svelte'
  import { goto } from '$app/navigation'
  import { toast } from 'svelte-sonner'

  import { createInternalTask, listRepos } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as Select from '$lib/components/ui/select'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Switch } from '$lib/components/ui/switch'

  // The sentinel value for "no repo": Select needs a non-empty string value.
  const NO_REPO = '__none__'

  let title = $state('')
  let description = $state('')
  let open = $state(true)
  let saving = $state(false)
  let repos = $state<Repository[]>([])
  let repoId = $state(NO_REPO)

  const repoLabel = $derived(
    repoId === NO_REPO
      ? 'No repo (tracking only)'
      : (repos.find((repo) => repo.id === repoId)?.full_name ?? 'Select a repo')
  )

  onMount(async () => {
    repos = await listRepos()
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
        repo_id: repoId === NO_REPO ? null : repoId
      })
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
        <Label for="repo">Target repository</Label>
        <Select.Root type="single" value={repoId} onValueChange={(value) => (repoId = value)}>
          <Select.Trigger id="repo" class="w-full">{repoLabel}</Select.Trigger>
          <Select.Content>
            <Select.Item value={NO_REPO} label="No repo (tracking only)">
              No repo (tracking only)
            </Select.Item>
            {#each repos as repo (repo.id)}
              <Select.Item value={repo.id} label={repo.full_name}>{repo.full_name}</Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
        <span class="text-xs text-muted-foreground">
          Pick a repo and the agent will branch and open a PR there when the ticket reaches To Do.
          Leave as tracking-only to assign a repo later.
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
