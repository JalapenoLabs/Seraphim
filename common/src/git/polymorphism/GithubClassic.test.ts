// Copyright © 2026 Jalapeno Labs

import type {
  GitListBranchesOptions,
  GitCreatePullRequestOptions,
  GitPullRequestLocator,
} from '../types'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { GithubClassic } from './GithubClassic'

class TestableGithubClassic extends GithubClassic {
  public async listBranchesForTest(options: GitListBranchesOptions) {
    return this.listBranches(options)
  }

  public async createPullRequestForTest(options: GitCreatePullRequestOptions) {
    return this.createPullRequest(options)
  }

  public async getPullRequestInfoForTest(locator: GitPullRequestLocator) {
    return this.getPullRequestInfo(locator)
  }

  public async getPullRequestCiStatusForTest(locator: GitPullRequestLocator) {
    return this.getPullRequestCiStatus(locator)
  }
}

function hasRequiredEnvValues() {
  return Boolean(
    process.env.VITEST_GITHUB_CLASSIC_TOKEN
    && process.env.VITEST_GITHUB_REPO_URL,
  )
}

function hasPullRequestReadEnvValues() {
  return Boolean(
    hasRequiredEnvValues()
    && process.env.VITEST_GITHUB_PULL_REQUEST_NUMBER,
  )
}

function hasPullRequestCreateEnvValues() {
  return Boolean(
    hasRequiredEnvValues()
    && process.env.VITEST_GITHUB_PULL_REQUEST_SOURCE_BRANCH
    && process.env.VITEST_GITHUB_PULL_REQUEST_TARGET_BRANCH,
  )
}

function createGithubClassicClient() {
  return new TestableGithubClassic(process.env.VITEST_GITHUB_CLASSIC_TOKEN || '')
}

describe('GithubClassic', () => {
  const invalidEnvironment = !hasRequiredEnvValues()
  const invalidPullRequestReadEnvironment = !hasPullRequestReadEnvValues()
  const invalidPullRequestCreateEnvironment = !hasPullRequestCreateEnvValues()

  it('throws a friendly error when token is missing', () => {
    expect(() => new TestableGithubClassic('')).toThrow('Token is missing or empty')
  })

  it.skipIf(invalidEnvironment)('validateToken accepts classic credentials', async () => {
    const client = createGithubClassicClient()
    const validation = await client.validateToken()

    expect(validation.isValid, validation.message).toBe(true)
    expect(validation.type).toBe('Github classic')
  })

  it.skipIf(invalidEnvironment)('listBranches supports query filtering', async () => {
    const client = createGithubClassicClient()

    const allBranches = await client.listBranchesForTest({
      repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
      q: '',
      page: 1,
      limit: 25,
    })

    expect(allBranches).not.toBeNull()
    if (!allBranches) {
      return
    }

    const preferredQuery = allBranches.defaultBranch || allBranches.branches[0]?.name || ''
    const query = preferredQuery.slice(0, 4)

    const filteredBranches = await client.listBranchesForTest({
      repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
      q: query,
      page: 1,
      limit: 25,
    })

    expect(filteredBranches).not.toBeNull()
    if (!filteredBranches) {
      return
    }

    for (const branch of filteredBranches.branches) {
      expect(branch.name.toLowerCase()).toContain(query.toLowerCase())
    }
  })

  it.skipIf(invalidEnvironment)('listBranches can enumerate all branch pages', async () => {
    const client = createGithubClassicClient()

    const firstPage = await client.listBranchesForTest({
      repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
      q: '',
      page: 1,
      limit: 25,
    })

    expect(firstPage).not.toBeNull()
    if (!firstPage) {
      return
    }

    const pageCount = Math.ceil(firstPage.totalCount / firstPage.limit)
    const branchNames = new Set(firstPage.branches.map((branch) => branch.name))

    for (let pageNumber = 2; pageNumber <= pageCount; pageNumber += 1) {
      const pageResponse = await client.listBranchesForTest({
        repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
        q: '',
        page: pageNumber,
        limit: firstPage.limit,
      })

      expect(pageResponse).not.toBeNull()
      if (!pageResponse) {
        return
      }

      for (const branch of pageResponse.branches) {
        branchNames.add(branch.name)
      }
    }

    expect(branchNames.size).toBe(firstPage.totalCount)
  })

  it.skipIf(invalidPullRequestReadEnvironment)('getPullRequestInfo returns pull request details', async () => {
    const client = createGithubClassicClient()

    const pullRequestNumber = Number(process.env.VITEST_GITHUB_PULL_REQUEST_NUMBER)
    const pullRequestInfo = await client.getPullRequestInfoForTest({
      repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
      pullRequestNumber,
    })

    expect(pullRequestInfo).not.toBeNull()
    if (!pullRequestInfo) {
      return
    }

    expect(pullRequestInfo.pullRequestNumber).toBe(pullRequestNumber)
    expect(pullRequestInfo.title.length).toBeGreaterThan(0)
    expect([ 'open', 'draft', 'merged', 'closed' ]).toContain(pullRequestInfo.lifecycle)
  })

  it.skipIf(invalidPullRequestReadEnvironment)('getPullRequestCiStatus returns CI status state', async () => {
    const client = createGithubClassicClient()

    const pullRequestNumber = Number(process.env.VITEST_GITHUB_PULL_REQUEST_NUMBER)
    const pullRequestCiStatus = await client.getPullRequestCiStatusForTest({
      repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
      pullRequestNumber,
    })

    expect(pullRequestCiStatus).not.toBeNull()
    if (!pullRequestCiStatus) {
      return
    }

    expect([ 'pending', 'success', 'failure' ]).toContain(pullRequestCiStatus.status)
    expect(pullRequestCiStatus.sourceSha.length).toBeGreaterThan(0)
  })

  it.skipIf(invalidPullRequestCreateEnvironment)(
    'createPullRequest returns a url for the new pull request',
    async () => {
      const client = createGithubClassicClient()

      const createdPullRequest = await client.createPullRequestForTest({
        repoPath: process.env.VITEST_GITHUB_REPO_URL || '',
        title: process.env.VITEST_GITHUB_PULL_REQUEST_TITLE || `Test PR ${Date.now()}`,
        description: process.env.VITEST_GITHUB_PULL_REQUEST_DESCRIPTION
          || 'Created from vitest integration test.',
        sourceBranch: process.env.VITEST_GITHUB_PULL_REQUEST_SOURCE_BRANCH || '',
        targetBranch: process.env.VITEST_GITHUB_PULL_REQUEST_TARGET_BRANCH || '',
        draft: true,
        maintainersCanModify: true,
      })

      expect(createdPullRequest).not.toBeNull()
      if (!createdPullRequest) {
        return
      }

      expect(createdPullRequest.pullRequestNumber).toBeGreaterThan(0)
      expect(createdPullRequest.url).toContain('/pull/')
    },
  )
})
