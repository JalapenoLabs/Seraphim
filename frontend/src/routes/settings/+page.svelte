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
    updateSettings
  } from '$lib/api'

  const CUSTOM_MODEL = '__custom__'

  let settings = $state<Settings | null>(null)
  let savedAt = $state<string | null>(null)
  let workspaceMessage = $state<string | null>(null)
  let importMessage = $state<string | null>(null)

  // Model picker: a dropdown of known ids plus a custom free-text fallback.
  let modelChoice = $state<string>(KNOWN_MODELS[0])

  const policies: ReviewPolicy[] = ['auto_squash_merge', 'human_review', 'none']

  async function load() {
    settings = await getSettings()
    modelChoice = KNOWN_MODELS.includes(settings.claude_model)
      ? settings.claude_model
      : CUSTOM_MODEL
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
            <option value={model}>{model}</option>
          {/each}
          <option value={CUSTOM_MODEL}>Custom…</option>
        </select>
        {#if modelChoice === CUSTOM_MODEL}
          <input
            class="custom-model"
            placeholder="exact model id, e.g. a brand-new release"
            bind:value={settings.claude_model}
          />
        {/if}
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
