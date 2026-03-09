// Copyright © 2026 Jalapeno Labs

import type { Llm } from '@prisma/client'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { LlmType } from '@prisma/client'
import { CallableCodex } from './codex'

type Context = {
  debugMock: ReturnType<typeof vi.spyOn>
  errorMock: ReturnType<typeof vi.spyOn>
}

// function hasRequiredEnvValues() {
//   return Boolean(process.env.VITEST_CODEX_AUTH_JSON)
// }

function buildLlm(overrides: Partial<Llm> = {}): Llm {
  return {
    id: 'codex-test-llm',
    userId: 'codex-test-user',
    type: LlmType.OPENAI_LOGIN_TOKEN,
    name: 'Codex Test LLM',
    isDefault: false,
    preferredModel: process.env.VITEST_CODEX_MODEL || 'gpt-5.2-codex',
    apiKey: process.env.VITEST_CODEX_AUTH_JSON || null,
    refreshToken: null,
    expiresAt: null,
    tokensUsed: 0,
    tokenLimit: 0,
    lastUsedAt: null,
    createdAt: new Date(),
    updatedAt: new Date(),
    ...overrides,
  }
}

describe('CallableCodex', () => {
  // const invalidEnvironment = !hasRequiredEnvValues()

  beforeEach<Context>((context) => {
    context.debugMock = vi.spyOn(console, 'debug').mockImplementation(() => {})
    context.errorMock = vi.spyOn(console, 'error').mockImplementation(() => {})
  })

  afterEach<Context>((context) => {
    context.debugMock.mockRestore()
    context.errorMock.mockRestore()
  })

  it('validateLlm rejects non-JSON token strings', async () => {
    const callable = new CallableCodex(
      buildLlm({
        id: 'codex-invalid-json',
        apiKey: 'not-json',
      }),
    )

    const [ success, errorMessage ] = await callable.validateLlm()
    expect(success).toBe(false)
    expect(errorMessage).toContain('not an auth.json')
  })

  it('validateLlm rejects JSON without tokens.access_token', async () => {
    const callable = new CallableCodex(
      buildLlm({
        id: 'codex-missing-access-token',
        apiKey: JSON.stringify({
          tokens: {},
        }),
      }),
    )

    const [ success, errorMessage ] = await callable.validateLlm()
    expect(success).toBe(false)
    expect(errorMessage).toContain('missing access token')
  })

  // it.skipIf(invalidEnvironment)('validateLlm accepts valid auth.json payload', async () => {
  //   const callable = new CallableCodex(buildLlm())
  //   const [ success, errorMessage ] = await callable.validateLlm()

  //   expect(errorMessage).toEqual('')
  //   expect(success).toBe(true)
  // })
})
