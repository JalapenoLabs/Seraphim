import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import Icons from 'unplugin-icons/vite'
import { defineConfig } from 'vite'

// In local `vite dev`, proxy API calls to the Rust backend. In production the
// SvelteKit server proxies them instead (see src/hooks.server.ts).
export default defineConfig({
  // `unplugin-icons` compiles `~icons/<set>/<name>` imports into Svelte
  // components at build time (tree-shaken, offline) from the @iconify/json data.
  plugins: [tailwindcss(), Icons({ compiler: 'svelte' }), sveltekit()],
  server: {
    proxy: {
      '/api': 'http://localhost:27182'
    }
  }
})
