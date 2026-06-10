<script lang="ts">
  import type { ReviewPolicy, Settings } from '$lib/types'

  import { onMount } from 'svelte'

  import { KNOWN_MODELS } from '$lib/types'
  import {
    exportConfig,
    getSettings,
    importConfig,
    recreateWorkspace,
    restartWorkspace,
    setTokens,
    updateSettings
  } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as Select from '$lib/components/ui/select'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Badge } from '$lib/components/ui/badge'

  const CUSTOM_MODEL = '__custom__'

  let settings = $state<Settings | null>(null)
  let savedAt = $state<string | null>(null)
  let workspaceMessage = $state<string | null>(null)
  let importMessage = $state<string | null>(null)

  // Write-only secret inputs; never populated from the server.
  let claudeTokenInput = $state('')
  let githubTokenInput = $state('')
  let tokensMessage = $state<string | null>(null)

  // Model picker: a dropdown of known ids plus a custom free-text fallback.
  let modelChoice = $state<string>(KNOWN_MODELS[0].value)

  const policies: ReviewPolicy[] = ['auto_squash_merge', 'human_review', 'none']

  const modelLabel = $derived(
    modelChoice === CUSTOM_MODEL
      ? 'Custom…'
      : (KNOWN_MODELS.find((model) => model.value === modelChoice)?.label ?? modelChoice)
  )

  async function load() {
    const loaded = await getSettings()
    settings = loaded
    modelChoice = KNOWN_MODELS.some((model) => model.value === loaded.claude_model)
      ? loaded.claude_model
      : CUSTOM_MODEL
  }

  function chooseModel(value: string) {
    modelChoice = value
    if (settings && value !== CUSTOM_MODEL) {
      settings.claude_model = value
    }
  }

  async function save() {
    if (!settings) {
      return
    }
    settings = await updateSettings({
      org_name: settings.org_name,
      global_instructions: settings.global_instructions,
      default_review_policy: settings.default_review_policy,
      claude_model: settings.claude_model,
      base_setup_script: settings.base_setup_script,
      config_repo_url: settings.config_repo_url,
      default_branch_template: settings.default_branch_template
    })
    savedAt = new Date().toLocaleTimeString()
  }

  async function saveTokens() {
    if (!claudeTokenInput.trim() && !githubTokenInput.trim()) {
      return
    }
    settings = await setTokens({
      claude_oauth_token: claudeTokenInput.trim() || undefined,
      github_token: githubTokenInput.trim() || undefined
    })
    claudeTokenInput = ''
    githubTokenInput = ''
    tokensMessage = 'Saved to the database.'
  }

  async function runRestart() {
    workspaceMessage = 'Restarting…'
    await restartWorkspace()
    workspaceMessage = 'Workspace restarted.'
  }

  async function runRecreate() {
    workspaceMessage = 'Recreating + provisioning…'
    await recreateWorkspace()
    workspaceMessage = 'Workspace recreated; repos + config reprovisioned.'
  }

  async function downloadExport() {
    const bundle = await exportConfig()
    const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = 'seraphim-config.json'
    anchor.click()
    URL.revokeObjectURL(url)
  }

  async function onImportFile(event: Event) {
    const input = event.target as HTMLInputElement
    const file = input.files?.[0]
    if (!file) {
      return
    }
    importMessage = 'Importing…'
    try {
      const bundle = JSON.parse(await file.text())
      await importConfig(bundle)
      importMessage = 'Imported.'
      await load()
    } catch (error) {
      importMessage = `Import failed: ${error instanceof Error ? error.message : 'invalid file'}`
    }
    input.value = ''
  }

  onMount(load)
</script>

