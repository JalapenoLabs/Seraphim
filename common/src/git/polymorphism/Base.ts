// Copyright © 2026 Jalapeno Labs

import type {
  RequestResult,
  GitTokenValidation,
  GitListBranchesOptions,
  GitListBranchesResult,
  GitCreatePullRequestOptions,
  GitCreatePullRequestResult,
  GitPullRequestLocator,
  GitPullRequestInfo,
  GitPullRequestLifecycle,
  GitPullRequestCiStatus,
} from '../types'
import type { GithubBranchSummary } from '@common/types'
import type {
  GithubCommitStatusPayload,
  GithubPullRequestPayload,
  GithubRepoPayload,
} from '../schema'

// Lib
import { Octokit } from '@octokit/core'
import { RequestError } from '@octokit/request-error'

// Utility
import { Cloner } from '@common/cloning/polymorphism/cloner'
import {
  githubBranchSchema,
  githubCommitStatusSchema,
  githubPullRequestSchema,
  githubRepoSchema,
} from '../schema'

const GITHUB_BRANCH_PAGE_SIZE = 100

type RepoDetails = {
  defaultBranch: string | null
}

type RepoBranchPage = {
  branches: GithubBranchSummary[]
  hasNextPage: boolean
}

type ParsedRepositoryDetails = {
  owner: string
  repo: string
}

export class BaseGit {
  protected readonly token: string
  protected readonly octokit: Octokit

  // ////////////////////////////// //
  //              Core              //
  // ////////////////////////////// //

  constructor(token: string) {
    this.token = token.trim()
    if (!this.token) {
      throw new Error('BaseGit: Token is missing or empty.')
    }

    this.octokit = new Octokit({
      auth: this.token,
    })
  }

  public async validateToken(): Promise<GitTokenValidation> {
    throw new Error('validateToken method not implemented for the base Git type.')
  }

  protected async doRequest<Shape>(command: string): Promise<RequestResult<Shape>> {
    try {
      const response = await this.octokit.request(command, {
        headers: {
          'X-GitHub-Api-Version': '2022-11-28',
        },
      })

      return [ '', response ] as const
    }
    catch (error) {
      if (error instanceof RequestError) {
        if (error.status === 401) {
          return [ 'Bad credentials: GitHub token is invalid', null ] as const
        }
      }

      console.debug('GitHub token validation request failed', { error })

      return [ 'GitHub token failed validation', null ] as const
    }
  }

  protected headerToSet(headerValue: string | number | null): Set<string> {
    if (!headerValue) {
      return new Set()
    }

    const headerString = String(headerValue)
    const items = headerString
      .split(',')
      .map((item) => item.trim())
      .filter((item) => item.length > 0)

    return new Set(items)
  }

  protected resolveRepositoryDetails(repoPath: string): ParsedRepositoryDetails | null {
    const cloner = new Cloner(repoPath)
    const repositoryDetails = cloner.getParsedRepositoryDetails()
    if (!repositoryDetails) {
      console.debug('GitHub operation failed because repository path could not be parsed', {
        repoPath,
      })
      return null
    }

    return repositoryDetails
  }

  // ////////////////////////////// //
  //        Listing branches        //
  // ////////////////////////////// //

  public async listBranches(
    options: GitListBranchesOptions,
  ): Promise<GitListBranchesResult | null> {
    const repo = this.resolveRepositoryDetails(options.repoPath)
    if (!repo) {
      return null
    }

    const [ branches, repoInfo ] = await Promise.all([
      this.fetchAllRepoBranches(repo.owner, repo.repo),
      this.fetchRepoDetails(repo.owner, repo.repo),
    ])

    if (!branches) {
      return null
    }

    const sortedBranches = this.sortBranches(
      branches,
      repoInfo?.defaultBranch ?? null,
      options.q,
    )
    const paginatedBranches = this.paginateBranches(
      sortedBranches,
      options.page,
      options.limit,
    )

    return {
      branches: paginatedBranches,
      defaultBranch: repoInfo?.defaultBranch ?? null,
      totalCount: sortedBranches.length,
      page: options.page,
      limit: options.limit,
    }
  }

  protected getBranchPriority(branchName: string, defaultBranch: string | null) {
    const normalizedBranchName = branchName.toLowerCase()

    if (defaultBranch && normalizedBranchName === defaultBranch.toLowerCase()) {
      return 0
    }

    if (normalizedBranchName === 'main') {
      return 1
    }

    if (normalizedBranchName === 'master') {
      return 2
    }

    if (normalizedBranchName === 'develop' || normalizedBranchName === 'devel') {
      return 3
    }

    if (normalizedBranchName === 'dev') {
      return 4
    }

    if (normalizedBranchName === 'staging') {
      return 5
    }

    if (normalizedBranchName === 'production' || normalizedBranchName === 'prod') {
      return 6
    }

    if (normalizedBranchName.startsWith('release/')) {
      return 7
    }

    if (normalizedBranchName.startsWith('hotfix/')) {
      return 8
    }

    return 100
  }

