import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// In local `vite dev`, proxy API calls to the Rust backend. In production the
// SvelteKit server proxies them instead (see src/hooks.server.ts).
export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    proxy: {
      '/api': 'http://localhost:27182'
    }
  }
})
