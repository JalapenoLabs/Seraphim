<script lang="ts">
  import type { AvailabilityWindow, EnvVar, ReviewPolicy, Settings } from '$lib/types'
  import type { EnvVarWrite } from '$lib/api'

  import { onMount } from 'svelte'

  import { KNOWN_MODELS } from '$lib/types'
  import { WEEKDAYS, minutesToTime, timeToMinutes } from '$lib/schedule'
  import { usFederalHolidays } from '$lib/holidays'
  import {
    exportConfig,
    getSettings,
    importConfig,
    listEnvVars,
    recreateWorkspace,
    restartWorkspace,
    setEnvVars,
    setTokens,
    updateSettings
  } from '$lib/api'

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
    const env = await listEnvVars()
    envRows = env.variables.map(toEnvRow)
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

  function onModelChange() {
    if (settings && modelChoice !== CUSTOM_MODEL) {
      settings.claude_model = modelChoice
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

<div class="page">
  <h1>Settings</h1>

  {#if settings}
    <section class="panel">
      <h2>Environment profile</h2>
      <div class="field">
        <label for="org">Organization name</label>
        <input id="org" bind:value={settings.org_name} />
      </div>

      <div class="field">
        <label for="model">Claude model</label>
        <select id="model" bind:value={modelChoice} onchange={onModelChange}>
          {#each KNOWN_MODELS as model}
            <option value={model.value}>{model.label}</option>
          {/each}
          <option value={CUSTOM_MODEL}>Custom…</option>
        </select>
        {#if modelChoice === CUSTOM_MODEL}
          <input
            class="custom-model"
            placeholder="exact model id, e.g. claude-opus-4-8[1m]"
            bind:value={settings.claude_model}
          />
        {/if}
        <p class="hint">
          Friendly names shown here; the coded model id is what's sent to the agent. Fable 5, Opus
          4.x, and Sonnet 4.6 are 1M-context; Haiku 4.5 is 200K.
        </p>
      </div>

      <div class="field">
        <label for="policy">Default review policy</label>
        <select id="policy" bind:value={settings.default_review_policy}>
          {#each policies as policy}
            <option value={policy}>{policy.replace(/_/g, ' ')}</option>
          {/each}
        </select>
      </div>

      <div class="field">
        <label for="global">Global agent instructions</label>
        <textarea id="global" rows="5" bind:value={settings.global_instructions}></textarea>
        <p class="hint">
          Written to <code>/workspace/AGENTS.md</code>, which the agent reads automatically at the
          start of every session. Put org-wide conventions here (how to branch, when to open vs.
          auto-merge PRs, coding standards).
        </p>
      </div>

      <div class="field">
        <label for="setup">Environment setup script</label>
        <textarea id="setup" rows="4" bind:value={settings.base_setup_script}></textarea>
        <p class="hint">
          Runs once when the workspace container is built or recreated. Use it to install CLIs and
          toolchains shared across repos (e.g. <code>corepack enable</code>, global npm packages,
          apt packages). Per-repo commands like <code>yarn install</code> belong in each
          repository's own setup script.
        </p>
      </div>

      <div class="actions">
        <button class="primary" onclick={save}>Save</button>
        {#if savedAt}<span class="muted">Saved at {savedAt}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h2>Availability schedule</h2>
      <p class="hint">
        Optional. When on, the agent only picks up new work during the hours and days you set
        here, in your time zone, and never on a skipped date. The database always stores UTC; this
        is just your local view. A task already in progress always runs to completion.
      </p>

      <div class="field checkbox">
        <input
          id="availability-enabled"
          type="checkbox"
          bind:checked={settings.availability_enabled}
        />
        <label for="availability-enabled">Restrict the agent to a schedule</label>
      </div>

      {#if settings.availability_enabled}
        <div class="field">
          <label for="timezone">Time zone</label>
          <select id="timezone" bind:value={settings.availability_timezone}>
            {#if !timezones.includes(settings.availability_timezone)}
              <option value={settings.availability_timezone}>
                {settings.availability_timezone}
              </option>
            {/if}
            {#each timezones as timezone}
              <option value={timezone}>{timezone}</option>
            {/each}
          </select>
        </div>

        <div class="field">
          <span class="section-label">Working hours</span>
          <p class="hint">
            Leave every day unchecked to allow any time of day (handy when you only want to skip
            specific dates).
          </p>
          <div class="days">
            {#each days as day, weekday}
              <div class="day-row" class:inactive={!day.active}>
                <label class="day-toggle">
                  <input type="checkbox" bind:checked={day.active} />
                  <span>{WEEKDAYS[weekday]}</span>
                </label>
                <div class="times">
                  <input type="time" bind:value={day.start} disabled={!day.active} />
                  <span>to</span>
                  <input type="time" bind:value={day.end} disabled={!day.active} />
                </div>
              </div>
            {/each}
          </div>
        </div>

        <div class="field">
          <span class="section-label">Skip dates</span>
          <p class="hint">Vacations, holidays, any single day the agent should stay idle.</p>
          <div class="skip-add">
            <input type="date" bind:value={newSkipDate} />
            <button onclick={() => addSkipDate(newSkipDate)}>Add date</button>
          </div>
          {#if skipDates.length}
            <div class="chips">
              {#each skipDates as date}
                <button class="chip" title="Remove" onclick={() => removeSkipDate(date)}>
                  {date} ✕
                </button>
              {/each}
            </div>
          {:else}
            <p class="muted">No skipped dates.</p>
          {/if}

          {#if holidaySuggestions.length}
            <p class="hint">Suggested US holidays (click to add):</p>
            <div class="chips">
              {#each holidaySuggestions as holiday}
                <button class="chip suggest" onclick={() => addSkipDate(holiday.date)}>
                  + {holiday.name} ({holiday.date})
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="actions">
        <button class="primary" onclick={saveSchedule}>Save schedule</button>
        {#if scheduleSavedAt}<span class="muted">Saved at {scheduleSavedAt}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h2>Secrets</h2>
      <p class="hint">
        Stored in the database, never in <code>.env</code> and never returned by the API. Injected
        into the agent only at runtime. Leave a field blank to keep the existing value.
      </p>
      <div class="field">
        <label for="claude-token">
          Claude OAuth token
          <span class="badge {settings.claude_token_set ? 'done' : ''}">
            {settings.claude_token_set ? 'configured' : 'not set'}
          </span>
        </label>
        <input
          id="claude-token"
          type="password"
          autocomplete="off"
          placeholder="from `claude setup-token`"
          bind:value={claudeTokenInput}
        />
        {#if settings.claude_token_preview}
          <p class="hint">Stored: <code>{settings.claude_token_preview}</code></p>
        {/if}
      </div>
      <div class="field">
        <label for="gh-token">
          GitHub token
          <span class="badge {settings.github_token_set ? 'done' : ''}">
            {settings.github_token_set ? 'configured' : 'not set'}
          </span>
        </label>
        <input
          id="gh-token"
          type="password"
          autocomplete="off"
          placeholder="PAT with repo + issues scope"
          bind:value={githubTokenInput}
        />
        {#if settings.github_token_preview}
          <p class="hint">Stored: <code>{settings.github_token_preview}</code></p>
        {/if}
      </div>
      <div class="actions">
        <button class="primary" onclick={saveTokens}>Save secrets</button>
        {#if tokensMessage}<span class="muted">{tokensMessage}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h2>Environment variables</h2>
      <p class="hint">
        Injected into the agent's environment at runtime (alongside its tokens) and available to
        setup scripts. Mark a row <strong>secret</strong> to have its value scrubbed from the
        agent's output before it reaches the logs or database, and only ever shown here masked.
      </p>

      {#if envRows.length}
        <div class="env-table">
          {#each envRows as row, index}
            <div class="env-row">
              <input class="env-key" placeholder="KEY" bind:value={row.key} />
              <input
                class="env-value"
                type={row.is_secret ? 'password' : 'text'}
                placeholder={row.is_secret && row.preview ? `${row.preview} (leave blank to keep)` : 'value'}
                autocomplete="off"
                bind:value={row.value}
              />
              <label class="env-secret" title="Scrub this value from all output">
                <input type="checkbox" bind:checked={row.is_secret} />
                <span>secret</span>
              </label>
              <button class="env-delete" title="Remove" onclick={() => removeEnvRow(index)}>
                ✕
              </button>
            </div>
          {/each}
        </div>
      {:else}
        <p class="muted">No environment variables yet.</p>
      {/if}

      <div class="actions">
        <button onclick={addEnvRow}>+ Add another</button>
        <button class="primary" onclick={saveEnv}>Save variables</button>
        {#if envMessage}<span class="muted">{envMessage}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h2>Agent config repo (~/.claude)</h2>
      <div class="field">
        <label for="configrepo">Config repo URL</label>
        <input
          id="configrepo"
          placeholder="git@github.com:navarrotech/agents.git"
          bind:value={settings.config_repo_url}
        />
        <p class="hint">
          The workspace clones this into the agent's config dir, so your <code>AGENTS.md</code>,
          docs, manuals, and skills travel with the deployment, no host mount required. Cloned over
          SSH using your mounted key. Secrets (e.g. credentials) should stay out of the repo;
          auth uses <code>CLAUDE_CODE_OAUTH_TOKEN</code>. Save, then Recreate to apply.
        </p>
      </div>
      <div class="actions">
        <button class="primary" onclick={save}>Save</button>
      </div>
    </section>

    <section class="panel">
      <h2>Backup & transfer</h2>
      <p class="muted">
        Export your settings, repositories, and sources as JSON to move a setup to another machine.
        Secrets are never included. Import merges into the current config.
      </p>
      <div class="actions">
        <button onclick={downloadExport}>Export JSON</button>
        <label class="import-button">
          Import JSON
          <input type="file" accept="application/json" onchange={onImportFile} hidden />
        </label>
        {#if importMessage}<span class="muted">{importMessage}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h2>Workspace</h2>
      <p class="muted">
        Restart re-runs the entrypoint; recreate rebuilds the container and reprovisions (config
        repo + all repos + setup scripts). The persistent volume (repos + Claude conversation) is
        preserved either way.
      </p>
      <div class="actions">
        <button onclick={runRestart}>Restart</button>
        <button onclick={runRecreate}>Recreate</button>
        {#if workspaceMessage}<span class="muted">{workspaceMessage}</span>{/if}
      </div>
    </section>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .page {
    max-width: 760px;
    margin: 0 auto;
    padding: 1.2rem 1.4rem 3rem;
  }

  .panel {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.1rem;
    margin-bottom: 1.2rem;
  }

  .panel h2 {
    margin-top: 0;
    font-size: 1rem;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 0.8rem;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .hint {
    color: var(--muted);
    font-size: 0.8rem;
    line-height: 1.45;
    margin: 0.4rem 0 0;
  }

  .hint code {
    background: var(--panel-2);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.75rem;
  }

  .custom-model {
    margin-top: 0.4rem;
  }

  .checkbox {
    flex-direction: row;
    align-items: center;
    gap: 0.6rem;
  }

  .checkbox input {
    width: auto;
  }

  .section-label {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .days {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    margin-top: 0.3rem;
  }

  .day-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.8rem;
  }

  .day-row.inactive {
    opacity: 0.55;
  }

  .day-toggle {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 9rem;
  }

  .day-toggle input {
    width: auto;
  }

  .times {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .env-table {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin: 0.6rem 0 0.9rem;
  }

  .env-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .times input {
    width: auto;
  }

  .skip-add {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  .skip-add input {
    width: auto;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin: 0.5rem 0;
  }

  .chip {
    font-size: 0.78rem;
    padding: 0.2rem 0.55rem;
    border-radius: 999px;
  }

  .chip.suggest {
    border-style: dashed;
    color: var(--muted);
  }

  .env-key {
    flex: 0 0 30%;
    font-family: monospace;
  }

  .env-value {
    flex: 1;
  }

  .env-secret {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--muted);
    font-size: 0.8rem;
    white-space: nowrap;
  }

  .env-secret input {
    width: auto;
  }

  .env-delete {
    flex: 0 0 auto;
    padding: 0.4rem 0.6rem;
  }

  .import-button {
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
  }

  .import-button:hover {
    border-color: var(--accent);
  }
</style>
