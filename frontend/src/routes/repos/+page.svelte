<script lang="ts">
  import type { RepoDeletionImpact, Repository } from '$lib/types'
  import type { UpsertRepoRequest } from '$lib/api'

  import { onMount } from 'svelte'
  import { toast } from 'svelte-sonner'
  import { CircleCheck, CircleOff, GitBranch, Pencil, Plus, Trash2 } from '@lucide/svelte'

  import { deleteRepo, importOrg, listRepos, repoDeletionImpact, updateRepo } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as AlertDialog from '$lib/components/ui/alert-dialog'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Switch } from '$lib/components/ui/switch'

  let repos = $state<Repository[]>([])
  let importOwner = $state('')
  let importMessage = $state<string | null>(null)

  async function load() {
    repos = await listRepos()
  }

  // The full update body for a quick in-place edit (the sync toggle), built from
  // the row we already have so nothing else changes.
  function repoToBody(repo: Repository): UpsertRepoRequest {
    return {
      full_name: repo.full_name,
      clone_url: repo.clone_url,
      default_branch: repo.default_branch,
      branch_template: repo.branch_template,
      setup_script: repo.setup_script,
      instructions: repo.instructions,
      review_policy: repo.review_policy,
      enabled: repo.enabled,
      sync_issues: repo.sync_issues,
      issue_labels: repo.issue_labels
    }
  }

  // Flip a repo's issue sync from the list, optimistically; revert on failure.
  async function toggleSync(repo: Repository, value: boolean) {
    const previous = repo.sync_issues
    repo.sync_issues = value
    try {
      await updateRepo(repo.id, repoToBody(repo))
    } catch {
      repo.sync_issues = previous
      toast.error('Could not update issue sync')
    }
  }

  // Deleting a repo cascades to every task synced from it (and their logs,
  // decisions, and notes), so confirm first and spell out the impact.
  let deleteTarget = $state<Repository | null>(null)
  let deleteImpact = $state<RepoDeletionImpact | null>(null)
  let deleting = $state(false)

  function askDelete(repo: Repository) {
    deleteTarget = repo
    deleteImpact = null
    repoDeletionImpact(repo.id)
      .then((impact) => {
        // Ignore a late response if the user has since opened a different repo.
        if (deleteTarget?.id === repo.id) {
          deleteImpact = impact
        }
      })
      .catch((error) => console.debug('failed to load deletion impact', error))
  }

  async function confirmDelete() {
    const target = deleteTarget
    if (!target) {
      return
    }
    deleting = true
    try {
      await deleteRepo(target.id)
      deleteTarget = null
      deleteImpact = null
      await load()
    } finally {
      deleting = false
    }
  }

  async function runImportOrg() {
    if (!importOwner.trim()) {
      return
    }
    importMessage = 'Importing…'
    const result = await importOrg(importOwner.trim())
    importMessage = `Discovered ${result.discovered}, imported ${result.imported} new.`
    importOwner = ''
    await load()
  }

  onMount(load)
</script>

