// Copyright © 2026 Jalapeno Labs

// Lib
import { z } from 'zod'

export const taskCreateSchema = z
  .object({
    workspaceId: z
      .string()
      .trim()
      .min(1, 'Workspace is required'),
    gitAccountId: z
      .string()
      .trim()
      .min(1, 'Git account is required'),
    llmId: z
      .string()
      .trim()
      .min(1, 'LLM is required'),
    issueTrackingId: z
      .string()
      .trim()
      .min(1, 'Issue tracking is required')
      .optional(),
    message: z
      .string()
      .trim()
      .min(1, 'A base prompt message is required'),
    branch: z
      .string()
      .trim()
      .min(1, 'A branch is required'),
    issueLink: z
      .string()
      .optional(),
    archived: z
      .boolean()
      .optional()
      .default(false),
  })
  .strict()

export const taskUpdateSchema = z
  .object({
    name: z.string().trim().min(1).optional(),
    branch: z.string().trim().min(1).optional(),
    container: z.string().trim().min(1).optional(),
    archived: z.boolean().optional(),
  })
  .strict()
  .refine(
    (data) => Object.keys(data).length > 0,
    { message: 'No valid fields provided for update' },
  )

export const taskNewMessageSchema = z
  .object({
    message: z
      .string()
      .trim()
      .min(1, 'Message is required'),
  })
  .strict()

export type TaskCreateRequest = z.infer<typeof taskCreateSchema>
export type TaskUpdateRequest = z.infer<typeof taskUpdateSchema>
export type TaskNewMessageRequest = z.infer<typeof taskNewMessageSchema>
