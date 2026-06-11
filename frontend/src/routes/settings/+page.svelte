<script lang="ts">
  import type {
    AvailabilityWindow,
    EnvVar,
    JiraBoard,
    JiraDeployment,
    NetworkAccessLevel,
    Repository,
    ReviewPolicy,
    Settings,
    TaskColumn
  } from '$lib/types'
  import type { EnvVarWrite } from '$lib/api'

  import { onMount } from 'svelte'

  import { COLUMNS, KNOWN_MODELS } from '$lib/types'
  import { WEEKDAYS, minutesToTime, timeToMinutes } from '$lib/schedule'
  import { usFederalHolidays } from '$lib/holidays'
  import {
    deleteJiraBoard,
    discoverJiraBoards,
    exportConfig,
    getSettings,
    importConfig,
    listEnvVars,
    listJiraBoards,
    listRepos,
    recreateWorkspace,
    resetStats,
    restartWorkspace,
    setEnvVars,
    setTokens,
    testJira,
    updateJiraBoard,
    updateSettings
  } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as Select from '$lib/components/ui/select'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Badge } from '$lib/components/ui/badge'
  import { Switch } from '$lib/components/ui/switch'

  const CUSTOM_MODEL = '__custom__'

  // One editable row per weekday for the working-hours grid. The data model
  // allows several windows per day, but a single contiguous shift covers the
  // common "9 to 5" case and keeps the UI legible.
  type DayRow = { active: boolean; start: string; end: string }

  // One editable row in the environment-variables table. For a secret loaded from
  // the server, `value` starts blank and `preview` holds the masked stored value;
  // leaving it blank on save keeps the stored secret unchanged.
  type EnvRow = {
    key: string
    value: string
    is_secret: boolean
    preview: string | null
  }

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

  // Availability schedule editing state, kept separate from `settings` because
  // the per-weekday grid and skip-date chips are derived shapes.
  let days = $state<DayRow[]>([])
  let skipDates = $state<string[]>([])
  let newSkipDate = $state('')
  let scheduleSavedAt = $state<string | null>(null)
  // The browser's full IANA zone list, when the engine exposes it.
  const timezones =
    typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : []

  // Environment variables (DigitalOcean-style rows).
  let envRows = $state<EnvRow[]>([])
  let envMessage = $state<string | null>(null)

  const policies: ReviewPolicy[] = ['auto_squash_merge', 'human_review', 'none']

  // Network access policy. Domains are edited as free text (one per line or
  // whitespace-separated) and split on save, per the issue's spec.
  let networkLevel = $state<NetworkAccessLevel>('full')
  let networkDomains = $state('')
  let networkIncludeDefaults = $state(true)
  let networkSavedAt = $state<string | null>(null)
  let usageSavedAt = $state<string | null>(null)
  let thoughtsSavedAt = $state<string | null>(null)

  // Order and copy mirror the claude.ai network-access selector.
  const NETWORK_LEVELS: { value: NetworkAccessLevel; title: string; description: string }[] = [
    { value: 'none', title: 'None', description: 'Blocks internet access for maximum security.' },
    {
      value: 'trusted',
      title: 'Trusted',
      description: 'Downloads packages from verified sources.'
    },
    {
      value: 'full',
      title: 'Full',
      description: 'Unrestricted internet access for maximum flexibility.'
    },
    { value: 'custom', title: 'Custom', description: 'Create a list of allowed domains.' }
  ]

  const networkLabel = $derived(
    NETWORK_LEVELS.find((level) => level.value === networkLevel)?.title ?? networkLevel
  )

  const modelLabel = $derived(
    modelChoice === CUSTOM_MODEL
      ? 'Custom…'
      : (KNOWN_MODELS.find((model) => model.value === modelChoice)?.label ?? modelChoice)
  )

  // Upcoming US federal holidays (this year and next) not already skipped.
  const holidaySuggestions = $derived.by(() => {
    const today = new Date().toISOString().slice(0, 10)
    const year = new Date().getFullYear()
    return [...usFederalHolidays(year), ...usFederalHolidays(year + 1)]
      .filter((holiday) => holiday.date >= today && !skipDates.includes(holiday.date))
      .slice(0, 8)
  })

  function buildDays(windows: AvailabilityWindow[]): DayRow[] {
    return WEEKDAYS.map((_, weekday) => {
      const window = windows.find((existing) => existing.weekday === weekday)
      if (!window) {
        return { active: false, start: '09:00', end: '17:00' }
      }
      return {
        active: true,
        start: minutesToTime(window.start_minute),
        end: minutesToTime(window.end_minute)
      }
    })
  }

  function toEnvRow(variable: EnvVar): EnvRow {
    return {
      key: variable.key,
      // Secrets arrive masked; show that as a placeholder and keep the input
      // blank so an unedited secret is preserved on save.
      value: variable.is_secret ? '' : variable.value,
      is_secret: variable.is_secret,
      preview: variable.is_secret ? variable.value : null
    }
  }

  async function load() {
    const loaded = await getSettings()
    settings = loaded
    modelChoice = KNOWN_MODELS.some((model) => model.value === loaded.claude_model)
      ? loaded.claude_model
      : CUSTOM_MODEL
    days = buildDays(loaded.availability_windows)
    skipDates = [...loaded.availability_skip_dates]
    networkLevel = loaded.network_access_level
    networkDomains = loaded.network_access_domains.join('\n')
    networkIncludeDefaults = loaded.network_access_include_defaults
    jiraDeployment = loaded.jira_deployment
    jiraBaseUrl = loaded.jira_base_url
    jiraEmail = loaded.jira_email
    const env = await listEnvVars()
    envRows = env.variables.map(toEnvRow)
    repos = await listRepos()
    await refreshJiraBoards()
  }

  function addSkipDate(date: string) {
    const trimmed = date.trim()
    if (!trimmed || skipDates.includes(trimmed)) {
      return
    }
    skipDates = [...skipDates, trimmed].sort()
    newSkipDate = ''
  }

  function removeSkipDate(date: string) {
    skipDates = skipDates.filter((existing) => existing !== date)
  }

  async function saveSchedule() {
    if (!settings) {
      return
    }
    const windows: AvailabilityWindow[] = days
      .map((day, weekday) => ({ day, weekday }))
      .filter(({ day }) => day.active)
      .map(({ day, weekday }) => ({
        weekday,
        start_minute: timeToMinutes(day.start),
        end_minute: timeToMinutes(day.end)
      }))
    settings = await updateSettings({
      availability_enabled: settings.availability_enabled,
      availability_timezone: settings.availability_timezone,
      availability_windows: windows,
      availability_skip_dates: skipDates
    })
    scheduleSavedAt = new Date().toLocaleTimeString()
  }

  async function saveNetwork() {
    if (!settings) {
      return
    }
    const domains = networkDomains
      .split(/\s+/)
      .map((domain) => domain.trim())
      .filter(Boolean)
    settings = await updateSettings({
      network_access_level: networkLevel,
      network_access_domains: domains,
      network_access_include_defaults: networkIncludeDefaults
    })
    networkDomains = settings.network_access_domains.join('\n')
    networkSavedAt = new Date().toLocaleTimeString()
  }

  // --- Jira ------------------------------------------------------------------
  let jiraDeployment = $state<JiraDeployment>('cloud')
  let jiraBaseUrl = $state('')
  let jiraEmail = $state('')
  let jiraTokenInput = $state('')
  let jiraSavedAt = $state<string | null>(null)
  let jiraTestMessage = $state<string | null>(null)
  let jiraBusy = $state(false)
  let repos = $state<Repository[]>([])
  let jiraBoards = $state<JiraBoard[]>([])

  // Per-board editable state, keyed by board id: the sync flag, the status->column
  // rows being edited, and the chosen repo ids. Kept separate from the loaded
  // boards so edits aren't lost until saved.
  type StatusRow = { status: string; column: TaskColumn }
  type BoardEdit = { sync_enabled: boolean; rows: StatusRow[]; repoIds: string[] }
  let boardEdits = $state<Record<string, BoardEdit>>({})

  const JIRA_DEPLOYMENTS: { value: JiraDeployment; label: string }[] = [
    { value: 'cloud', label: 'Jira Cloud (email + API token)' },
    { value: 'server', label: 'Jira Server / Data Center (PAT)' }
  ]
  const jiraDeploymentLabel = $derived(
    JIRA_DEPLOYMENTS.find((option) => option.value === jiraDeployment)?.label ?? jiraDeployment
  )

  function rebuildBoardEdits() {
    boardEdits = Object.fromEntries(
      jiraBoards.map((board) => [
        board.id,
        {
          sync_enabled: board.sync_enabled,
          rows: Object.entries(board.status_map).map(([status, column]) => ({ status, column })),
          repoIds: [...board.repo_ids]
        }
      ])
    )
  }

  async function refreshJiraBoards() {
    jiraBoards = await listJiraBoards()
    rebuildBoardEdits()
  }

  async function saveJiraConnection() {
    settings = await updateSettings({
      jira_enabled: settings?.jira_enabled ?? false,
      jira_deployment: jiraDeployment,
      jira_base_url: jiraBaseUrl.trim(),
      jira_email: jiraEmail.trim()
    })
    if (jiraTokenInput.trim()) {
      settings = await setTokens({ jira_api_token: jiraTokenInput.trim() })
      jiraTokenInput = ''
    }
    jiraSavedAt = new Date().toLocaleTimeString()
  }

  async function runJiraTest() {
    jiraBusy = true
    jiraTestMessage = null
    try {
      const result = await testJira()
      jiraTestMessage = result.ok
        ? `Connected as ${result.user ?? 'Jira user'}`
        : (result.error ?? 'Connection failed')
    } finally {
      jiraBusy = false
    }
  }

  async function runJiraDiscover() {
    jiraBusy = true
    try {
      jiraBoards = await discoverJiraBoards()
      rebuildBoardEdits()
    } finally {
      jiraBusy = false
    }
  }

  function addStatusRow(boardId: string) {
    boardEdits[boardId]?.rows.push({ status: '', column: 'available' })
  }

  function removeStatusRow(boardId: string, index: number) {
    boardEdits[boardId]?.rows.splice(index, 1)
  }

  function toggleBoardRepo(boardId: string, repoId: string) {
    const edit = boardEdits[boardId]
    if (!edit) {
      return
    }
    edit.repoIds = edit.repoIds.includes(repoId)
      ? edit.repoIds.filter((id) => id !== repoId)
      : [...edit.repoIds, repoId]
  }

  async function saveBoard(boardId: string) {
    const edit = boardEdits[boardId]
    if (!edit) {
      return
    }
    const status_map: Record<string, TaskColumn> = {}
    for (const row of edit.rows) {
      const status = row.status.trim()
      if (status) {
        status_map[status] = row.column
      }
    }
    await updateJiraBoard(boardId, {
      sync_enabled: edit.sync_enabled,
      status_map,
      repo_ids: edit.repoIds
    })
    await refreshJiraBoards()
  }

  async function removeBoard(boardId: string) {
    await deleteJiraBoard(boardId)
    await refreshJiraBoards()
  }

  async function saveUsage() {
    if (!settings) {
      return
    }
    settings = await updateSettings({
      usage_limit_pause_enabled: settings.usage_limit_pause_enabled,
      usage_limit_threshold: settings.usage_limit_threshold
    })
    usageSavedAt = new Date().toLocaleTimeString()
  }

  async function saveThoughts() {
    if (!settings) {
      return
    }
    settings = await updateSettings({ post_thoughts_enabled: settings.post_thoughts_enabled })
    thoughtsSavedAt = new Date().toLocaleTimeString()
  }

  function addEnvRow() {
    envRows = [...envRows, { key: '', value: '', is_secret: false, preview: null }]
  }

  function removeEnvRow(index: number) {
    envRows = envRows.filter((_, position) => position !== index)
  }

  async function saveEnv() {
    const variables: EnvVarWrite[] = envRows
      .filter((row) => row.key.trim())
      .map((row) => {
        const key = row.key.trim()
        if (!row.is_secret) {
          return { key, value: row.value, is_secret: false }
        }
        // A blank secret input keeps the stored value, so omit it; otherwise the
        // typed value becomes the new secret.
        if (row.value) {
          return { key, value: row.value, is_secret: true }
        }
        return { key, is_secret: true }
      })
    const saved = await setEnvVars(variables)
    envRows = saved.variables.map(toEnvRow)
    envMessage = 'Saved.'
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

  let statsMessage = $state<string | null>(null)
  async function runResetStats() {
    if (!confirm('Reset global statistics? Cost, tokens, and time totals start from zero.')) {
      return
    }
    statsMessage = 'Resetting…'
    await resetStats()
    statsMessage = 'Global statistics reset.'
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
          <Label for="branch-template">Default branch template</Label>
          <Input id="branch-template" bind:value={settings.default_branch_template} />
          <p class="text-xs leading-relaxed text-muted-foreground">
            The branch name the agent creates for each task. Supports
            <code class="rounded bg-secondary px-1 py-0.5 text-xs">{'{number}'}</code> (the issue
            number) and <code class="rounded bg-secondary px-1 py-0.5 text-xs">{'{slug}'}</code> (a
            slug of the title). Individual repositories can override this on the
            <a href="/repos" class="underline">Repositories</a> page.
          </p>
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
        <Card.Title>Availability schedule</Card.Title>
        <Card.Description>
          Optional. When on, the agent only picks up new work during the hours and days you set
          here, in your time zone, and never on a skipped date. The database always stores UTC; this
          is just your local view. A task already in progress always runs to completion.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="flex items-center gap-2">
          <Switch id="availability-enabled" bind:checked={settings.availability_enabled} />
          <Label for="availability-enabled">Restrict the agent to a schedule</Label>
        </div>

        {#if settings.availability_enabled}
          <div class="space-y-1.5">
            <Label for="timezone">Time zone</Label>
            <select
              id="timezone"
              bind:value={settings.availability_timezone}
              class="h-9 w-full rounded-md border border-input bg-transparent px-3 text-sm shadow-xs focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
            >
              {#if !timezones.includes(settings.availability_timezone)}
                <option value={settings.availability_timezone}>{settings.availability_timezone}</option>
              {/if}
              {#each timezones as timezone}
                <option value={timezone}>{timezone}</option>
              {/each}
            </select>
          </div>

          <div class="space-y-2">
            <Label>Working hours</Label>
            <p class="text-xs text-muted-foreground">
              Leave every day unchecked to allow any time of day (handy when you only want to skip
              specific dates).
            </p>
            <div class="space-y-2">
              {#each days as day, weekday}
                <div class="flex items-center gap-3 {day.active ? '' : 'opacity-60'}">
                  <label class="flex w-28 items-center gap-2">
                    <Switch bind:checked={day.active} />
                    <span class="text-sm">{WEEKDAYS[weekday]}</span>
                  </label>
                  <Input type="time" class="w-32" bind:value={day.start} disabled={!day.active} />
                  <span class="text-sm text-muted-foreground">to</span>
                  <Input type="time" class="w-32" bind:value={day.end} disabled={!day.active} />
                </div>
              {/each}
            </div>
          </div>

          <div class="space-y-2">
            <Label>Skip dates</Label>
            <p class="text-xs text-muted-foreground">
              Vacations, holidays, any single day the agent should stay idle.
            </p>
            <div class="flex items-center gap-2">
              <Input type="date" class="w-44" bind:value={newSkipDate} />
              <Button variant="outline" size="sm" onclick={() => addSkipDate(newSkipDate)}>Add date</Button>
            </div>
            {#if skipDates.length}
              <div class="flex flex-wrap gap-2">
                {#each skipDates as date}
                  <Button variant="outline" size="sm" class="h-7" title="Remove" onclick={() => removeSkipDate(date)}>
                    {date} ✕
                  </Button>
                {/each}
              </div>
            {:else}
              <p class="text-sm text-muted-foreground">No skipped dates.</p>
            {/if}

            {#if holidaySuggestions.length}
              <p class="text-xs text-muted-foreground">Suggested US holidays (click to add):</p>
              <div class="flex flex-wrap gap-2">
                {#each holidaySuggestions as holiday}
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-7 border border-dashed border-border text-muted-foreground"
                    onclick={() => addSkipDate(holiday.date)}
                  >
                    + {holiday.name} ({holiday.date})
                  </Button>
                {/each}
              </div>
            {/if}
          </div>

          <div class="flex items-center gap-3">
            <Button onclick={saveSchedule}>Save schedule</Button>
            {#if scheduleSavedAt}<span class="text-sm text-muted-foreground">Saved at {scheduleSavedAt}</span>{/if}
          </div>
        {/if}
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Network access</Card.Title>
        <Card.Description>
          Controls outbound connectivity for the agent's workspace. The choice is written into the
          agent's <code class="rounded bg-secondary px-1 py-0.5 text-xs">~/.claude/settings.json</code>
          permissions on the next Recreate, so save here, then Recreate the workspace to apply.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="space-y-1.5">
          <Label for="network-level">Access level</Label>
          <Select.Root
            type="single"
            value={networkLevel}
            onValueChange={(value) => (networkLevel = value as NetworkAccessLevel)}
          >
            <Select.Trigger id="network-level" class="w-full">{networkLabel}</Select.Trigger>
            <Select.Content>
              {#each NETWORK_LEVELS as level}
                <Select.Item value={level.value} label={level.title}>
                  <span class="flex flex-col gap-0.5 py-0.5">
                    <span>{level.title}</span>
                    <span class="text-xs text-muted-foreground">{level.description}</span>
                  </span>
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>

        {#if networkLevel === 'custom'}
          <div class="space-y-1.5">
            <Label for="network-domains">Allowed domains</Label>
            <Textarea
              id="network-domains"
              rows={5}
              class="font-mono"
              placeholder={'api.example.com\n*.internal.example.com\nregistry.example.com'}
              bind:value={networkDomains}
            />
            <p class="text-xs leading-relaxed text-muted-foreground">
              One domain per line, or separated by spaces. Use
              <code class="rounded bg-secondary px-1 py-0.5 text-xs">*.</code> for wildcard
              subdomain matching.
            </p>
            <label class="flex items-center gap-2">
              <Switch bind:checked={networkIncludeDefaults} />
              <span class="text-sm">Also include the default list of common package managers</span>
            </label>
          </div>
        {/if}

        <div class="flex items-center gap-3">
          <Button onclick={saveNetwork}>Save network access</Button>
          {#if networkSavedAt}<span class="text-sm text-muted-foreground">Saved at {networkSavedAt}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Usage limits</Card.Title>
        <Card.Description>
          When the agent's subscription usage approaches its limit, pause new work until the limit
          window resets, then resume automatically. A task already in progress always finishes first.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="flex items-center gap-2">
          <Switch id="usage-enabled" bind:checked={settings.usage_limit_pause_enabled} />
          <Label for="usage-enabled">Auto-pause near the usage limit</Label>
        </div>

        {#if settings.usage_limit_pause_enabled}
          <div class="space-y-1.5">
            <Label for="usage-threshold">Pause at utilization</Label>
            <div class="flex items-center gap-2">
              <Input
                id="usage-threshold"
                type="number"
                min="1"
                max="100"
                class="w-24"
                bind:value={settings.usage_limit_threshold}
              />
              <span class="text-sm text-muted-foreground">% of the current window</span>
            </div>
            <p class="text-xs leading-relaxed text-muted-foreground">
              Claude reports utilization once a window crosses its early-warning threshold (around
              80%), so the default pauses as soon as that warning fires. A reached (100%) limit
              always pauses regardless.
            </p>
          </div>

          {#if settings.usage_paused_until && new Date(settings.usage_paused_until).getTime() > Date.now()}
            <div class="rounded-md border border-warning/40 bg-card p-3 text-sm">
              Paused for usage until
              <strong>{new Date(settings.usage_paused_until).toLocaleString()}</strong>. The agent
              resumes automatically when the window resets.
            </div>
          {/if}
        {/if}

        <div class="flex items-center gap-3">
          <Button onclick={saveUsage}>Save usage limits</Button>
          {#if usageSavedAt}<span class="text-sm text-muted-foreground">Saved at {usageSavedAt}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Issue updates</Card.Title>
        <Card.Description>
          When on, after each work turn the agent's reasoning is condensed by a separate Claude
          call and posted as a single comment on the source GitHub issue, so you can follow along
          on the ticket. Off by default.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="flex items-center gap-2">
          <Switch id="post-thoughts" bind:checked={settings.post_thoughts_enabled} />
          <Label for="post-thoughts">Post a reasoning summary to the issue after each turn</Label>
        </div>
        <div class="flex items-center gap-3">
          <Button onclick={saveThoughts}>Save</Button>
          {#if thoughtsSavedAt}<span class="text-sm text-muted-foreground">Saved at {thoughtsSavedAt}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Jira</Card.Title>
        <Card.Description>
          Connect a Jira site to pull tickets onto the board alongside GitHub issues. The token is
          stored in the database, never in .env. Moving a Jira card between columns transitions the
          ticket's status per the mapping below. The agent does not auto-code Jira tickets yet.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-5">
        <div class="flex items-center gap-2">
          <Switch id="jira-enabled" bind:checked={settings.jira_enabled} />
          <Label for="jira-enabled">Enable Jira integration</Label>
        </div>

        <div class="grid gap-2">
          <Label for="jira-deployment">Deployment</Label>
          <Select.Root
            type="single"
            value={jiraDeployment}
            onValueChange={(value) => (jiraDeployment = value as JiraDeployment)}
          >
            <Select.Trigger id="jira-deployment" class="w-full">{jiraDeploymentLabel}</Select.Trigger>
            <Select.Content>
              {#each JIRA_DEPLOYMENTS as option}
                <Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>

        <div class="grid gap-2">
          <Label for="jira-url">Site URL</Label>
          <Input id="jira-url" placeholder="https://your-org.atlassian.net" bind:value={jiraBaseUrl} />
        </div>

        {#if jiraDeployment === 'cloud'}
          <div class="grid gap-2">
            <Label for="jira-email">Account email</Label>
            <Input id="jira-email" placeholder="you@example.com" bind:value={jiraEmail} />
          </div>
        {/if}

        <div class="grid gap-2">
          <Label for="jira-token">
            {jiraDeployment === 'cloud' ? 'API token' : 'Personal access token'}
          </Label>
          <Input
            id="jira-token"
            type="password"
            placeholder={settings.jira_token_preview ?? 'Paste a token'}
            bind:value={jiraTokenInput}
          />
          {#if settings.jira_token_set}
            <span class="text-xs text-muted-foreground">A token is stored. Leave blank to keep it.</span>
          {/if}
        </div>

        <div class="flex flex-wrap items-center gap-3">
          <Button onclick={saveJiraConnection}>Save</Button>
          <Button variant="outline" disabled={jiraBusy} onclick={runJiraTest}>Test connection</Button>
          {#if jiraSavedAt}<span class="text-sm text-muted-foreground">Saved at {jiraSavedAt}</span>{/if}
          {#if jiraTestMessage}<span class="text-sm text-muted-foreground">{jiraTestMessage}</span>{/if}
        </div>

        <hr class="border-border" />

        <div class="flex items-center justify-between">
          <h3 class="text-sm font-semibold">Followed boards</h3>
          <Button variant="outline" size="sm" disabled={jiraBusy} onclick={runJiraDiscover}>
            Discover boards
          </Button>
        </div>

        {#if jiraBoards.length === 0}
          <p class="text-sm text-muted-foreground">
            No boards yet. Save your connection, then discover boards.
          </p>
        {/if}

        {#each jiraBoards as board (board.id)}
          {@const edit = boardEdits[board.id]}
          {#if edit}
            <div class="space-y-3 rounded-lg border border-border p-3">
              <div class="flex items-center justify-between gap-2">
                <div class="min-w-0">
                  <div class="font-medium">{board.name}</div>
                  {#if board.project_key}
                    <div class="text-xs text-muted-foreground">Project {board.project_key}</div>
                  {/if}
                </div>
                <div class="flex items-center gap-2">
                  <Switch id={`board-sync-${board.id}`} bind:checked={edit.sync_enabled} />
                  <Label for={`board-sync-${board.id}`} class="text-xs">Sync</Label>
                </div>
              </div>

              <div class="space-y-2">
                <div class="text-xs font-semibold text-muted-foreground">Map Jira status to column</div>
                {#each edit.rows as row, index}
                  <div class="flex items-center gap-2">
                    <Input placeholder="Jira status (e.g. In Progress)" bind:value={row.status} class="flex-1" />
                    <span class="text-muted-foreground">→</span>
                    <Select.Root
                      type="single"
                      value={row.column}
                      onValueChange={(value) => (row.column = value as TaskColumn)}
                    >
                      <Select.Trigger class="w-40">
                        {COLUMNS.find((column) => column.key === row.column)?.label ?? row.column}
                      </Select.Trigger>
                      <Select.Content>
                        {#each COLUMNS as column}
                          <Select.Item value={column.key} label={column.label}>{column.label}</Select.Item>
                        {/each}
                      </Select.Content>
                    </Select.Root>
                    <Button variant="ghost" size="sm" onclick={() => removeStatusRow(board.id, index)}>
                      Remove
                    </Button>
                  </div>
                {/each}
                <Button variant="outline" size="sm" onclick={() => addStatusRow(board.id)}>
                  Add mapping
                </Button>
              </div>

              <div class="space-y-1">
                <div class="text-xs font-semibold text-muted-foreground">
                  Repositories a ticket targets
                </div>
                {#if repos.length === 0}
                  <p class="text-xs text-muted-foreground">No repositories configured yet.</p>
                {/if}
                <div class="flex flex-wrap gap-3">
                  {#each repos as repo (repo.id)}
                    <label class="flex items-center gap-1.5 text-sm">
                      <input
                        type="checkbox"
                        checked={edit.repoIds.includes(repo.id)}
                        onchange={() => toggleBoardRepo(board.id, repo.id)}
                      />
                      {repo.full_name}
                    </label>
                  {/each}
                </div>
              </div>

              <div class="flex items-center gap-2">
                <Button size="sm" onclick={() => saveBoard(board.id)}>Save board</Button>
                <Button variant="ghost" size="sm" onclick={() => removeBoard(board.id)}>
                  Stop following
                </Button>
              </div>
            </div>
          {/if}
        {/each}
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Environment variables</Card.Title>
        <Card.Description>
          Injected into the agent's environment at runtime (alongside its tokens) and available to
          setup scripts. Mark a row <strong>secret</strong> to have its value scrubbed from the
          agent's output before it reaches the logs or database, and only ever shown here masked.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        {#if envRows.length}
          <div class="space-y-2">
            {#each envRows as row, index}
              <div class="flex items-center gap-2">
                <Input class="w-1/3 font-mono" placeholder="KEY" bind:value={row.key} />
                <Input
                  class="flex-1 font-mono"
                  type={row.is_secret ? 'password' : 'text'}
                  placeholder={row.is_secret && row.preview ? `${row.preview} (leave blank to keep)` : 'value'}
                  autocomplete="off"
                  bind:value={row.value}
                />
                <label class="flex items-center gap-1.5 text-sm text-muted-foreground" title="Scrub this value from all output">
                  <Switch bind:checked={row.is_secret} />
                  secret
                </label>
                <Button variant="ghost" size="icon" title="Remove" onclick={() => removeEnvRow(index)}>✕</Button>
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-sm text-muted-foreground">No environment variables yet.</p>
        {/if}

        <div class="flex items-center gap-3">
          <Button variant="outline" onclick={addEnvRow}>+ Add another</Button>
          <Button onclick={saveEnv}>Save variables</Button>
          {#if envMessage}<span class="text-sm text-muted-foreground">{envMessage}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Statistics</Card.Title>
        <Card.Description>
          The live stats on the board and task pages count cost, tokens, and time. Reset clears the
          global totals (non-destructive: it just starts counting from now, the history is kept).
          A hard task reset (re-queuing a card) resets that task's own time.
        </Card.Description>
      </Card.Header>
      <Card.Content class="flex items-center gap-3">
        <Button variant="outline" onclick={runResetStats}>Reset global statistics</Button>
        {#if statsMessage}<span class="text-sm text-muted-foreground">{statsMessage}</span>{/if}
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