<div class="mx-auto max-w-6xl space-y-5 px-6 py-6">
  <div class="flex items-center justify-between gap-3">
    <h1 class="text-2xl font-semibold">Repositories</h1>
    <a href="/repos/new" class={buttonVariants({ variant: 'default' })}>
      <Plus class="size-4" /> Add repository
    </a>
  </div>

  <Card.Root>
    <Card.Header>
      <Card.Title>Import from org</Card.Title>
      <Card.Description>
        Pull in every repository under a GitHub org/user at once. New repos are added with issue-sync
        on and your default branch template + review policy; existing repos are left untouched.
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <div class="flex items-center gap-3">
        <Input class="max-w-xs" placeholder="org or user (e.g. MooreslabAI)" bind:value={importOwner} />
        <Button variant="outline" onclick={runImportOrg}>Import</Button>
        {#if importMessage}<span class="text-sm text-muted-foreground">{importMessage}</span>{/if}
      </div>
    </Card.Content>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>Managed repositories</Card.Title>
    </Card.Header>
    <Card.Content class="divide-y divide-border">
      {#if repos.length === 0}
        <p class="text-sm text-muted-foreground">
          No repositories yet. Add one, or import a whole org above.
        </p>
      {/if}
      {#each repos as repo (repo.id)}
        <div class="flex items-center gap-3 py-3 first:pt-0 last:pb-0">
          <!-- Enabled / disabled indicator, furthest left. -->
          <span class="flex-none" title={repo.enabled ? 'Enabled' : 'Disabled'}>
            {#if repo.enabled}
              <CircleCheck class="size-4 text-success" aria-label="Enabled" />
            {:else}
              <CircleOff class="size-4 text-muted-foreground" aria-label="Disabled" />
            {/if}
          </span>

          <!-- Quick issue-sync toggle. -->
          <span
            class="flex-none"
            title={repo.sync_issues ? 'Syncing issues (toggle off)' : 'Not syncing issues (toggle on)'}
          >
            <Switch
              checked={repo.sync_issues}
              onCheckedChange={(value) => toggleSync(repo, value)}
              aria-label="Sync issues"
            />
          </span>

          <!-- Name + details. -->
          <div class="min-w-0 flex-1">
            <div
              class="truncate font-medium {repo.enabled ? '' : 'text-muted-foreground'}"
              title={repo.clone_url}
            >
              {repo.full_name}
            </div>
            <div class="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
              <span class="inline-flex items-center gap-1">
                <GitBranch class="size-3 flex-none" />
                {repo.default_branch}
              </span>
              <span>{repo.review_policy ? repo.review_policy.replace(/_/g, ' ') : 'inherit review'}</span>
              <span>branch: {repo.branch_template || 'inherits global'}</span>
              {#if repo.issue_labels.length}
                <span>labels: {repo.issue_labels.join(', ')}</span>
              {/if}
              {#if repo.setup_script.trim()}<span>setup script</span>{/if}
              {#if !repo.sync_issues}<span class="text-muted-foreground/70">sync off</span>{/if}
            </div>
          </div>

          <!-- Actions. -->
          <div class="flex flex-none gap-1">
            <a
              href={`/repos/${repo.id}/edit`}
              title="Edit"
              aria-label="Edit"
              class={buttonVariants({ variant: 'ghost', size: 'icon' })}
            >
              <Pencil class="size-4" />
            </a>
            <Button
              variant="ghost"
              size="icon"
              title="Delete"
              aria-label="Delete"
              class="text-destructive hover:text-destructive"
              onclick={() => askDelete(repo)}
            >
              <Trash2 class="size-4" />
            </Button>
          </div>
        </div>
      {/each}
    </Card.Content>
  </Card.Root>
</div>

<AlertDialog.Root
  open={deleteTarget !== null}
  onOpenChange={(open) => {
    if (!open) {
      deleteTarget = null
    }
  }}
>
  <AlertDialog.Content>
    {#if deleteTarget}
      <AlertDialog.Header>
        <AlertDialog.Title>Delete {deleteTarget.full_name}?</AlertDialog.Title>
        <AlertDialog.Description>
          This permanently removes the repository and everything synced from it. This cannot be
          undone.
        </AlertDialog.Description>
      </AlertDialog.Header>

      <div class="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm">
        {#if deleteImpact}
          <p class="mb-1 font-medium">This will also delete:</p>
          <ul class="list-disc space-y-0.5 pl-5 text-muted-foreground">
            <li>
              {deleteImpact.tasks}
              {deleteImpact.tasks === 1 ? 'task' : 'tasks'} (issues on the board)
            </li>
            <li>
              {deleteImpact.turns} agent {deleteImpact.turns === 1 ? 'turn' : 'turns'} with
              {deleteImpact.events} activity log {deleteImpact.events === 1 ? 'event' : 'events'}
            </li>
            <li>
              {deleteImpact.questions}
              {deleteImpact.questions === 1 ? 'decision' : 'decisions'} and
              {deleteImpact.suggestions} environment {deleteImpact.suggestions === 1
                ? 'note'
                : 'notes'}
            </li>
          </ul>
        {:else}
          <p class="text-muted-foreground">Counting what will be removed…</p>
        {/if}
      </div>

      <AlertDialog.Footer>
        <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
        <AlertDialog.Action
          class={buttonVariants({ variant: 'destructive' })}
          disabled={deleting}
          onclick={confirmDelete}
        >
          {deleting ? 'Deleting…' : 'Delete repository'}
        </AlertDialog.Action>
      </AlertDialog.Footer>
    {/if}
  </AlertDialog.Content>
</AlertDialog.Root>
