<script lang="ts">
  // The add / edit repository form, used on its own dedicated page so navigating
  // never resets a form you are mid-edit on. Pass a `repo` to edit it, or nothing
  // to add a new one (which pre-fills from your last-used defaults).
  import type { Repository, ReviewPolicy } from '../types'
  import type { UpsertRepoRequest } from '../api'

  import { onMount } from 'svelte'
  import { goto } from '$app/navigation'
  import { toast } from 'svelte-sonner'

  import { getSettings, updateRepo, upsertRepo } from '../api'
  import * as Card from './ui/card'
  import * as Select from './ui/select'
  import { Button, buttonVariants } from './ui/button'
  import { Input } from './ui/input'
  import { Label } from './ui/label'
  import { Textarea } from './ui/textarea'
  import { Switch } from './ui/switch'

  let { repo = null }: { repo?: Repository | null } = $props()

  // Defaults you tend to reuse are remembered locally so a new-repo form pre-fills
  // with your last choices.
  const PREFS_KEY = 'seraphim.repoFormPrefs'

  type FormState = {
    full_name: string
    clone_url: string
    default_branch: string
    branch_template: string
    review_policy: ReviewPolicy | ''
    instructions: string
    setup_script: string
    setup_script_always_run: boolean
    enabled: boolean
    sync_issues: boolean
    issue_labels: string
  }

  type FormPrefs = Pick<
    FormState,
    | 'default_branch'
    | 'branch_template'
    | 'review_policy'
    | 'enabled'
    | 'sync_issues'
    | 'issue_labels'
    | 'setup_script_always_run'
  >

  function loadPrefs(): FormPrefs {
    const fallback: FormPrefs = {
      default_branch: 'main',
      branch_template: '',
      review_policy: '',
      enabled: true,
      sync_issues: true,
      issue_labels: '',
      setup_script_always_run: false
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

  function savePrefs(state: FormState) {
    if (typeof localStorage === 'undefined') {
      return
    }
    const prefs: FormPrefs = {
      default_branch: state.default_branch,
      branch_template: state.branch_template,
      review_policy: state.review_policy,
      enabled: state.enabled,
      sync_issues: state.sync_issues,
      issue_labels: state.issue_labels,
      setup_script_always_run: state.setup_script_always_run
    }
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs))
  }

  function initialForm(): FormState {
    if (repo) {
      return {
        full_name: repo.full_name,
        clone_url: repo.clone_url,
        default_branch: repo.default_branch,
        branch_template: repo.branch_template ?? '',
        review_policy: repo.review_policy ?? '',
        instructions: repo.instructions,
        setup_script: repo.setup_script,
        setup_script_always_run: repo.setup_script_always_run,
        enabled: repo.enabled,
        sync_issues: repo.sync_issues,
        issue_labels: repo.issue_labels.join(', ')
      }
    }
    return { full_name: '', clone_url: '', instructions: '', setup_script: '', ...loadPrefs() }
  }

  let form = $state<FormState>(initialForm())
  let globalBranchTemplate = $state('seraphim/issue-{number}-{slug}')
  let saving = $state(false)

  // The review-policy select uses an "inherit" sentinel since Bits UI dislikes an
  // empty-string option value; the form keeps '' to mean "inherit default".
  const policyValue = $derived(form.review_policy === '' ? 'inherit' : form.review_policy)
  const policyLabel = $derived(
    form.review_policy === '' ? 'inherit default' : form.review_policy.replace(/_/g, ' ')
  )

  function choosePolicy(value: string) {
    form.review_policy = value === 'inherit' ? '' : (value as ReviewPolicy)
  }

  onMount(async () => {
    try {
      const settings = await getSettings()
      globalBranchTemplate = settings.default_branch_template
    } catch (error) {
      console.debug('failed to load the global branch template', error)
    }
  })

  async function submit() {
    if (!form.full_name.trim()) {
      toast.error('A full name (owner/repo) is required')
      return
    }
    saving = true
    const cloneUrl = form.clone_url.trim() || `https://github.com/${form.full_name.trim()}.git`
    const labels = form.issue_labels
      .split(',')
      .map((label) => label.trim())
      .filter(Boolean)
    const body: UpsertRepoRequest = {
      full_name: form.full_name.trim(),
      clone_url: cloneUrl,
      default_branch: form.default_branch,
      // Blank inherits the global template (sent as null, like review policy).
      branch_template: form.branch_template.trim() || null,
      review_policy: form.review_policy === '' ? null : form.review_policy,
      instructions: form.instructions,
      setup_script: form.setup_script,
      setup_script_always_run: form.setup_script_always_run,
      enabled: form.enabled,
      sync_issues: form.sync_issues,
      issue_labels: labels
    }
    try {
      if (repo) {
        await updateRepo(repo.id, body)
      } else {
        await upsertRepo(body)
        savePrefs(form)
      }
      toast.success(repo ? 'Repository updated' : 'Repository added')
      goto('/repos')
    } catch {
      toast.error('Could not save the repository')
      saving = false
    }
  }
</script>

<Card.Root>
  <Card.Content class="space-y-5 pt-6">
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
      <Textarea id="instr" rows={4} bind:value={form.instructions} class="resize-y" />
      <p class="text-xs leading-relaxed text-muted-foreground">
        Written to <code class="rounded bg-secondary px-1 py-0.5 text-xs">/workspace/{'{repo}'}/CLAUDE.md</code>,
        loaded whenever the agent works in this repo. Put build/test commands and repo-specific
        gotchas here.
      </p>
    </div>

    <div class="space-y-1.5">
      <Label for="rsetup">Setup script (run after clone/checkout)</Label>
      <Textarea id="rsetup" rows={4} bind:value={form.setup_script} class="resize-y" />
      <p class="text-xs leading-relaxed text-muted-foreground">
        Runs in this repo after it's cloned/updated, as the
        <code class="rounded bg-secondary px-1 py-0.5 text-xs">node</code> user (passwordless
        <code class="rounded bg-secondary px-1 py-0.5 text-xs">sudo</code> available). Newlines execute
        sequentially, e.g. <code class="rounded bg-secondary px-1 py-0.5 text-xs">pnpm install</code> or
        <code class="rounded bg-secondary px-1 py-0.5 text-xs">yarn install</code>. pnpm, yarn, and npm are
        preinstalled, so skip <code class="rounded bg-secondary px-1 py-0.5 text-xs">corepack enable</code>.
      </p>
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center gap-2">
        <Switch id="rsetup-always" bind:checked={form.setup_script_always_run} />
        <Label for="rsetup-always">Re-run the setup script before every task</Label>
      </div>
      <p class="text-xs leading-relaxed text-muted-foreground">
        By default the setup script runs only on a fresh clone. Turn this on to re-run it before
        every task, so dependencies are reinstalled after a stacked-dependency branch is merged in
        (which can add new dev dependencies the persistent clone is missing).
      </p>
    </div>

    <div class="flex items-center gap-3">
      <Button disabled={saving} onclick={submit}>
        {saving ? 'Saving…' : repo ? 'Update repository' : 'Add repository'}
      </Button>
      <a href="/repos" class={buttonVariants({ variant: 'outline' })}>Cancel</a>
    </div>
  </Card.Content>
</Card.Root>
