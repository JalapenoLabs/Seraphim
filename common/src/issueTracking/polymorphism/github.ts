// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@prisma/client'
import type {
  IssueTrackingIssue,
  IssueTrackingIssueList,
  IssueTrackingIssueUpdate,
  IssueTrackingLabel,
  IssueTrackingListIssuesParams,
  IssueTrackingStatusType,
} from '../types'

// Core
import { IssueTracker } from './issueTracker'
import { Octokit } from '@octokit/core'
import { RequestError } from '@octokit/request-error'

export class GithubIssueTracker extends IssueTracker {
  private readonly octokit: Octokit

  constructor(issueTracking: IssueTracking) {
    super(issueTracking)

    this.octokit = new Octokit({
      auth: this.issueTracking.accessToken,
      baseUrl: IssueTracker.resolveIssueTrackingBaseUrl(
        this.issueTracking.baseUrl,
        this.issueTracking.provider,
      ),
    })
  }

  public async check(): Promise<[ boolean, string ]> {
    if (!this.issueTracking.accessToken?.trim()) {
      console.debug('GitHub issue tracker check failed because access token is missing', {
        issueTrackingId: this.issueTracking.id,
      })
      return [ false, 'GitHub access token is required' ]
    }

    const owner = this.getOwner()
    if (!owner) {
      return [ false, 'GitHub organization or owner is required' ]
    }

    const repo = this.getRepo()
    if (!repo) {
      return [ false, 'GitHub repository is required' ]
    }

    try {
      await this.octokit.request('GET /repos/{owner}/{repo}', {
        owner,
        repo,
        headers: {
          'X-GitHub-Api-Version': '2022-11-28',
        },
      })

      return [ true, '' ]
    }
    catch (error) {
      return this.resolveGithubError(error, 'Unable to validate GitHub repository access')
    }
  }

  public async listIssues(
    params: IssueTrackingListIssuesParams = {},
  ): Promise<IssueTrackingIssueList> {
    const owner = this.getOwner()
    const repo = this.getRepo()
    const page = params.page ?? 1
    const limit = params.limit ?? 50

    if (!owner || !repo) {
      console.debug('GitHub listIssues cannot run without owner and repo', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
      })
      return super.listIssues(params)
    }

