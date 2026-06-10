<script lang="ts">
  import type { Repository, ReviewPolicy } from '$lib/types'

  import { onMount } from 'svelte'

  import { deleteRepo, listRepos, upsertRepo } from '$lib/api'

  type FormState = {
    full_name: string
    clone_url: string
    default_branch: string
    branch_template: string
    review_policy: ReviewPolicy | ''
    instructions: string
    setup_script: string
    enabled: boolean
  }

  function emptyForm(): FormState {
    return {
      full_name: '',
      clone_url: '',
      default_branch: 'main',
      branch_template: 'seraphim/issue-{number}-{slug}',
      review_policy: '',
      instructions: '',
      setup_script: '',
      enabled: true
    }
  }

  let repos = $state<Repository[]>([])
  let form = $state<FormState>(emptyForm())

  async function load() {
    repos = await listRepos()
  }

  function edit(repo: Repository) {
    form = {
      full_name: repo.full_name,
      clone_url: repo.clone_url,
      default_branch: repo.default_branch,
      branch_template: repo.branch_template,
      review_policy: repo.review_policy ?? '',
      instructions: repo.instructions,
      setup_script: repo.setup_script,
      enabled: repo.enabled
    }
  }

  async function submit() {
    if (!form.full_name.trim()) {
      return
    }
    // Default the clone URL from the full name for GitHub repos.
    const cloneUrl = form.clone_url.trim() || `https://github.com/${form.full_name.trim()}.git`
    await upsertRepo({
      full_name: form.full_name.trim(),
      clone_url: cloneUrl,
      default_branch: form.default_branch,
      branch_template: form.branch_template,
      review_policy: form.review_policy === '' ? null : form.review_policy,
      instructions: form.instructions,
      setup_script: form.setup_script,
      enabled: form.enabled
    })
    form = emptyForm()
    await load()
  }

  async function remove(repoId: string) {
    await deleteRepo(repoId)
    await load()
  }

  onMount(load)
</script>

<div class="page">
  <h1>Repositories</h1>

  <section class="panel">
    <h2>{form.full_name ? `Edit ${form.full_name}` : 'Add a repository'}</h2>
    <div class="grid">
      <div class="field">
        <label for="full">Full name (owner/repo)</label>
        <input id="full" placeholder="navarrotech/seraphim" bind:value={form.full_name} />
      </div>
      <div class="field">
        <label for="clone">Clone URL (optional)</label>
        <input id="clone" placeholder="defaults from full name" bind:value={form.clone_url} />
      </div>
      <div class="field">
        <label for="branch">Default branch</label>
        <input id="branch" bind:value={form.default_branch} />
      </div>
      <div class="field">
        <label for="tmpl">Branch template</label>
        <input id="tmpl" bind:value={form.branch_template} />
      </div>
      <div class="field">
        <label for="rpolicy">Review policy</label>
        <select id="rpolicy" bind:value={form.review_policy}>
          <option value="">inherit default</option>
          <option value="auto_squash_merge">auto squash merge</option>
          <option value="human_review">human review</option>
          <option value="none">none</option>
        </select>
      </div>
      <div class="field checkbox">
        <label for="enabled">Enabled</label>
        <input id="enabled" type="checkbox" bind:checked={form.enabled} />
      </div>
    </div>
    <p class="hint">
      Clone URL accepts SSH (<code>git@github.com:owner/repo.git</code>) or HTTPS. SSH uses your
      mounted <code>~/.ssh</code> key; HTTPS uses <code>GH_TOKEN</code>.
    </p>
    <div class="field">
      <label for="instr">Repo-specific instructions</label>
      <textarea id="instr" rows="3" bind:value={form.instructions}></textarea>
      <p class="hint">
        Written to <code>/workspace/{'{repo}'}/CLAUDE.md</code>, loaded whenever the agent works in
        this repo. Put build/test commands and repo-specific gotchas here.
      </p>
    </div>
    <div class="field">
      <label for="rsetup">Setup script (run after clone/checkout)</label>
      <textarea id="rsetup" rows="3" bind:value={form.setup_script}></textarea>
      <p class="hint">
        Runs in this repo after it's cloned/updated (e.g. <code>yarn install</code>,
        <code>corepack enable</code>). Tools shared across all repos belong in the environment setup
        script under Settings.
      </p>
    </div>
    <div class="actions">
      <button class="primary" onclick={submit}>Save repository</button>
      <button onclick={() => (form = emptyForm())}>Clear</button>
    </div>
  </section>

  <section class="panel">
    <h2>Configured</h2>
    {#if repos.length === 0}
      <p class="muted">No repositories yet.</p>
    {/if}
    {#each repos as repo}
      <div class="row">
        <div class="info">
          <strong>{repo.full_name}</strong>
          <span class="badge">{repo.review_policy ?? 'inherit'}</span>
          {#if !repo.enabled}<span class="muted">disabled</span>{/if}
        </div>
        <div class="row-actions">
          <button onclick={() => edit(repo)}>Edit</button>
          <button onclick={() => remove(repo.id)}>Delete</button>
        </div>
      </div>
    {/each}
  </section>
</div>

<style>
  .page {
    max-width: 820px;
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

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 1rem;
  }

  .checkbox {
    flex-direction: row;
    align-items: center;
    gap: 0.6rem;
  }

  .checkbox input {
    width: auto;
  }

  .actions {
    display: flex;
    gap: 0.7rem;
  }

  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
  }

  .info {
    display: flex;
    align-items: center;
    gap: 0.7rem;
  }

  .row-actions {
    display: flex;
    gap: 0.5rem;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .hint {
    color: var(--muted);
    font-size: 0.8rem;
    line-height: 1.45;
    margin: 0.2rem 0 0.6rem;
  }

  .hint code {
    background: var(--panel-2);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.75rem;
  }
</style>
