// Copyright © 2026 Jalapeno Labs

// Lib
import { z } from 'zod'

export const workspaceIdSchema = z.string().trim().uuid()
export const userIdSchema = z.string().trim().uuid()
export const taskIdSchema = z.string().trim().uuid()
export const llmIdSchema = z.string().trim().uuid()
export const gitAccountIdSchema = z.string().trim().uuid()
export const issueTrackingIdSchema = z.string().trim().uuid()