<div class="mx-auto max-w-3xl space-y-5 px-6 py-6">
  <h1 class="text-2xl font-semibold">Settings</h1>

  {#if settings}
    <Card.Root>
      <Card.Header>
        <Card.Title>Environment profile</Card.Title>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="space-y-1.5">
          <Label for="org">Organization name</Label>
          <Input id="org" bind:value={settings.org_name} />
        </div>

        <div class="space-y-1.5">
          <Label for="model">Claude model</Label>
          <Select.Root type="single" value={modelChoice} onValueChange={chooseModel}>
            <Select.Trigger id="model" class="w-full">{modelLabel}</Select.Trigger>
            <Select.Content>
              {#each KNOWN_MODELS as model}
                <Select.Item value={model.value} label={model.label}>{model.label}</Select.Item>
              {/each}
              <Select.Item value={CUSTOM_MODEL} label="Custom…">Custom…</Select.Item>
            </Select.Content>
          </Select.Root>
          {#if modelChoice === CUSTOM_MODEL}
            <Input placeholder="exact model id, e.g. claude-opus-4-8[1m]" bind:value={settings.claude_model} />
          {/if}
          <p class="text-xs leading-relaxed text-muted-foreground">
            Friendly names shown here; the coded model id is what's sent to the agent. Fable 5, Opus
            4.x, and Sonnet 4.6 are 1M-context; Haiku 4.5 is 200K.
          </p>
        </div>

        <div class="space-y-1.5">
          <Label for="policy">Default review policy</Label>
          <Select.Root
            type="single"
            value={settings.default_review_policy}
            onValueChange={(value) => settings && (settings.default_review_policy = value as ReviewPolicy)}
          >
            <Select.Trigger id="policy" class="w-full">
              {settings.default_review_policy.replace(/_/g, ' ')}
            </Select.Trigger>
            <Select.Content>
              {#each policies as policy}
                <Select.Item value={policy} label={policy.replace(/_/g, ' ')}>
                  {policy.replace(/_/g, ' ')}
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>

        <div class="space-y-1.5">
          <Label for="global">Global agent instructions</Label>
          <Textarea id="global" rows={5} bind:value={settings.global_instructions} />
          <p class="text-xs leading-relaxed text-muted-foreground">
            Written to <code class="rounded bg-secondary px-1 py-0.5 text-xs">/workspace/AGENTS.md</code>,
            which the agent reads automatically at the start of every session. Put org-wide
            conventions here (how to branch, when to open vs. auto-merge PRs, coding standards).
          </p>
        </div>

        <div class="space-y-1.5">
          <Label for="setup">Environment setup script</Label>
          <Textarea id="setup" rows={4} bind:value={settings.base_setup_script} />
          <p class="text-xs leading-relaxed text-muted-foreground">
            Runs once when the workspace container is built or recreated, as the non-root
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">node</code> user (passwordless
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">sudo</code> is available). The image is
            Debian 12 (bookworm) with Node 22; <code class="rounded bg-secondary px-1 py-0.5 text-xs">pnpm</code>,
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">yarn</code>, and
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">npm</code> are already installed, so you do
            not need <code class="rounded bg-secondary px-1 py-0.5 text-xs">corepack enable</code>. Per-repo
            commands like <code class="rounded bg-secondary px-1 py-0.5 text-xs">yarn install</code> belong in
            each repository's own setup script.
          </p>
        </div>

        <div class="flex items-center gap-3">
          <Button onclick={save}>Save</Button>
          {#if savedAt}<span class="text-sm text-muted-foreground">Saved at {savedAt}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Secrets</Card.Title>
        <Card.Description>
          Stored in the database, never in <code class="rounded bg-secondary px-1 py-0.5 text-xs">.env</code>
          and never returned by the API. Injected into the agent only at runtime. Leave a field blank
          to keep the existing value.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="space-y-1.5">
          <Label for="claude-token" class="flex items-center gap-2">
            Claude OAuth token
            <Badge variant="outline" class={settings.claude_token_set ? 'border-success/40 text-success' : 'text-muted-foreground'}>
              {settings.claude_token_set ? 'configured' : 'not set'}
            </Badge>
          </Label>
          <Input
            id="claude-token"
            type="password"
            autocomplete="off"
            placeholder="from `claude setup-token`"
            bind:value={claudeTokenInput}
          />
        </div>
        <div class="space-y-1.5">
          <Label for="gh-token" class="flex items-center gap-2">
            GitHub token
            <Badge variant="outline" class={settings.github_token_set ? 'border-success/40 text-success' : 'text-muted-foreground'}>
              {settings.github_token_set ? 'configured' : 'not set'}
            </Badge>
          </Label>
          <Input
            id="gh-token"
            type="password"
            autocomplete="off"
            placeholder="PAT with repo + issues scope"
            bind:value={githubTokenInput}
          />
        </div>
        <div class="flex items-center gap-3">
          <Button onclick={saveTokens}>Save secrets</Button>
          {#if tokensMessage}<span class="text-sm text-muted-foreground">{tokensMessage}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Agent config repo (~/.claude)</Card.Title>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="space-y-1.5">
          <Label for="configrepo">Config repo URL</Label>
          <Input id="configrepo" placeholder="git@github.com:navarrotech/agents.git" bind:value={settings.config_repo_url} />
          <p class="text-xs leading-relaxed text-muted-foreground">
            The workspace clones this into the agent's config dir, so your
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">AGENTS.md</code>, docs, manuals, and skills
            travel with the deployment, no host mount required. Cloned over SSH using your mounted key.
            Save, then Recreate to apply.
          </p>
        </div>
        <Button onclick={save}>Save</Button>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Backup & transfer</Card.Title>
        <Card.Description>
          Export your settings and repositories as JSON to move a setup to another machine. Secrets
          are never included. Import merges into the current config.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <div class="flex items-center gap-3">
          <Button variant="outline" onclick={downloadExport}>Export JSON</Button>
          <label class={buttonVariants({ variant: 'outline' })}>
            Import JSON
            <input type="file" accept="application/json" onchange={onImportFile} hidden />
          </label>
          {#if importMessage}<span class="text-sm text-muted-foreground">{importMessage}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Workspace</Card.Title>
        <Card.Description>
          Restart re-runs the entrypoint; recreate rebuilds the container and reprovisions (config
          repo + all repos + setup scripts). The persistent volume (repos + Claude conversation) is
          preserved either way.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <div class="flex items-center gap-3">
          <Button variant="outline" onclick={runRestart}>Restart</Button>
          <Button variant="outline" onclick={runRecreate}>Recreate</Button>
          {#if workspaceMessage}<span class="text-sm text-muted-foreground">{workspaceMessage}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>
  {:else}
    <p class="text-muted-foreground">Loading…</p>
  {/if}
</div>