  protected sortBranches(
    branches: GithubBranchSummary[],
    defaultBranch: string | null,
    searchQueryRaw?: string | null,
  ) {
    const searchQuery = searchQueryRaw?.trim()?.toLowerCase()

    const matchedBranches = branches.filter((branch) => {
      if (!searchQuery) {
        return true
      }

      return branch.name.toLowerCase().includes(searchQuery)
    })

    return matchedBranches.sort((firstBranch, secondBranch) => {
      const firstPriority = this.getBranchPriority(firstBranch.name, defaultBranch)
      const secondPriority = this.getBranchPriority(secondBranch.name, defaultBranch)

      if (firstPriority !== secondPriority) {
        return firstPriority - secondPriority
      }

      return firstBranch.name.localeCompare(secondBranch.name)
    })
  }

  protected paginateBranches(
    branches: GithubBranchSummary[],
    page: number,
    limit: number,
  ) {
    const startIndex = (page - 1) * limit
    const endIndex = startIndex + limit

    return branches.slice(startIndex, endIndex)
  }

  // ////////////////////////////// //
  //         Pull requests          //
  // ////////////////////////////// //

  public async createPullRequest(
    options: GitCreatePullRequestOptions,
  ): Promise<GitCreatePullRequestResult | null> {
    const repo = this.resolveRepositoryDetails(options.repoPath)
    if (!repo) {
      return null
    }

    const title = options.title.trim()
    const sourceBranch = options.sourceBranch.trim()
    const targetBranch = options.targetBranch.trim()

    if (!title || !sourceBranch || !targetBranch) {
      console.debug('Create pull request failed because required fields are missing', {
        title,
        sourceBranch,
        targetBranch,
      })
      return null
    }

    try {
      const response = await this.octokit.request('POST /repos/{owner}/{repo}/pulls', {
        owner: repo.owner,
        repo: repo.repo,
        title,
        head: sourceBranch,
        base: targetBranch,
        body: options.description?.trim(),
        draft: options.draft ?? false,
        maintainer_can_modify: options.maintainersCanModify ?? true,
      })

      const parsed = githubPullRequestSchema.safeParse(response.data)
      if (!parsed.success) {
        console.debug('GitHub create pull request payload failed validation', {
          owner: repo.owner,
          repo: repo.repo,
          error: parsed.error,
        })
        return null
      }

      return {
        pullRequestNumber: parsed.data.number,
        url: parsed.data.html_url,
      }
    }
    catch (error) {
      console.debug('Create pull request failed', {
        owner: repo.owner,
        repo: repo.repo,
        error,
      })
      return null
    }
  }

  public async getPullRequestInfo(
    locator: GitPullRequestLocator,
  ): Promise<GitPullRequestInfo | null> {
    const pullRequestPayload = await this.fetchPullRequestPayload(locator)
    if (!pullRequestPayload) {
      return null
    }

    const lifecycle = this.resolvePullRequestLifecycle(pullRequestPayload)

    return {
      pullRequestNumber: pullRequestPayload.number,
      title: pullRequestPayload.title,
      description: pullRequestPayload.body,
      state: pullRequestPayload.state,
      lifecycle,
      isDraft: pullRequestPayload.draft,
      isMerged: Boolean(pullRequestPayload.merged_at),
      url: pullRequestPayload.html_url,
      authorUsername: pullRequestPayload.user?.login ?? null,
      authorUrl: pullRequestPayload.user?.html_url ?? null,
      sourceBranch: pullRequestPayload.head.ref,
      targetBranch: pullRequestPayload.base.ref,
      sourceSha: pullRequestPayload.head.sha,
      createdAt: pullRequestPayload.created_at,
      updatedAt: pullRequestPayload.updated_at,
      closedAt: pullRequestPayload.closed_at,
      mergedAt: pullRequestPayload.merged_at,
    }
  }

  public async getPullRequestCiStatus(
    locator: GitPullRequestLocator,
  ): Promise<GitPullRequestCiStatus | null> {
    const repo = this.resolveRepositoryDetails(locator.repoPath)
    if (!repo) {
      return null
    }

    const pullRequestInfo = await this.getPullRequestInfo(locator)
    if (!pullRequestInfo) {
      return null
    }

    try {
      const response = await this.octokit.request('GET /repos/{owner}/{repo}/commits/{ref}/status', {
        owner: repo.owner,
        repo: repo.repo,
        ref: pullRequestInfo.sourceSha,
      })

      const parsed = githubCommitStatusSchema.safeParse(response.data)
      if (!parsed.success) {
        console.debug('GitHub commit status payload failed validation', {
          owner: repo.owner,
          repo: repo.repo,
          ref: pullRequestInfo.sourceSha,
          error: parsed.error,
        })
        return null
      }

      return {
        status: this.mapCommitStateToCiStatus(parsed.data.state),
        sourceSha: pullRequestInfo.sourceSha,
      }
    }
    catch (error) {
      console.debug('Get pull request CI status failed', {
        owner: repo.owner,
        repo: repo.repo,
        pullRequestNumber: locator.pullRequestNumber,
        error,
      })
      return null
    }
  }

