// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@prisma/client'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { IssueTrackingProvider } from '@prisma/client'
import { GithubIssueTracker } from './github'

function buildIssueTracking(
  overrides: Partial<IssueTracking> = {},
): IssueTracking {
  return {
    id: 'github-test',
    userId: 'user-test',
    provider: IssueTrackingProvider.Github,
    accessToken: 'test-token',
    baseUrl: 'https://api.github.com',
    name: 'GitHub Test Account',
    email: 'jalapenolabs',
    targetBoard: 'seraphim',
    lastUsedAt: null,
    createdAt: new Date(),
    updatedAt: new Date(),
    ...overrides,
  }
}

type RequestMock = ReturnType<typeof vi.fn>

function setOctokitRequestMock(
  tracker: GithubIssueTracker,
  requestMock: RequestMock,
) {
  Reflect.set(tracker, 'octokit', {
    request: requestMock,
  })
}

describe('GithubIssueTracker', () => {
  beforeEach(() => {
    vi.spyOn(console, 'debug').mockImplementation(() => {})
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('check fails when owner is missing', async () => {
    const tracker = new GithubIssueTracker(
      buildIssueTracking({
        email: '',
      }),
    )

    const [ success, error ] = await tracker.check()

    expect(success).toBe(false)
    expect(error).toContain('owner')
  })

  it('check validates repository access', async () => {
    const tracker = new GithubIssueTracker(buildIssueTracking())

    const requestMock = vi.fn().mockResolvedValue({
      status: 200,
      data: {
        id: 1,
      },
    })

    setOctokitRequestMock(tracker, requestMock)

    const [ success, error ] = await tracker.check()

    expect(success, error).toBe(true)
    expect(requestMock).toHaveBeenCalledWith('GET /repos/{owner}/{repo}', {
      owner: 'jalapenolabs',
      repo: 'seraphim',
      headers: {
        'X-GitHub-Api-Version': '2022-11-28',
      },
    })
  })

  it('listIssues maps issue payloads and excludes pull requests', async () => {
    const tracker = new GithubIssueTracker(buildIssueTracking())

    const requestMock = vi.fn().mockResolvedValue({
      data: {
        total_count: 2,
        items: [
          {
            number: 10,
            title: 'Fix bug',
            state: 'open',
            labels: [
              {
                id: 99,
                name: 'bug',
              },
            ],
          },
          {
            number: 11,
            title: 'PR item',
            state: 'open',
            pull_request: {
              url: 'https://example.com',
            },
          },
        ],
      },
    })

    setOctokitRequestMock(tracker, requestMock)

    const response = await tracker.listIssues({
      q: 'is:open',
      page: 1,
      limit: 20,
    })

    expect(response.totalCount).toBe(2)
    expect(response.items).toEqual([
      {
        id: '10',
        key: '#10',
        summary: 'Fix bug',
        statusId: 'open',
        labels: [ 'bug' ],
      },
    ])
  })
})
