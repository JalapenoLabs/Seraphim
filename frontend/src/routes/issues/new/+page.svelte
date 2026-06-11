<script lang="ts">
  import { goto } from '$app/navigation'
  import { toast } from 'svelte-sonner'

  import { createInternalTask } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Switch } from '$lib/components/ui/switch'

  let title = $state('')
  let description = $state('')
  let open = $state(true)
  let saving = $state(false)

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
        state: open ? 'open' : 'closed'
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