  protected resolvePullRequestLifecycle(
    pullRequest: GithubPullRequestPayload,
  ): GitPullRequestLifecycle {
    if (pullRequest.merged_at) {
      return 'merged'
    }

    if (pullRequest.state === 'closed') {
      return 'closed'
    }

    if (pullRequest.draft) {
      return 'draft'
    }

    return 'open'
  }

  protected mapCommitStateToCiStatus(
    state: GithubCommitStatusPayload['state'],
  ): GitPullRequestCiStatus['status'] {
    if (state === 'success') {
      return 'success'
    }

    if (state === 'pending') {
      return 'pending'
    }

    return 'failure'
  }

  protected async fetchPullRequestPayload(
    locator: GitPullRequestLocator,
  ): Promise<GithubPullRequestPayload | null> {
    const repo = this.resolveRepositoryDetails(locator.repoPath)
    if (!repo) {
      return null
    }

    if (!Number.isInteger(locator.pullRequestNumber) || locator.pullRequestNumber <= 0) {
      console.debug('Get pull request failed because pull request number is invalid', {
        pullRequestNumber: locator.pullRequestNumber,
      })
      return null
    }

    try {
      const response = await this.octokit.request('GET /repos/{owner}/{repo}/pulls/{pull_number}', {
        owner: repo.owner,
        repo: repo.repo,
        pull_number: locator.pullRequestNumber,
      })

      const parsed = githubPullRequestSchema.safeParse(response.data)
      if (!parsed.success) {
        console.debug('GitHub pull request payload failed validation', {
          owner: repo.owner,
          repo: repo.repo,
          pullRequestNumber: locator.pullRequestNumber,
          error: parsed.error,
        })
        return null
      }

      return parsed.data
    }
    catch (error) {
      console.debug('Get pull request failed', {
        owner: repo.owner,
        repo: repo.repo,
        pullRequestNumber: locator.pullRequestNumber,
        error,
      })
      return null
    }
  }

  protected async fetchRepoDetails(
    owner: string,
    repo: string,
  ): Promise<RepoDetails | null> {
    try {
      const response = await this.octokit.request('GET /repos/{owner}/{repo}', {
        owner,
        repo,
      })

      const parsed = githubRepoSchema.safeParse(response.data)
      if (!parsed.success) {
        console.debug('Github repo payload failed validation', parsed.error)
        return null
      }

      return {
        defaultBranch: (parsed.data as GithubRepoPayload)?.default_branch ?? null,
      }
    }
    catch (error) {
      console.debug('Failed to fetch Github repo details', { owner, repo, error })
      return null
    }
  }

  protected async fetchRepoBranchPage(
    owner: string,
    repo: string,
    page: number,
  ): Promise<RepoBranchPage | null> {
    try {
      const response = await this.octokit.request('GET /repos/{owner}/{repo}/branches', {
        owner,
        repo,
        page,
        per_page: GITHUB_BRANCH_PAGE_SIZE,
      })

      const payloadList = Array.isArray(response.data)
        ? response.data
        : []

      const branches: GithubBranchSummary[] = []
      payloadList.forEach((payload) => {
        const parsed = githubBranchSchema.safeParse(payload)
        if (!parsed.success) {
          console.debug('Github branch payload failed validation', parsed.error)
          return
        }

        const branch: GithubBranchSummary = {
          name: parsed.data.name,
          sha: parsed.data.commit.sha,
          isProtected: parsed.data.protected,
        }

        branches.push(branch)
      })

      const linkHeader = response.headers.link
      const hasNextPage = typeof linkHeader === 'string'
        && linkHeader.includes('rel="next"')

      return {
        branches,
        hasNextPage,
      }
    }
    catch (error) {
      console.debug('Failed to fetch Github branches', { owner, repo, page, error })
      return null
    }
  }

  protected async fetchAllRepoBranches(
    owner: string,
    repo: string,
  ): Promise<GithubBranchSummary[] | null> {
    const allBranches: GithubBranchSummary[] = []
    let page = 1

    while (true) {
      const pageResponse = await this.fetchRepoBranchPage(owner, repo, page)

      if (!pageResponse) {
        console.debug('Failed to fetch branch page while collecting all repository branches', {
          owner,
          repo,
          page,
        })
        return null
      }

      allBranches.push(...pageResponse.branches)

      if (!pageResponse.hasNextPage) {
        return allBranches
      }

      page += 1
    }
  }
}
