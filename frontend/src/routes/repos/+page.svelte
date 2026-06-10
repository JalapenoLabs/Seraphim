<script lang="ts">
  import type { Repository, ReviewPolicy } from '$lib/types'

  import { onMount } from 'svelte'
  import { Pencil, Trash2 } from '@lucide/svelte'

  import { deleteRepo, getSettings, importOrg, listRepos, updateRepo, upsertRepo } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as Select from '$lib/components/ui/select'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Switch } from '$lib/components/ui/switch'
  import { Badge } from '$lib/components/ui/badge'

  // Form preferences (the defaults you tend to reuse) are remembered locally so a
  // new repo form pre-fills with your last choices.
  const PREFS_KEY = 'seraphim.repoFormPrefs'

  type FormState = {
    full_name: string
    clone_url: string
    default_branch: string
    branch_template: string
    review_policy: ReviewPolicy | ''
    instructions: string
    setup_script: string
    enabled: boolean
    sync_issues: boolean
    issue_labels: string
  }

  type FormPrefs = Pick<
    FormState,
    'default_branch' | 'branch_template' | 'review_policy' | 'enabled' | 'sync_issues' | 'issue_labels'
  >

  function loadPrefs(): FormPrefs {
    const fallback: FormPrefs = {
      default_branch: 'main',
      // Blank inherits the global template set in Settings.
      branch_template: '',
      review_policy: '',
      enabled: true,
      sync_issues: true,
      issue_labels: ''
    }
    if (typeof localStorage === 'undefined') {
      return fallback
    }
    try {
      const stored = localStorage.getItem(PREFS_KEY)
      return stored ? { ...fallback, ...JSON.parse(stored) } : fallback
    } catch {
      return fallback
    }
  }

  function savePrefs(form: FormState) {
    if (typeof localStorage === 'undefined') {
      return
    }
    const prefs: FormPrefs = {
      default_branch: form.default_branch,
      branch_template: form.branch_template,
      review_policy: form.review_policy,
      enabled: form.enabled,
      sync_issues: form.sync_issues,
      issue_labels: form.issue_labels
    }
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs))
  }

  function emptyForm(): FormState {
    return { full_name: '', clone_url: '', instructions: '', setup_script: '', ...loadPrefs() }
  }

  let repos = $state<Repository[]>([])
  // The global branch template, shown as the placeholder when a repo's override
  // is blank (it inherits this).
  let globalBranchTemplate = $state('seraphim/issue-{number}-{slug}')
  let form = $state<FormState>(emptyForm())
  // The id of the repo being edited, or null when adding a new one. Editing
  // updates that row by id (rename-safe); adding upserts by full name.
  let editingId = $state<string | null>(null)
  let importOwner = $state('')
  let importMessage = $state<string | null>(null)

  // The review-policy select uses an "inherit" sentinel since Bits UI dislikes an
  // empty-string option value; the form keeps '' to mean "inherit default".
  const policyValue = $derived(form.review_policy === '' ? 'inherit' : form.review_policy)
  const policyLabel = $derived(
    form.review_policy === '' ? 'inherit default' : form.review_policy.replace(/_/g, ' ')
  )

  function choosePolicy(value: string) {
    form.review_policy = value === 'inherit' ? '' : (value as ReviewPolicy)
  }

  async function load() {
    const [loadedRepos, settings] = await Promise.all([listRepos(), getSettings()])
    repos = loadedRepos
    globalBranchTemplate = settings.default_branch_template
  }

  function clearForm() {
    editingId = null
    form = emptyForm()
  }

  function edit(repo: Repository) {
    editingId = repo.id
    form = {
      full_name: repo.full_name,
      clone_url: repo.clone_url,
      default_branch: repo.default_branch,
      branch_template: repo.branch_template ?? '',
      review_policy: repo.review_policy ?? '',
      instructions: repo.instructions,
      setup_script: repo.setup_script,
      enabled: repo.enabled,
      sync_issues: repo.sync_issues,
      issue_labels: repo.issue_labels.join(', ')
    }
  }

  async function submit() {
    if (!form.full_name.trim()) {
      return
    }
    const cloneUrl = form.clone_url.trim() || `https://github.com/${form.full_name.trim()}.git`
    const labels = form.issue_labels
      .split(',')
      .map((label) => label.trim())
      .filter(Boolean)
    const body = {
      full_name: form.full_name.trim(),
      clone_url: cloneUrl,
      default_branch: form.default_branch,
      // Blank inherits the global template (sent as null, like review policy).
      branch_template: form.branch_template.trim() || null,
      review_policy: form.review_policy === '' ? null : form.review_policy,
      instructions: form.instructions,
      setup_script: form.setup_script,
      enabled: form.enabled,
      sync_issues: form.sync_issues,
      issue_labels: labels
    }
    if (editingId) {
      await updateRepo(editingId, body)
    } else {
      await upsertRepo(body)
    }
    savePrefs(form)
    clearForm()
    await load()
  }

  async function remove(repoId: string) {
    await deleteRepo(repoId)
    await load()
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

<div class="mx-auto max-w-4xl space-y-5 px-6 py-6">
  <h1 class="text-2xl font-semibold">Repositories</h1>

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

  {#if repos.length}
    <Card.Root>
      <Card.Header>
        <Card.Title>Managed repositories</Card.Title>
      </Card.Header>
      <Card.Content class="divide-y divide-border">
        {#each repos as repo (repo.id)}
          <div class="flex items-center justify-between gap-3 py-3 first:pt-0 last:pb-0">
            <div class="min-w-0">
              <div class="truncate font-medium">{repo.full_name}</div>
              <div class="mt-1 flex flex-wrap items-center gap-2">
                <Badge variant="outline" class="text-muted-foreground">
                  {repo.review_policy ? repo.review_policy.replace(/_/g, ' ') : 'inherit'}
                </Badge>
                {#if repo.sync_issues}
                  <Badge variant="outline" class="border-primary/40 text-primary">syncing</Badge>
                {/if}
                {#if !repo.enabled}
                  <Badge variant="outline" class="text-muted-foreground">disabled</Badge>
                {/if}
              </div>
            </div>
            <div class="flex flex-none gap-1">
              <Button variant="ghost" size="icon" title="Edit" onclick={() => edit(repo)}>
                <Pencil class="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                title="Delete"
                class="text-destructive hover:text-destructive"
                onclick={() => remove(repo.id)}
              >
                <Trash2 class="size-4" />
              </Button>
            </div>
          </div>
        {/each}
      </Card.Content>
    </Card.Root>
  {/if}

  <Card.Root>
    <Card.Header>
      <Card.Title>{editingId ? `Edit ${form.full_name}` : 'Add a repository'}</Card.Title>
    </Card.Header>
    <Card.Content class="space-y-5">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div class="space-y-1.5">
          <Label for="full">Full name (owner/repo)</Label>
          <Input id="full" placeholder="navarrotech/seraphim" bind:value={form.full_name} />
        </div>
        <div class="space-y-1.5">
          <Label for="clone">Clone URL (optional)</Label>
          <Input id="clone" placeholder="defaults from full name" bind:value={form.clone_url} />
        </div>
        <div class="space-y-1.5">
          <Label for="branch">Default branch</Label>
          <Input id="branch" bind:value={form.default_branch} />
        </div>
        <div class="space-y-1.5">
          <Label for="tmpl">Branch template</Label>
          <Input id="tmpl" placeholder={`inherit: ${globalBranchTemplate}`} bind:value={form.branch_template} />
          <p class="text-xs leading-relaxed text-muted-foreground">
            Leave blank to inherit the global template from
            <a href="/settings" class="underline">Settings</a>. Supports
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">{'{number}'}</code> and
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">{'{slug}'}</code>.
          </p>
        </div>
        <div class="space-y-1.5">
          <Label for="rpolicy">Review policy</Label>
          <Select.Root type="single" value={policyValue} onValueChange={choosePolicy}>
            <Select.Trigger id="rpolicy" class="w-full">{policyLabel}</Select.Trigger>
            <Select.Content>
              <Select.Item value="inherit" label="inherit default">inherit default</Select.Item>
              <Select.Item value="auto_squash_merge" label="auto squash merge">auto squash merge</Select.Item>
              <Select.Item value="human_review" label="human review">human review</Select.Item>
              <Select.Item value="none" label="none">none</Select.Item>
            </Select.Content>
          </Select.Root>
        </div>
        <div class="space-y-1.5">
          <Label for="labels">Issue label filter (optional)</Label>
          <Input id="labels" placeholder="comma-separated; blank = all" bind:value={form.issue_labels} />
        </div>
      </div>

      <div class="flex flex-wrap gap-6">
        <div class="flex items-center gap-2">
          <Switch id="enabled" bind:checked={form.enabled} />
          <Label for="enabled">Enabled</Label>
        </div>
        <div class="flex items-center gap-2">
          <Switch id="sync" bind:checked={form.sync_issues} />
          <Label for="sync">Sync issues from this repo</Label>
        </div>
      </div>

      <div class="space-y-1.5">
        <Label for="instr">Repo-specific instructions</Label>
        <Textarea id="instr" rows={3} bind:value={form.instructions} />
        <p class="text-xs leading-relaxed text-muted-foreground">
          Written to <code class="rounded bg-secondary px-1 py-0.5 text-xs">/workspace/{'{repo}'}/CLAUDE.md</code>,
          loaded whenever the agent works in this repo. Put build/test commands and repo-specific
          gotchas here.
        </p>
      </div>

      <div class="space-y-1.5">
        <Label for="rsetup">Setup script (run after clone/checkout)</Label>
        <Textarea id="rsetup" rows={3} bind:value={form.setup_script} />
        <p class="text-xs leading-relaxed text-muted-foreground">
          Runs in this repo after it's cloned/updated, as the
          <code class="rounded bg-secondary px-1 py-0.5 text-xs">node</code> user (passwordless
          <code class="rounded bg-secondary px-1 py-0.5 text-xs">sudo</code> available). Newlines execute
          sequentially, e.g. <code class="rounded bg-secondary px-1 py-0.5 text-xs">pnpm install</code> or
          <code class="rounded bg-secondary px-1 py-0.5 text-xs">yarn install</code>. pnpm, yarn, and npm are
          preinstalled, so skip <code class="rounded bg-secondary px-1 py-0.5 text-xs">corepack enable</code>.
        </p>
      </div>

      <div class="flex items-center gap-3">
        <Button onclick={submit}>{editingId ? 'Update repository' : 'Add repository'}</Button>
        <Button variant="outline" onclick={clearForm}>{editingId ? 'Cancel' : 'Clear'}</Button>
      </div>
    </Card.Content>
  </Card.Root>
</div>