    try {
      const searchQuery = this.buildSearchQuery(owner, repo, params.q)

      const response = await this.octokit.request('GET /search/issues', {
        q: searchQuery,
        page,
        per_page: limit,
        headers: {
          'X-GitHub-Api-Version': '2022-11-28',
        },
      })

      const issues = this.mapIssueListPayload(response.data.items)
      const totalCount = Number.isFinite(response.data.total_count)
        ? Number(response.data.total_count)
        : issues.length

      return {
        items: issues,
        page,
        limit,
        totalCount,
      }
    }
    catch (error) {
      console.debug('GitHub listIssues failed', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
        search: params.q ?? null,
        page,
        limit,
        error,
      })
      return super.listIssues(params)
    }
  }

  public async getIssueById(issueId: string): Promise<IssueTrackingIssue | null> {
    if (!issueId?.trim()) {
      console.debug('GitHub getIssueById received invalid issueId', {
        issueId,
      })
      return super.getIssueById(issueId)
    }

    const owner = this.getOwner()
    const repo = this.getRepo()

    if (!owner || !repo) {
      console.debug('GitHub getIssueById cannot run without owner and repo', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
      })
      return super.getIssueById(issueId)
    }

    const issueNumber = Number(issueId)
    if (!Number.isInteger(issueNumber) || issueNumber <= 0) {
      console.debug('GitHub getIssueById received non-numeric issue id', {
        issueTrackingId: this.issueTracking.id,
        issueId,
      })
      return super.getIssueById(issueId)
    }

    try {
      const response = await this.octokit.request('GET /repos/{owner}/{repo}/issues/{issue_number}', {
        owner,
        repo,
        issue_number: issueNumber,
        headers: {
          'X-GitHub-Api-Version': '2022-11-28',
        },
      })

      return this.mapIssuePayload(response.data)
    }
    catch (error) {
      console.debug('GitHub getIssueById failed', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
        issueId,
        error,
      })

      return super.getIssueById(issueId)
    }
  }

  public async listLabels(): Promise<IssueTrackingLabel[]> {
    const owner = this.getOwner()
    const repo = this.getRepo()

    if (!owner || !repo) {
      console.debug('GitHub listLabels cannot run without owner and repo', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
      })
      return super.listLabels()
    }

    try {
      const response = await this.octokit.request('GET /repos/{owner}/{repo}/labels', {
        owner,
        repo,
        per_page: 100,
        headers: {
          'X-GitHub-Api-Version': '2022-11-28',
        },
      })

      return this.mapLabelsPayload(response.data)
    }
    catch (error) {
      console.debug('GitHub listLabels failed', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
        error,
      })
      return super.listLabels()
    }
  }

  public async listStatusTypes(): Promise<IssueTrackingStatusType[]> {
    return [
      {
        id: 'open',
        name: 'Open',
        category: 'To Do',
      },
      {
        id: 'closed',
        name: 'Closed',
        category: 'Done',
      },
    ]
  }

  public async updateIssueById(
    issueId: string,
    update: IssueTrackingIssueUpdate,
  ): Promise<IssueTrackingIssue | null> {
    if (!issueId?.trim()) {
      console.debug('GitHub updateIssueById received invalid issueId', {
        issueId,
      })
      return super.updateIssueById(issueId, update)
    }

    if (!update || Object.keys(update).length === 0) {
      console.debug('GitHub updateIssueById received empty update payload', {
        issueId,
        update,
      })
      return super.updateIssueById(issueId, update)
    }

    const owner = this.getOwner()
    const repo = this.getRepo()
    if (!owner || !repo) {
      console.debug('GitHub updateIssueById cannot run without owner and repo', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
      })
      return super.updateIssueById(issueId, update)
    }

    const issueNumber = Number(issueId)
    if (!Number.isInteger(issueNumber) || issueNumber <= 0) {
      console.debug('GitHub updateIssueById received non-numeric issue id', {
        issueTrackingId: this.issueTracking.id,
        issueId,
      })
      return super.updateIssueById(issueId, update)
    }

    const requestBody = this.buildUpdateRequest(update)
    if (!requestBody) {
      console.debug('GitHub updateIssueById did not receive any supported update fields', {
        issueTrackingId: this.issueTracking.id,
        issueId,
        update,
      })
      return super.updateIssueById(issueId, update)
    }

    try {
      const response = await this.octokit.request('PATCH /repos/{owner}/{repo}/issues/{issue_number}', {
        owner,
        repo,
        issue_number: issueNumber,
        ...requestBody,
        headers: {
          'X-GitHub-Api-Version': '2022-11-28',
        },
      })

      return this.mapIssuePayload(response.data)
    }
    catch (error) {
      console.debug('GitHub updateIssueById failed', {
        issueTrackingId: this.issueTracking.id,
        owner,
        repo,
        issueId,
        update,
        error,
      })

      return super.updateIssueById(issueId, update)
    }
  }

  private getOwner(): string {
    const owner = this.issueTracking.email?.trim() || ''
    if (!owner) {
      console.debug('GitHub issue tracker owner is missing', {
        issueTrackingId: this.issueTracking.id,
      })
    }
    return owner
  }

  private getRepo(): string {
    const repo = this.issueTracking.targetBoard?.trim() || ''
    if (!repo) {
      console.debug('GitHub issue tracker repository is missing', {
        issueTrackingId: this.issueTracking.id,
      })
    }
    return repo
  }

  private resolveGithubError(
    error: unknown,
    fallbackMessage: string,
  ): [ boolean, string ] {
    if (error instanceof RequestError) {
      if (error.status === 401) {
        return [ false, 'GitHub authentication failed. Check the access token.' ]
      }

      if (error.status === 403) {
        return [ false, 'GitHub access denied to this repository.' ]
      }

      if (error.status === 404) {
        return [ false, 'GitHub repository was not found for the provided org/repo.' ]
      }
    }

    console.debug('GitHub issue tracker request failed', {
      issueTrackingId: this.issueTracking.id,
      error,
    })

    return [ false, fallbackMessage ]
  }

  private mapIssueListPayload(payload: unknown): IssueTrackingIssue[] {
    if (!Array.isArray(payload)) {
      console.debug('GitHub listIssues payload was not an array', {
        issueTrackingId: this.issueTracking.id,
        payload,
      })
      return []
    }

    const issues: IssueTrackingIssue[] = []

    for (const issuePayload of payload) {
      const issue = this.mapIssuePayload(issuePayload)
      if (!issue) {
        continue
      }

      issues.push(issue)
    }

    return issues
  }

  private mapIssuePayload(payload: unknown): IssueTrackingIssue | null {
    if (!payload || typeof payload !== 'object') {
      console.debug('GitHub issue payload was not an object', {
        issueTrackingId: this.issueTracking.id,
        payload,
      })
      return null
    }

    if (!this.isRecord(payload)) {
      console.debug('GitHub issue payload could not be parsed as object record', {
        issueTrackingId: this.issueTracking.id,
        payload,
      })
      return null
    }

    const issueRecord = payload

    if (issueRecord.pull_request && typeof issueRecord.pull_request === 'object') {
      return null
    }

    const issueNumber = issueRecord.number
    const issueTitle = issueRecord.title
    const issueState = issueRecord.state

    if (!Number.isFinite(issueNumber) || typeof issueTitle !== 'string' || typeof issueState !== 'string') {
      console.debug('GitHub issue payload missing required fields', {
        issueTrackingId: this.issueTracking.id,
        issueNumber,
        issueTitle,
        issueState,
      })
      return null
    }

    const issueLabels = this.resolveLabelNames(issueRecord.labels)

    return {
      id: String(issueNumber),
      key: `#${issueNumber}`,
      summary: issueTitle,
      statusId: issueState,
      labels: issueLabels,
    }
  }

  private resolveLabelNames(payload: unknown): string[] {
    if (!Array.isArray(payload)) {
      return []
    }

    const labelNames: string[] = []

    for (const label of payload) {
      if (!label || typeof label !== 'object') {
        continue
      }

      if (!this.isRecord(label)) {
        continue
      }

      const labelName = label.name
      if (typeof labelName !== 'string' || !labelName.trim()) {
        continue
      }

      labelNames.push(labelName)
    }

    return labelNames
  }

  private mapLabelsPayload(payload: unknown): IssueTrackingLabel[] {
    if (!Array.isArray(payload)) {
      console.debug('GitHub listLabels payload was not an array', {
        issueTrackingId: this.issueTracking.id,
        payload,
      })
      return []
    }

    const labels: IssueTrackingLabel[] = []

    for (const labelPayload of payload) {
      if (!labelPayload || typeof labelPayload !== 'object') {
        continue
      }

      if (!this.isRecord(labelPayload)) {
        continue
      }

      const labelRecord = labelPayload
      const labelName = labelRecord.name
      const labelId = labelRecord.id

      if (typeof labelName !== 'string' || !labelName.trim()) {
        continue
      }

      const resolvedLabelId = Number.isFinite(labelId)
        ? String(labelId)
        : labelName

      labels.push({
        id: resolvedLabelId,
        name: labelName,
      })
    }

    return labels
  }

  private buildSearchQuery(owner: string, repo: string, search?: string): string {
    const trimmedSearch = search?.trim() || ''
    const baseQuery = `repo:${owner}/${repo} is:issue`

    if (!trimmedSearch) {
      return baseQuery
    }

    return `${baseQuery} ${trimmedSearch}`
  }

  private buildUpdateRequest(update: IssueTrackingIssueUpdate) {
    const requestBody: {
      title?: string
      body?: string
      state?: 'open' | 'closed'
      labels?: string[]
      assignee?: string
      assignees?: string[]
    } = {}

    if (typeof update.summary === 'string') {
      requestBody.title = update.summary
    }

    if (typeof update.description === 'string') {
      requestBody.body = update.description
    }

    if (typeof update.statusId === 'string') {
      const status = update.statusId.toLowerCase().trim()
      if (status === 'open' || status === 'closed') {
        requestBody.state = status
      }
      else {
        console.debug('GitHub updateIssueById ignored unsupported statusId', {
          issueTrackingId: this.issueTracking.id,
          statusId: update.statusId,
        })
      }
    }

    if (Array.isArray(update.labels)) {
      requestBody.labels = update.labels
    }

    if (typeof update.assigneeId === 'string' && update.assigneeId.trim()) {
      requestBody.assignee = update.assigneeId
      requestBody.assignees = [ update.assigneeId ]
    }

    if (update.assigneeId === null) {
      requestBody.assignees = []
      requestBody.assignee = ''
    }

    if (Object.keys(requestBody).length === 0) {
      return null
    }

    return requestBody
  }

  private isRecord(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object'
  }
}
