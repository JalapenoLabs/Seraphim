<script lang="ts">
  import type { IssueSource, ReviewPolicy, Settings } from '$lib/types'

  import { onMount } from 'svelte'

  import {
    createSource,
    deleteSource,
    getSettings,
    listSources,
    recreateWorkspace,
    restartWorkspace,
    updateSettings
  } from '$lib/api'

  let settings = $state<Settings | null>(null)
  let sources = $state<IssueSource[]>([])
  let savedAt = $state<string | null>(null)
  let workspaceMessage = $state<string | null>(null)

  // New GitHub source form.
  let newOwner = $state('')
  let newRepo = $state('')
  let newLabels = $state('')

  const policies: ReviewPolicy[] = ['auto_squash_merge', 'human_review', 'none']

  async function load() {
    settings = await getSettings()
    sources = await listSources()
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
      base_setup_script: settings.base_setup_script
    })
    savedAt = new Date().toLocaleTimeString()
  }

  async function addSource() {
    if (!newOwner.trim() || !newRepo.trim()) {
      return
    }
    const labels = newLabels
      .split(',')
      .map((label) => label.trim())
      .filter(Boolean)
    await createSource('github', { owner: newOwner.trim(), repo: newRepo.trim(), labels })
    newOwner = ''
    newRepo = ''
    newLabels = ''
    sources = await listSources()
  }

  async function removeSource(sourceId: string) {
    await deleteSource(sourceId)
    sources = await listSources()
  }

  async function runRestart() {
    workspaceMessage = 'Restarting…'
    await restartWorkspace()
    workspaceMessage = 'Workspace restarted.'
  }

  async function runRecreate() {
    workspaceMessage = 'Recreating…'
    await recreateWorkspace()
    workspaceMessage = 'Workspace recreated; setup script re-run.'
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
        <input id="model" bind:value={settings.claude_model} />
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
      </div>
      <div class="field">
        <label for="setup">Workspace base setup script</label>
        <textarea id="setup" rows="4" bind:value={settings.base_setup_script}></textarea>
      </div>
      <div class="actions">
        <button class="primary" onclick={save}>Save</button>
        {#if savedAt}<span class="muted">Saved at {savedAt}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h2>Issue sources</h2>
      {#if sources.length === 0}
        <p class="muted">No sources configured.</p>
      {/if}
      {#each sources as source}
        <div class="row">
          <span class="badge">{source.kind}</span>
          <span class="mono">{JSON.stringify(source.config)}</span>
          <button onclick={() => removeSource(source.id)}>Remove</button>
        </div>
      {/each}

      <div class="add-source">
        <input placeholder="owner" bind:value={newOwner} />
        <input placeholder="repo" bind:value={newRepo} />
        <input placeholder="labels (comma-separated, optional)" bind:value={newLabels} />
        <button onclick={addSource}>Add GitHub source</button>
      </div>
    </section>

    <section class="panel">
      <h2>Workspace</h2>
      <p class="muted">
        Restart re-runs the entrypoint; recreate rebuilds the container and re-runs the base setup
        script. The persistent volume (repos + Claude conversation) is preserved either way.
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

  .row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.4rem 0;
    border-bottom: 1px solid var(--border);
  }

  .mono {
    font-family: ui-monospace, monospace;
    font-size: 0.8rem;
    color: var(--muted);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .add-source {
    display: grid;
    grid-template-columns: 1fr 1fr 2fr auto;
    gap: 0.5rem;
    margin-top: 0.8rem;
  }
</style>
