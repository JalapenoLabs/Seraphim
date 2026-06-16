import { defineConfig } from 'vitest/config'

// Unit tests run as plain TypeScript in a node environment, deliberately without
// the SvelteKit/Tailwind plugins: the only tested code is pure logic (e.g. the
// runewood event mapper), so the lighter config keeps `yarn test` fast and free
// of browser/SvelteKit setup.
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts']
  }
})
