// Copyright © 2026 Jalapeno Labs

import type { CodexAuthJson } from '@common/types'
import type { RateLimitWindow } from '@common/vendor/codex-protocol/v2/RateLimitWindow'
import type { RateLimitSnapshot } from '@common/vendor/codex-protocol/v2/RateLimitSnapshot'
import type { PlanType } from '@common/vendor/codex-protocol/PlanType'

// Core
import { BaseCallableLLM } from './base'
import { Codex } from '@openai/codex-sdk'

// Node.js
import { tmpdir } from 'node:os'
import { resolve } from 'node:path'
import { writeFile, mkdtemp, rm } from 'node:fs/promises'
import { existsSync } from 'node:fs'

// Misc
import { safeParseJson } from '@common/json'
import { Timer } from '@common/timer'

export class CallableCodex extends BaseCallableLLM {
  private getTargetTriple() {
    if (process.platform === 'linux' || process.platform === 'android') {
      if (process.arch === 'x64') {
        return 'x86_64-unknown-linux-musl'
      }

      if (process.arch === 'arm64') {
        return 'aarch64-unknown-linux-musl'
      }
    }

    if (process.platform === 'darwin') {
      if (process.arch === 'x64') {
        return 'x86_64-apple-darwin'
      }

      if (process.arch === 'arm64') {
        return 'aarch64-apple-darwin'
      }
    }

    if (process.platform === 'win32') {
      if (process.arch === 'x64') {
        return 'x86_64-pc-windows-msvc'
      }

      if (process.arch === 'arm64') {
        return 'aarch64-pc-windows-msvc'
      }
    }

    throw new Error(`Unsupported platform: ${process.platform} (${process.arch})`)
  }

  private getCodexPathOverride(): string | undefined {
    const codexBinaryName = process.platform === 'win32' ? 'codex.exe' : 'codex'
    const targetTriple = this.getTargetTriple()

    // `@openai/codex-sdk` does not export package.json, so probe workspace roots directly.
    const candidateWorkspaceRoots = [
      process.cwd(),
      resolve(process.cwd(), '..'),
    ]

    for (const workspaceRoot of candidateWorkspaceRoots) {
      const codexBinaryPath = resolve(
        workspaceRoot,
        'node_modules',
        '@openai',
        'codex-sdk',
        'vendor',
        targetTriple,
        'codex',
        codexBinaryName,
      )

      if (existsSync(codexBinaryPath)) {
        return codexBinaryPath
      }
    }

    console.warn('Codex binary path override could not be resolved from known node_modules locations', {
      cwd: process.cwd(),
      targetTriple,
    })

    return undefined
  }

  private async getCodex(): Promise<[ Codex, string ]> {
    let contextDirectory: string | null = null

    contextDirectory = await mkdtemp(
      resolve(tmpdir(), 'seraphim-codex-'),
    )

    // In our database, we save the "auth.json" file as a string under the apiKey attribute
    await Promise.all([
      writeFile(
        resolve(contextDirectory, 'auth.json'),
        this.llm.apiKey,
        'utf-8',
      ),
      writeFile(
        resolve(contextDirectory, 'config.toml'),
        `cli_auth_credentials_store = "file"\n`,
        'utf-8',
      ),
    ])

    const client = new Codex({
      codexPathOverride: this.getCodexPathOverride(),
      env: {
        CODEX_HOME: contextDirectory,
      },
    })

    return [ client, contextDirectory ]
  }

  private async teardownCodex(contextDirectory: string) {
    try {
      if (contextDirectory) {
        await rm(contextDirectory, { recursive: true, force: true })
      }
    }
    catch (error) {
      console.error('Failed to clean up Codex context directory', error)
    }
  }

  public async query(prompt: string, systemPrompt?: string): Promise<string> {
    const timer = new Timer('Codex query')

    let contextDirectory: string | null = null
    try {
      const [ client, contextDir ] = await this.getCodex()
      contextDirectory = contextDir

      let normalizedPrompt = ''
      if (systemPrompt) {
        normalizedPrompt += 'System prompt:\n' + systemPrompt.trim() + '\n\nUser prompt:\n'
      }
      normalizedPrompt += (prompt?.trim() || '')

      if (!normalizedPrompt) {
        console.log('Codex query was empty after normalization')
        return ''
      }

      console.log('Sending query to Codex', { prompt: normalizedPrompt, systemPrompt })

      const thread = client.startThread()
      const result = await thread.run(normalizedPrompt)

      console.log('Codex response', { response: result.finalResponse })

      return result.finalResponse
    }
    catch (error) {
      console.error('Error during Codex query', error)
    }
    finally {
      await this.teardownCodex(contextDirectory)
      timer.stop()
    }

    return ''
  }

