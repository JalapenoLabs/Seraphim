// Copyright © 2026 Jalapeno Labs

// Lib
import { z } from 'zod'

export const githubBranchSchema = z.object({
  name: z.string(),
  protected: z.boolean(),
  commit: z.object({
    sha: z.string(),
  }),
})

export const githubRepoSchema = z.object({
  default_branch: z.string(),
})

export const githubPullRequestSchema = z.object({
  number: z.number().int(),
  html_url: z.string().url(),
  title: z.string(),
  body: z.string().nullable(),
  state: z.enum([ 'open', 'closed' ]),
  draft: z.boolean(),
  merged_at: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  closed_at: z.string().nullable(),
  user: z.object({
    login: z.string(),
    html_url: z.string().url(),
  }).nullable(),
  head: z.object({
    ref: z.string(),
    sha: z.string(),
  }),
  base: z.object({
    ref: z.string(),
  }),
})

export const githubCommitStatusSchema = z.object({
  state: z.enum([ 'error', 'failure', 'pending', 'success' ]),
})

export type GithubBranchPayload = z.infer<typeof githubBranchSchema>
export type GithubRepoPayload = z.infer<typeof githubRepoSchema>
export type GithubPullRequestPayload = z.infer<typeof githubPullRequestSchema>
export type GithubCommitStatusPayload = z.infer<typeof githubCommitStatusSchema>
