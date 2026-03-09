// Copyright © 2026 Jalapeno Labs

import { defineConfig } from 'vite'
import path from 'path'

// https://vitejs.dev/config
export default defineConfig({
  plugins: [
    {
      // This will tell Electron to restart the app when the main process code changes.
      // https://github.com/electron/forge/issues/682#issuecomment-1793552649
      name: 'restart',
      closeBundle() {
        process.stdin.emit('data', 'rs')
      },
    },
  ],
  build: {
    rollupOptions: {
      external: [
        'cpu-features',
        'dockerode',
        'ws',
        'bufferutil',
        'utf-8-validate',
      ],
    },
    // (In KB) We don't need to worry about large assets, as this is a desktop app deployed via GUI.
    // The only reason we would be concerned about this is if it was being served over a network.
    // It's served straight from the disk so it's not a big deal.
    // https://vite.dev/config/build-options.html#build-chunksizewarninglimit
    chunkSizeWarningLimit: 1_000 * 100, // 100 MB
  },
  resolve: {
    alias: {
      '@common': path.resolve(__dirname, '../common/src'),
      '@electron': path.resolve(__dirname, 'src'),
    },
  },
})