  public async getRateLimits(): Promise<RateLimitSnapshot | null> {
    try {
      const auth: CodexAuthJson = JSON.parse(this.llm.apiKey)

      if (!auth.tokens.access_token) {
        throw new Error('Missing codex access token in auth.json!')
      }

      type AccountStatus = {
        user_id?: string
        account_id?: string
        email?: string
        plan_type?: PlanType | null
        rate_limit?: {
          allowed?: boolean
          limit_reached?: boolean
          primary_window?: {
            used_percent?: number
            limit_window_seconds?: number
            reset_after_seconds?: number
            reset_at?: number
          } | null
          secondary_window?: {
            used_percent?: number
            limit_window_seconds?: number
            reset_after_seconds?: number
            reset_at?: number
          } | null
        } | null
        code_review_rate_limit?: {
          allowed?: boolean
          limit_reached?: boolean
          primary_window?: {
            used_percent?: number
            limit_window_seconds?: number
            reset_after_seconds?: number
            reset_at?: number
          } | null
          secondary_window?: {
            used_percent?: number
            limit_window_seconds?: number
            reset_after_seconds?: number
            reset_at?: number
          } | null
        } | null
        additional_rate_limits?: unknown
        credits?: {
          has_credits?: boolean
          unlimited?: boolean
          balance?: string | null
          approx_local_messages?: [number, number] | number[]
          approx_cloud_messages?: [number, number] | number[]
        } | null
        promo?: unknown
      }

      const response = await fetch('https://chatgpt.com/backend-api/wham/usage', {
        headers: {
          'Accept': '*/*',
          'User-Agent': 'Seraphim',
          'Authorization': `Bearer ${auth.tokens.access_token}`,
        },
      })

      if (!response.ok) {
        throw new Error(`Failed to fetch usage statistics: ${response.statusText}`)
      }

      const content: AccountStatus = await response.json()

      function toRateLimitWindow(window: AccountStatus['rate_limit']['primary_window']): RateLimitWindow | null {
        if (!window) {
          return null
        }

        return {
          usedPercent:
            typeof window.used_percent === 'number' && Number.isFinite(window.used_percent)
              ? window.used_percent
              : 0,
          windowDurationMins:
            typeof window.limit_window_seconds === 'number' && Number.isFinite(window.limit_window_seconds)
              ? Math.round(window.limit_window_seconds / 60)
              : null,
          resetsAt:
            typeof window.reset_at === 'number' && Number.isFinite(window.reset_at)
              ? window.reset_at
              : null, // preserve server epoch format as-is
        }
      }

      const rateLimits: RateLimitSnapshot = {
        primary: toRateLimitWindow(content.rate_limit?.primary_window),
        secondary: toRateLimitWindow(content.rate_limit?.secondary_window),
        credits: content.credits
          ? {
              hasCredits: Boolean(content.credits.has_credits),
              unlimited: Boolean(content.credits.unlimited),
              balance: content.credits.balance ?? null,
            }
          : null,
        planType: content.plan_type || 'unknown',
      }

      return rateLimits
    }
    catch (error) {
      console.error('Error during Codex rate limit request', error)
    }

    return null
  }

  public async validateLlm(): Promise<[ boolean, string ]> {
    try {
      const authAsJson: CodexAuthJson = safeParseJson(this.llm.apiKey)
      if (!authAsJson) {
        console.debug('Codex validation failed, apiKey is not valid JSON')
        return [ false, 'Invalid api key provided (not an auth.json)' ]
      }

      if (!authAsJson.tokens?.access_token) {
        console.debug('Codex validation failed, apiKey JSON does not contain access token', { authAsJson })
        return [ false, 'Invalid api key provided (missing access token)' ]
      }

      const response = await this.query('Please reply with hello world.')

      if (response?.length && typeof response === 'string') {
        return [ true, '' ]
      }

      console.warn('Codex validation failed, unexpected response', { response })
      return [ false, 'Codex did not return a valid response' ]
    }
    catch (error) {
      console.error('Error during Codex validation', error)
    }

    return [ false, 'Codex authentication failed' ]
  }
}
