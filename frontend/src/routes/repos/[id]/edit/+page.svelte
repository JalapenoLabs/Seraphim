<script lang="ts">
  import type { Repository } from '$lib/types'

  import { onMount } from 'svelte'
  import { page } from '$app/stores'

  import { listRepos } from '$lib/api'
  import RepoForm from '$lib/components/RepoForm.svelte'

  const id = $page.params.id ?? ''
  let repo = $state<Repository | null>(null)
  let notFound = $state(false)

  onMount(async () => {
    try {
      const found = (await listRepos()).find((candidate) => candidate.id === id)
      if (found) {
        repo = found
      } else {
        notFound = true
      }
    } catch (error) {
      console.debug('failed to load the repository', error)
      notFound = true
    }
  })
</script>

<div class="mx-auto max-w-3xl space-y-4 px-6 py-6">
  <a href="/repos" class="text-sm text-muted-foreground hover:text-foreground">← Repositories</a>
  {#if repo}
    <h1 class="text-2xl font-semibold">Edit {repo.full_name}</h1>
    <RepoForm {repo} />
  {:else if notFound}
    <p class="text-muted-foreground">Repository not found.</p>
  {:else}
    <p class="text-muted-foreground">Loading…</p>
  {/if}
</div>
