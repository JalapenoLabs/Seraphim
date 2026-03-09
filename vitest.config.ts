// Copyright © 2026 Jalapeno Labs

import type { VitestEnvironment } from 'vitest/node'

// Core
import 'dotenv/config'
import { defineConfig } from 'vitest/config'
import { resolve } from 'path'

// Plugins:
import tsconfigPaths from 'vite-tsconfig-paths'

// Env
const IS_CI = process.env.GITHUB_ACTIONS === 'true'
const TEST_RUNNING_DIR = process.env.TEST_RUNNING_DIR || 'all'
const IS_SMOKETEST = process.env.SMOKETEST === 'true'
const environment: VitestEnvironment = [ 'frontend', 'vscode', 'chrome' ].includes(TEST_RUNNING_DIR)
  ? 'happy-dom'
  : 'node'


export default defineConfig({
  plugins: [
    tsconfigPaths(),
  ],
  test: {
    setupFiles: resolve(__dirname, 'common/src/setupVitest.ts'),
    include: IS_SMOKETEST
      ? [ '**/*.spec.ts' ]
      : [ '**/*.test.ts' ],
    reporters: IS_CI
      ? [ 'junit', 'verbose' ]
      : [ 'verbose' ],
    outputFile: {
      'junit': `.test/${TEST_RUNNING_DIR}-results.xml`,
    },
    passWithNoTests: true,
    silent: false,
    // Coverage (V8)
    coverage: {
      reporter: [
        'text-summary',
      ],
      reportsDirectory: `.test/${TEST_RUNNING_DIR}/coverage`,
      provider: 'v8',
    },
    // Typescript
    typecheck: {
      enabled: true,
    },
    // React.js:
    globals: true,
    environment,

    // CICD compatibility:
    minWorkers: 2,
    maxWorkers: 3,
    logHeapUsage: true,
    testTimeout: 60_000,

    // Debugging:
    onStackTrace(error, { file }): boolean | void {
      // If we've encountered a ReferenceError, show the whole stack.
      if (error.name === 'ReferenceError') {
        return false
      }

      // Reject all frames from third party libraries.
      if (file.includes('node_modules')) {
        return false
      }

      return true
    },
  },
  resolve: {
    alias: {
      '@common': resolve(__dirname, 'common/src'),
      '@': resolve(__dirname, 'src'),
    },
  },
})
