// Copyright © 2026 Jalapeno Labs

import type { GitAccount } from '@prisma/client'
import type { Request, Response } from 'express'

// Core
import { parseRequestParams } from '../../validation'

// Lib
import { z } from 'zod'

// Utility
import { gitAccountIdSchema } from '@electron/validators'
import { requireDatabaseClient } from '@electron/database'
import { sanitizeAccount } from './utils'

type RouteParams = {
  accountId: string
}

type GetAccountResponse = {
  account: GitAccount
}

const getAccountParamsSchema = z.object({
  accountId: gitAccountIdSchema,
})

export async function handleGetGitAccountRequest(
  request: Request<RouteParams>,
  response: Response<GetAccountResponse | { error: string }>,
): Promise<void> {
  const databaseClient = requireDatabaseClient('Get git account')

  const params = parseRequestParams(
    getAccountParamsSchema,
    request,
    response,
    {
      context: 'Get git account',
      errorMessage: 'Invalid account identifier',
    },
  )
  if (!params) {
    console.debug('Get account request failed route param validation')
    return
  }

  try {
    const account = await databaseClient.gitAccount.findUnique({
      where: {
        id: params.accountId,
      },
    })

    if (!account) {
      console.debug('Git account not found', {
        accountId: params.accountId,
      })
      response.status(404).json({ error: 'Git account not found' })
      return
    }

    response.status(200).json({
      account: sanitizeAccount(account),
    })
  }
  catch (error) {
    console.error('Failed to fetch git account', error)
    if (!response.headersSent) {
      response.status(500).json({ error: 'Failed to fetch git account' })
    }
  }
}
