<script lang="ts">
  import '../app.css'
  import { page } from '$app/stores'

  import Notifications from '$lib/components/Notifications.svelte'

  let { children } = $props()

  const links = [
    { href: '/', label: 'Board' },
    { href: '/repos', label: 'Repositories' },
    { href: '/settings', label: 'Settings' }
  ]
</script>

<div class="shell">
  <header>
    <a class="brand" href="/">Seraphim</a>
    <nav>
      {#each links as link}
        <a class="nav-link" class:active={$page.url.pathname === link.href} href={link.href}>
          {link.label}
        </a>
      {/each}
    </nav>
    <div class="spacer"></div>
    <Notifications />
  </header>
  <main>
    {@render children()}
  </main>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  header {
    display: flex;
    align-items: center;
    gap: 2rem;
    padding: 0.8rem 1.4rem;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }

  .brand {
    font-weight: 700;
    font-size: 1.15rem;
    color: var(--text);
    letter-spacing: 0.02em;
  }

  nav {
    display: flex;
    gap: 0.4rem;
  }

  .spacer {
    flex: 1;
  }

  .nav-link {
    color: var(--muted);
    padding: 0.35rem 0.7rem;
    border-radius: 8px;
  }

  .nav-link.active,
  .nav-link:hover {
    color: var(--text);
    background: var(--panel-2);
  }

  main {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
