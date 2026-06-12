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
    TailscaleStatus,
    TaskColumn
  } from '$lib/types'
  import type { EnvVarWrite, UpdateStatus } from '$lib/api'

  import { onMount } from 'svelte'

  import { COLUMNS, KNOWN_MODELS } from '$lib/types'
  import { WEEKDAYS, minutesToTime, timeToMinutes } from '$lib/schedule'
  import { usFederalHolidays } from '$lib/holidays'
  import {
    checkForUpdate,
    deleteJiraBoard,
    discoverJiraBoards,
    exportConfig,
    getSettings,
    getUpdateStatus,
    getVersion,
    importConfig,
    listEnvVars,
    listJiraBoards,
    listRepos,
    recreateWorkspace,
    resetAgent,
    resetStats,
    restartWorkspace,
    runUpdate,
    setEnvVars,
    setTokens,
    testJira,
    updateJiraBoard,
    updateSettings,
    uploadSound,
    clearSound,
    soundUrl,
    getTailscaleStatus,
    tailscaleUp,
    tailscaleDown,
    tailscaleReauth,
    tailscaleRestart
  } from '$lib/api'
  import type { SoundKind } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as Select from '$lib/components/ui/select'
  import * as AlertDialog from '$lib/components/ui/alert-dialog'
  import { Button, buttonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Textarea } from '$lib/components/ui/textarea'
  import { Badge } from '$lib/components/ui/badge'
  import { Switch } from '$lib/components/ui/switch'

  const CUSTOM_MODEL = '__custom__'

  // The settings subpages, in left-menu order. Each id matches one card's
  // {#if active === '...'} wrapper; only the selected subpage is rendered.
  const SECTIONS = [
    { id: 'profile', label: 'Profile' },
    { id: 'secrets', label: 'Secrets' },
    { id: 'config', label: 'Config repo' },
    { id: 'jira', label: 'Jira' },
    { id: 'availability', label: 'Availability' },
    { id: 'network', label: 'Network access' },
    { id: 'usage', label: 'Usage limits' },
    { id: 'sounds', label: 'Notification sounds' },
    { id: 'issue-updates', label: 'Issue updates' },
    { id: 'statistics', label: 'Statistics' },
    { id: 'env', label: 'Environment variables' },
    { id: 'workspace', label: 'Workspace' },
    { id: 'tailscale', label: 'Tailscale' },
    { id: 'updates', label: 'Updates' },
    { id: 'backup', label: 'Backup & restore' }
  ] as const

  let active = $state<(typeof SECTIONS)[number]['id']>('profile')

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
  let githubWebhookSecretInput = $state('')
  let jiraWebhookSecretInput = $state('')
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
  let closeIssueSavedAt = $state<string | null>(null)
  let soundsSavedAt = $state<string | null>(null)
  // Cache-buster bumped after an upload/clear so the <audio> preview refetches.
  let soundVersion = $state(0)

  async function saveSounds() {
    if (!settings) {
      return
    }
    settings = await updateSettings({
      attention_sound_enabled: settings.attention_sound_enabled,
      completion_sound_enabled: settings.completion_sound_enabled
    })
    soundsSavedAt = new Date().toLocaleTimeString()
  }

  // Plays the clip an operator would hear for this event (custom if uploaded, else
  // the bundled default), so they can check it from Settings.
  function previewSound(kind: SoundKind) {
    if (!settings || typeof Audio === 'undefined') {
      return
    }
    const custom = kind === 'attention' ? settings.attention_sound_custom : settings.completion_sound_custom
    const audio = new Audio(`${soundUrl(kind, custom)}?v=${soundVersion}`)
    audio.volume = 0.6
    void audio.play().catch((error) => console.debug('preview blocked', error))
  }

  async function onSoundFile(kind: SoundKind, event: Event) {
    const input = event.target as HTMLInputElement
    const file = input.files?.[0]
    input.value = ''
    if (!file) {
      return
    }
    try {
      settings = await uploadSound(kind, file)
      soundVersion += 1
      soundsSavedAt = new Date().toLocaleTimeString()
    } catch (error) {
      console.debug('sound upload failed', error)
      window.alert('Could not upload that file. Use a short audio clip under 1 MB.')
    }
  }

  async function resetSound(kind: SoundKind) {
    settings = await clearSound(kind)
    soundVersion += 1
    soundsSavedAt = new Date().toLocaleTimeString()
  }

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

  async function saveCloseIssue() {
    if (!settings) {
      return
    }
    settings = await updateSettings({ close_issue_on_done: settings.close_issue_on_done })
    closeIssueSavedAt = new Date().toLocaleTimeString()
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
    if (
      !claudeTokenInput.trim() &&
      !githubTokenInput.trim() &&
      !githubWebhookSecretInput.trim() &&
      !jiraWebhookSecretInput.trim()
    ) {
      return
    }
    settings = await setTokens({
      claude_oauth_token: claudeTokenInput.trim() || undefined,
      github_token: githubTokenInput.trim() || undefined,
      github_webhook_secret: githubWebhookSecretInput.trim() || undefined,
      jira_webhook_secret: jiraWebhookSecretInput.trim() || undefined
    })
    claudeTokenInput = ''
    githubTokenInput = ''
    githubWebhookSecretInput = ''
    jiraWebhookSecretInput = ''
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

  // --- Tailscale management ---------------------------------------------------
  let tailscale = $state<TailscaleStatus | null>(null)
  let tailscaleLoading = $state(false)
  let tailscaleBusy = $state(false)
  let tailscaleMessage = $state<string | null>(null)
  let reauthDialogOpen = $state(false)
  let tailscaleRestartDialogOpen = $state(false)

  function errorText(error: unknown): string {
    return error instanceof Error ? error.message : 'unknown error'
  }

  async function loadTailscale() {
    tailscaleLoading = true
    try {
      tailscale = await getTailscaleStatus()
    } catch (error) {
      tailscaleMessage = `Could not read Tailscale status: ${errorText(error)}`
    } finally {
      tailscaleLoading = false
    }
  }

  // Lazy-load the status the first time the operator opens the Tailscale section
  // (it execs into the container, so we don't pay for it on every settings visit).
  $effect(() => {
    if (active === 'tailscale' && !tailscale && !tailscaleLoading) {
      loadTailscale()
    }
  })

  async function runTailscaleAction(
    action: () => Promise<{ ok: boolean; message: string; status: TailscaleStatus }>,
    pending: string
  ) {
    tailscaleBusy = true
    tailscaleMessage = pending
    try {
      const result = await action()
      tailscale = result.status
      tailscaleMessage = result.message
    } catch (error) {
      tailscaleMessage = `Failed: ${errorText(error)}`
    } finally {
      tailscaleBusy = false
    }
  }

  const runTailscaleUp = () => runTailscaleAction(tailscaleUp, 'Connecting…')
  const runTailscaleDown = () => runTailscaleAction(tailscaleDown, 'Disconnecting…')

  function runTailscaleReauth() {
    reauthDialogOpen = false
    return runTailscaleAction(() => tailscaleReauth(true), 'Starting re-authentication…')
  }

  // When the node simply needs login (no force), get the URL without a warning.
  const runTailscaleLogin = () =>
    runTailscaleAction(() => tailscaleReauth(false), 'Requesting a login URL…')

  function runTailscaleRestart() {
    tailscaleRestartDialogOpen = false
    return runTailscaleAction(tailscaleRestart, 'Restarting the container…')
  }

  async function copyToClipboard(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      tailscaleMessage = 'Copied to clipboard.'
    } catch (error) {
      tailscaleMessage = `Could not copy: ${errorText(error)}`
    }
  }

  // Hard reset: purge the agent's history/session (and optionally memories), then
  // it spins up a brand-new, context-free session on its next turn.
  let resetDialogOpen = $state(false)
  let resetMemories = $state(false)
  let resetting = $state(false)
  async function runReset() {
    resetting = true
    try {
      await resetAgent(resetMemories)
      resetDialogOpen = false
      // Everything (history, session, stats) is gone; reload to a clean slate.
      window.location.reload()
    } catch (error) {
      console.error('hard reset failed', error)
      workspaceMessage = 'Hard reset failed; see logs.'
    } finally {
      resetting = false
    }
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

  // --- Self-update -----------------------------------------------------------
  let updateStatus = $state<UpdateStatus | null>(null)
  let checkingUpdate = $state(false)
  let updateMessage = $state<string | null>(null)
  // True from clicking Update until the new build is up and we reload.
  let updateRunning = $state(false)

  async function loadUpdateStatus() {
    try {
      updateStatus = await getUpdateStatus()
    } catch (error) {
      console.debug('failed to load update status', error)
    }
  }

  async function runCheck() {
    checkingUpdate = true
    updateMessage = null
    try {
      updateStatus = await checkForUpdate()
    } catch {
      updateMessage = 'Update check failed.'
    } finally {
      checkingUpdate = false
    }
  }

  async function doUpdate() {
    if (!updateStatus) {
      return
    }
    updateRunning = true
    updateMessage = 'Pausing the agent and rebuilding… this can take a few minutes.'
    let startingSha: string
    try {
      startingSha = (await getVersion()).sha
      await runUpdate()
    } catch {
      updateMessage = 'Failed to start the update. Is the agent idle and HOST_REPO_DIR set?'
      updateRunning = false
      return
    }
    // The rebuild replaces the API; poll /version (tolerating downtime) until the
    // commit changes, then reload onto the new build.
    const deadline = Date.now() + 12 * 60 * 1000
    const poll = setInterval(async () => {
      if (Date.now() > deadline) {
        clearInterval(poll)
        updateMessage = 'Update is taking a while. Refresh the page once it finishes.'
        updateRunning = false
        return
      }
      try {
        const { sha } = await getVersion()
        if (sha && sha !== startingSha) {
          clearInterval(poll)
          window.location.reload()
        }
      } catch {
        // API is down mid-rebuild; keep polling until it returns on the new build.
      }
    }, 4000)
  }

  onMount(() => {
    load()
    loadUpdateStatus()
  })
</script>

<div class="mx-auto flex max-w-5xl gap-6 px-6 py-6">
  <!-- Left sub-menu: each item is a subpage; only the selected one renders. -->
  <nav class="sticky top-6 w-52 flex-none self-start">
    <h1 class="mb-3 text-2xl font-semibold">Settings</h1>
    <ul class="space-y-0.5">
      {#each SECTIONS as section}
        <li>
          <button
            type="button"
            onclick={() => (active = section.id)}
            class="w-full rounded-md px-3 py-1.5 text-left text-sm transition-colors {active ===
            section.id
              ? 'bg-secondary font-medium text-foreground'
              : 'text-muted-foreground hover:bg-secondary hover:text-foreground'}"
          >
            {section.label}
          </button>
        </li>
      {/each}
    </ul>
  </nav>

  <div class="min-w-0 flex-1 space-y-5">
    {#if settings}
    {#if active === 'profile'}
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
    {/if}

    {#if active === 'secrets'}
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

        <!-- Realtime issue webhooks: set the shared secret here, then point the
             provider's webhook at the matching endpoint so new issues appear at
             once instead of waiting for the next poll. -->
        <div class="border-t border-border pt-5">
          <h3 class="text-sm font-semibold text-foreground">Realtime issue webhooks</h3>
          <p class="mt-1 text-sm text-muted-foreground">
            Optional. Set a secret, then add a webhook in the provider pointed at this server so new
            issues show up instantly. Without one, issues still sync on the regular poll.
          </p>
        </div>
        <div class="space-y-1.5">
          <Label for="gh-webhook-secret" class="flex items-center gap-2">
            GitHub webhook secret
            <Badge variant="outline" class={settings.github_webhook_secret_set ? 'border-success/40 text-success' : 'text-muted-foreground'}>
              {settings.github_webhook_secret_set ? 'configured' : 'not set'}
            </Badge>
          </Label>
          <Input
            id="gh-webhook-secret"
            type="password"
            autocomplete="off"
            placeholder="shared secret for the GitHub webhook"
            bind:value={githubWebhookSecretInput}
          />
          <p class="text-xs text-muted-foreground">
            Add a repo (or org) webhook for the <strong>Issues</strong> event with content type
            <code class="rounded bg-secondary px-1 py-0.5">application/json</code>, this same secret, and the
            URL <code class="rounded bg-secondary px-1 py-0.5">&lt;this-server&gt;/api/v1/webhooks/github</code>.
          </p>
        </div>
        <div class="space-y-1.5">
          <Label for="jira-webhook-secret" class="flex items-center gap-2">
            Jira webhook secret
            <Badge variant="outline" class={settings.jira_webhook_secret_set ? 'border-success/40 text-success' : 'text-muted-foreground'}>
              {settings.jira_webhook_secret_set ? 'configured' : 'not set'}
            </Badge>
          </Label>
          <Input
            id="jira-webhook-secret"
            type="password"
            autocomplete="off"
            placeholder="shared secret for the Jira webhook"
            bind:value={jiraWebhookSecretInput}
          />
          <p class="text-xs text-muted-foreground">
            Add a Jira webhook for the issue <strong>created / updated / deleted</strong> events to
            <code class="rounded bg-secondary px-1 py-0.5">&lt;this-server&gt;/api/v1/webhooks/jira</code>.
            Cloud signs with the secret; on Server / Data Center append
            <code class="rounded bg-secondary px-1 py-0.5">?secret=&lt;value&gt;</code> to the URL.
          </p>
        </div>
        <div class="flex items-center gap-3">
          <Button onclick={saveTokens}>Save secrets</Button>
          {#if tokensMessage}<span class="text-sm text-muted-foreground">{tokensMessage}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>
    {/if}

    {#if active === 'config'}
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
    {/if}

    {#if active === 'backup'}
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
    {/if}

    {#if active === 'availability'}
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
    {/if}

    {#if active === 'network'}
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
    {/if}

    {#if active === 'usage'}
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
    {/if}

    {#if active === 'sounds'}
    <Card.Root>
      <Card.Header>
        <Card.Title>Notification sounds</Card.Title>
        <Card.Description>
          Play a sound in this browser when a task needs your attention and when a task finishes, so
          you notice without watching the board. Each event uses a built-in chime by default; upload a
          short custom clip (under 1 MB) to override it. Sounds play only while this app is open in a
          tab, and the browser may stay silent until you have clicked the page once.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-6">
        <!-- Attention: questions + heart attacks. -->
        <div>
          <div class="flex items-center gap-2">
            <Switch id="attention-sound" bind:checked={settings.attention_sound_enabled} />
            <Label for="attention-sound">Play a sound when a task needs your attention</Label>
          </div>
          <p class="mt-1.5 text-sm text-muted-foreground">
            Fires when the agent asks you a question, or when a turn has a heart attack.
          </p>
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" onclick={() => previewSound('attention')}>Preview</Button>
            <label class={buttonVariants({ variant: 'outline', size: 'sm' })}>
              {settings.attention_sound_custom ? 'Replace clip' : 'Upload custom clip'}
              <input type="file" accept="audio/*" onchange={(event) => onSoundFile('attention', event)} hidden />
            </label>
            {#if settings.attention_sound_custom}
              <Button variant="ghost" size="sm" onclick={() => resetSound('attention')}>Reset to default</Button>
              <span class="text-xs text-muted-foreground">Custom clip set</span>
            {:else}
              <span class="text-xs text-muted-foreground">Using the default chime</span>
            {/if}
          </div>
        </div>

        <!-- Completion: a task auto-merged to Done. -->
        <div class="border-t border-border pt-5">
          <div class="flex items-center gap-2">
            <Switch id="completion-sound" bind:checked={settings.completion_sound_enabled} />
            <Label for="completion-sound">Play a sound when a task finishes</Label>
          </div>
          <p class="mt-1.5 text-sm text-muted-foreground">
            Fires when a task auto-merges its pull request and moves to Done.
          </p>
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" onclick={() => previewSound('completion')}>Preview</Button>
            <label class={buttonVariants({ variant: 'outline', size: 'sm' })}>
              {settings.completion_sound_custom ? 'Replace clip' : 'Upload custom clip'}
              <input type="file" accept="audio/*" onchange={(event) => onSoundFile('completion', event)} hidden />
            </label>
            {#if settings.completion_sound_custom}
              <Button variant="ghost" size="sm" onclick={() => resetSound('completion')}>Reset to default</Button>
              <span class="text-xs text-muted-foreground">Custom clip set</span>
            {:else}
              <span class="text-xs text-muted-foreground">Using the default chime</span>
            {/if}
          </div>
        </div>

        <div class="flex items-center gap-3 border-t border-border pt-5">
          <Button onclick={saveSounds}>Save</Button>
          {#if soundsSavedAt}<span class="text-sm text-muted-foreground">Saved at {soundsSavedAt}</span>{/if}
          <span class="text-xs text-muted-foreground">
            (Uploads and resets save immediately; this saves the on/off toggles.)
          </span>
        </div>
      </Card.Content>
    </Card.Root>
    {/if}

    {#if active === 'issue-updates'}
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

        <div class="border-t border-border pt-5">
          <div class="flex items-center gap-2">
            <Switch id="close-issue-on-done" bind:checked={settings.close_issue_on_done} />
            <Label for="close-issue-on-done">Close the linked issue when a task auto-merges to done</Label>
          </div>
          <p class="mt-1.5 text-sm text-muted-foreground">
            On by default. The agent merges into <code class="rounded bg-secondary px-1 py-0.5">develop</code>, so
            GitHub's own keyword-close (which fires only on the default branch) never triggers. Turn this off
            to rely on that instead.
          </p>
          <div class="mt-3 flex items-center gap-3">
            <Button onclick={saveCloseIssue}>Save</Button>
            {#if closeIssueSavedAt}<span class="text-sm text-muted-foreground">Saved at {closeIssueSavedAt}</span>{/if}
          </div>
        </div>
      </Card.Content>
    </Card.Root>
    {/if}

    {#if active === 'jira'}
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
                    <button
                      type="button"
                      role="switch"
                      aria-checked={edit.repoIds.includes(repo.id)}
                      onclick={() => toggleBoardRepo(board.id, repo.id)}
                      class="flex cursor-pointer items-center gap-1.5 text-sm"
                    >
                      <Switch
                        size="sm"
                        checked={edit.repoIds.includes(repo.id)}
                        tabindex={-1}
                        aria-hidden="true"
                        class="pointer-events-none"
                      />
                      {repo.full_name}
                    </button>
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
    {/if}

    {#if active === 'env'}
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
    {/if}

    {#if active === 'statistics'}
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
    {/if}

    {#if active === 'workspace'}
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

    <Card.Root class="mt-6 border-destructive/40">
      <Card.Header>
        <Card.Title class="text-destructive">Hard reset the agent</Card.Title>
        <Card.Description>
          Wipes the agent's conversation history, statistics, and the current Claude session, and
          requeues whatever it was working on. The next turn starts a brand-new, context-free
          session. This cannot be undone.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <AlertDialog.Root bind:open={resetDialogOpen}>
          <AlertDialog.Trigger class={buttonVariants({ variant: 'destructive' })}>
            Hard reset
          </AlertDialog.Trigger>
          <AlertDialog.Content>
            <AlertDialog.Header>
              <AlertDialog.Title>Hard reset the agent?</AlertDialog.Title>
              <AlertDialog.Description>
                This purges the agent's history and current session and requeues its in-progress
                task, then starts a fresh session with no context. This cannot be undone.
              </AlertDialog.Description>
            </AlertDialog.Header>
            <label class="flex items-center gap-3 rounded-md border border-border p-3">
              <Switch id="reset-memories" bind:checked={resetMemories} />
              <span class="text-sm">
                <span class="font-medium">Also purge memories</span>
                <span class="block text-muted-foreground">
                  Delete the agent's accumulated memory files too.
                </span>
              </span>
            </label>
            <AlertDialog.Footer>
              <AlertDialog.Cancel disabled={resetting}>Cancel</AlertDialog.Cancel>
              <Button variant="destructive" disabled={resetting} onclick={runReset}>
                {resetting ? 'Resetting…' : resetMemories ? 'Reset + purge memories' : 'Reset'}
              </Button>
            </AlertDialog.Footer>
          </AlertDialog.Content>
        </AlertDialog.Root>
      </Card.Content>
    </Card.Root>
    {/if}

    {#if active === 'tailscale'}
    <Card.Root>
      <Card.Header>
        <Card.Title>Tailscale</Card.Title>
        <Card.Description>
          Seraphim's UI is exposed over your tailnet by a Tailscale sidecar container. See its URL
          and hosting status here, connect or disconnect it, restart it, or get a login link when it
          needs to be authenticated.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        <div class="flex items-center gap-3">
          <Button variant="outline" disabled={tailscaleLoading} onclick={loadTailscale}>
            {tailscaleLoading ? 'Refreshing…' : 'Refresh'}
          </Button>
          {#if tailscaleMessage}
            <span class="text-sm text-muted-foreground break-words">{tailscaleMessage}</span>
          {/if}
        </div>

        {#if tailscale && !tailscale.container_running}
          <div class="rounded-md border border-warning/40 bg-warning/5 p-3 text-sm">
            The Tailscale container isn't running. Start the stack (<code
              class="rounded bg-secondary px-1 py-0.5">docker compose up -d</code
            >), or try Restart below.
          </div>
        {:else if tailscale}
          <!-- Connection state + the tailnet URL. -->
          <div class="space-y-3 rounded-md border border-border p-4">
            <div class="flex flex-wrap items-center gap-2">
              {#if tailscale.connected}
                <Badge class="bg-success/15 text-success">Connected</Badge>
              {:else if tailscale.needs_login}
                <Badge class="bg-warning/15 text-warning">Needs authentication</Badge>
              {:else}
                <Badge variant="outline" class="text-muted-foreground">Disconnected</Badge>
              {/if}
              {#if tailscale.connected}
                <Badge variant="outline" class={tailscale.online ? 'text-success' : 'text-muted-foreground'}>
                  {tailscale.online ? 'Online' : 'Offline'}
                </Badge>
                <Badge variant="outline" class={tailscale.serve_active ? 'text-success' : 'text-muted-foreground'}>
                  {tailscale.serve_active ? 'Hosting UI' : 'Not hosting'}
                </Badge>
              {/if}
              {#if tailscale.backend_state}
                <span class="text-xs text-muted-foreground">({tailscale.backend_state})</span>
              {/if}
            </div>

            {#if tailscale.url}
              <div>
                <div class="text-xs uppercase tracking-wide text-muted-foreground">Tailnet URL</div>
                <div class="mt-1 flex flex-wrap items-center gap-2">
                  <a
                    href={tailscale.url}
                    target="_blank"
                    rel="noreferrer"
                    class="font-mono text-sm text-primary hover:underline break-all"
                  >
                    {tailscale.url}
                  </a>
                  <Button variant="ghost" size="sm" onclick={() => copyToClipboard(tailscale!.url!)}>
                    Copy
                  </Button>
                </div>
              </div>
            {/if}

            <div class="grid grid-cols-1 gap-1 text-sm sm:grid-cols-2">
              {#if tailscale.tailnet}
                <div><span class="text-muted-foreground">Tailnet:</span> {tailscale.tailnet}</div>
              {/if}
              {#if tailscale.hostname}
                <div><span class="text-muted-foreground">Hostname:</span> {tailscale.hostname}</div>
              {/if}
              {#if tailscale.tailscale_ips.length}
                <div class="sm:col-span-2">
                  <span class="text-muted-foreground">IPs:</span>
                  <span class="font-mono text-xs">{tailscale.tailscale_ips.join(', ')}</span>
                </div>
              {/if}
            </div>
          </div>

          <!-- A pending login URL the operator needs to visit to authenticate. -->
          {#if tailscale.auth_url}
            <div class="space-y-2 rounded-md border border-warning/50 bg-warning/5 p-4">
              <div class="text-sm font-medium">This node needs to be authenticated.</div>
              <div class="flex flex-wrap items-center gap-2">
                <a
                  href={tailscale.auth_url}
                  target="_blank"
                  rel="noreferrer"
                  class={buttonVariants({ variant: 'default', size: 'sm' })}
                >
                  Open login page
                </a>
                <Button variant="ghost" size="sm" onclick={() => copyToClipboard(tailscale!.auth_url!)}>
                  Copy link
                </Button>
              </div>
            </div>
          {/if}
        {/if}

        <!-- Management actions. -->
        <div class="flex flex-wrap items-center gap-2 border-t border-border pt-4">
          {#if tailscale?.connected}
            <Button variant="outline" disabled={tailscaleBusy} onclick={runTailscaleDown}>Disconnect</Button>
          {:else}
            <Button variant="outline" disabled={tailscaleBusy} onclick={runTailscaleUp}>Connect</Button>
          {/if}

          {#if tailscale?.needs_login && !tailscale?.auth_url}
            <Button variant="outline" disabled={tailscaleBusy} onclick={runTailscaleLogin}>
              Get login URL
            </Button>
          {/if}

          <!-- Re-authenticate (force): disconnects until the new login completes. -->
          <AlertDialog.Root bind:open={reauthDialogOpen}>
            <AlertDialog.Trigger class={buttonVariants({ variant: 'outline' })} disabled={tailscaleBusy}>
              Re-authenticate
            </AlertDialog.Trigger>
            <AlertDialog.Content>
              <AlertDialog.Header>
                <AlertDialog.Title>Re-authenticate this node?</AlertDialog.Title>
                <AlertDialog.Description>
                  This starts a fresh login and returns a new link to authenticate the node (for
                  example to move it to a different Tailscale account). The node disconnects until the
                  login is completed, so the tailnet URL is briefly unavailable.
                </AlertDialog.Description>
              </AlertDialog.Header>
              <AlertDialog.Footer>
                <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
                <AlertDialog.Action onclick={runTailscaleReauth}>Re-authenticate</AlertDialog.Action>
              </AlertDialog.Footer>
            </AlertDialog.Content>
          </AlertDialog.Root>

          <AlertDialog.Root bind:open={tailscaleRestartDialogOpen}>
            <AlertDialog.Trigger class={buttonVariants({ variant: 'outline' })} disabled={tailscaleBusy}>
              Restart
            </AlertDialog.Trigger>
            <AlertDialog.Content>
              <AlertDialog.Header>
                <AlertDialog.Title>Restart the Tailscale container?</AlertDialog.Title>
                <AlertDialog.Description>
                  Restarts the sidecar in place (re-applying its serve config and auth key). The
                  tailnet URL is briefly unavailable while it reconnects.
                </AlertDialog.Description>
              </AlertDialog.Header>
              <AlertDialog.Footer>
                <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
                <AlertDialog.Action onclick={runTailscaleRestart}>Restart</AlertDialog.Action>
              </AlertDialog.Footer>
            </AlertDialog.Content>
          </AlertDialog.Root>
        </div>
      </Card.Content>
    </Card.Root>
    {/if}

    {#if active === 'updates'}
    <Card.Root>
      <Card.Header>
        <Card.Title>Updates</Card.Title>
        <Card.Description>
          Pull the latest from GitHub (the branch this was deployed from) and rebuild the stack.
          Checked automatically every hour. The agent is paused before an update begins, and the
          page reloads once the new build is live.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        {#if updateStatus}
          <div class="flex flex-wrap items-center gap-x-6 gap-y-1 text-sm">
            <span class="text-muted-foreground">
              Running:
              <code class="rounded bg-secondary px-1 py-0.5">{updateStatus.current_branch}</code>
              @
              <code class="rounded bg-secondary px-1 py-0.5">
                {updateStatus.current_sha === 'unknown'
                  ? 'unknown'
                  : updateStatus.current_sha.slice(0, 7)}
              </code>
            </span>
            {#if updateStatus.checked_at}
              <span class="text-muted-foreground">
                Last checked {new Date(updateStatus.checked_at).toLocaleString()}
              </span>
            {/if}
          </div>

          {#if updateStatus.update_available}
            <div
              class="flex flex-wrap items-center gap-3 rounded-lg border border-primary/40 bg-primary/10 px-4 py-3"
            >
              <span class="text-sm font-medium text-primary">An update is available.</span>
              {#if updateStatus.latest_sha}
                <code class="rounded bg-secondary px-1 py-0.5 text-xs">
                  {updateStatus.latest_sha.slice(0, 7)}
                </code>
              {/if}
              <Button
                onclick={doUpdate}
                disabled={updateRunning ||
                  updateStatus.updating ||
                  updateStatus.agent_working ||
                  !updateStatus.configured}
              >
                {updateRunning || updateStatus.updating ? 'Updating…' : 'Update'}
              </Button>
            </div>
          {:else if !updateStatus.error}
            <p class="text-sm text-success">You're on the latest version.</p>
          {/if}

          {#if updateStatus.agent_working}
            <p class="text-sm text-warning">
              The agent is working. Wait until it's idle (or pause it) to update.
            </p>
          {/if}
          {#if !updateStatus.configured}
            <p class="text-sm text-muted-foreground">
              In-app updates aren't configured: set
              <code class="rounded bg-secondary px-1 py-0.5">HOST_REPO_DIR</code> in
              <code class="rounded bg-secondary px-1 py-0.5">.env</code> (the host path to this repo)
              and restart. The version check still works without it.
            </p>
          {/if}
          {#if updateStatus.error}
            <p class="text-sm text-muted-foreground">{updateStatus.error}</p>
          {/if}
        {/if}

        <div class="flex items-center gap-3">
          <Button variant="outline" onclick={runCheck} disabled={checkingUpdate || updateRunning}>
            {checkingUpdate ? 'Checking…' : 'Check for updates'}
          </Button>
          {#if updateMessage}<span class="text-sm text-muted-foreground">{updateMessage}</span>{/if}
        </div>
      </Card.Content>
    </Card.Root>
    {/if}
  {:else}
    <p class="text-muted-foreground">Loading…</p>
  {/if}
  </div>
</div>
