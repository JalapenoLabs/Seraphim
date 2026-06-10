<script lang="ts">
  import '../app.css'
  import { page } from '$app/stores'

  let { children } = $props()

  const links = [
    { href: '/', label: 'Board' },
    { href: '/repos', label: 'Repositories' },
    { href: '/settings', label: 'Settings' }
  ]
</script>

<div class="flex h-screen flex-col">
  <header class="flex items-center gap-8 border-b border-border bg-card px-6 py-3">
    <a href="/" class="flex items-center gap-2 text-lg font-bold tracking-tight text-foreground">
      <img
        src="/favicon.png"
        alt=""
        class="h-6 w-6"
        onerror={(event) => ((event.currentTarget as HTMLImageElement).style.display = 'none')}
      />
      Seraphim
    </a>
    <nav class="flex gap-1">
      {#each links as link}
        <a
          href={link.href}
          class="rounded-md px-3 py-1.5 text-sm transition-colors {$page.url.pathname === link.href
            ? 'bg-secondary text-foreground'
            : 'text-muted-foreground hover:bg-secondary hover:text-foreground'}"
        >
          {link.label}
        </a>
      {/each}
    </nav>
  </header>
  <main class="min-h-0 flex-1 overflow-auto">
    {@render children()}
  </main>
</div>
