<script lang="ts">
  import RepoForm from '$lib/components/RepoForm.svelte'
</script>

<div class="mx-auto max-w-3xl space-y-4 px-6 py-6">
  <a href="/repos" class="text-sm text-muted-foreground hover:text-foreground">← Repositories</a>
  <h1 class="text-2xl font-semibold">Add a repository</h1>
  <RepoForm />
</div>
