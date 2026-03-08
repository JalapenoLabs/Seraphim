// Copyright © 2026 Jalapeno Labs

import { z } from 'zod'
import { IssueTrackingProvider } from '@prisma/client'

const optionalStringSchema = z.string().trim().optional()

function hasValue(value?: string) {
  return Boolean(value?.trim())
}

export const upsertIssueTrackingSchema = z
  .object({
    name: z.string().trim().min(1).optional(),
    provider: z.nativeEnum(IssueTrackingProvider),
    baseUrl: z.string().trim().url().optional(),
    email: optionalStringSchema,
    accessToken: optionalStringSchema,
    targetBoard: z.string().trim().min(1).optional(),
  })
  .superRefine((value, context) => {
    const ownerOrEmail = value.email?.trim()
    const targetBoard = value.targetBoard?.trim()

    if (value.provider === 'Jira') {
      if (hasValue(ownerOrEmail) && !z.string().email().safeParse(ownerOrEmail).success) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: [ 'email' ],
          message: 'Jira account email must be a valid email address',
        })
      }
      return
    }

    if (!hasValue(ownerOrEmail)) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: [ 'email' ],
        message: 'GitHub organization or owner is required',
      })
    }

    if (!hasValue(targetBoard)) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: [ 'targetBoard' ],
        message: 'GitHub repository is required',
      })
    }
  })

export type UpsertIssueTrackingRequest = z.infer<typeof upsertIssueTrackingSchema>

export const listIssueTrackingIssuesParamsSchema = z.object({
  issueTrackingId: z.string().trim().uuid(),
})
export type ListIssueTrackingIssuesParams = z.infer<typeof listIssueTrackingIssuesParamsSchema>

export const listIssueTrackingIssuesQuerySchema = z.object({
  q: z.string().trim().optional(),
  mode: z.enum([ 'text', 'jql' ]).optional(),
  page: z.coerce.number().int().positive().optional(),
  limit: z.coerce.number().int().positive().max(100).optional(),
})
export type ListIssueTrackingIssuesQuery = z.infer<typeof listIssueTrackingIssuesQuerySchema>

export const listIssueTrackingIssuesRequestSchema = z.object({
  issueTrackingId: z.string().trim().uuid(),
  q: z.string().trim().optional(),
  mode: z.enum([ 'text', 'jql' ]).optional(),
  page: z.coerce.number().int().positive().optional(),
  limit: z.coerce.number().int().positive().max(100).optional(),
})
export type ListIssueTrackingIssuesRequest = z.infer<typeof listIssueTrackingIssuesRequestSchema>
