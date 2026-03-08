// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@prisma/client'
import type {
  IssueTrackingIssue,
  IssueTrackingIssueList,
  IssueTrackingIssueUpdate,
  IssueTrackingLabel,
  IssueTrackingListIssuesParams,
  IssueTrackingSearchMode,
  IssueTrackingStatusType,
} from '../types'

// Core
import { IssueTracker } from './issueTracker'
import { AgileClient, HttpException, Version3Client } from 'jira.js'

export class JiraIssueTracker extends IssueTracker {
  private readonly client: Version3Client
  private readonly agileClient: AgileClient

  constructor(issueTracking: IssueTracking) {
    super(issueTracking)

    const clientConfig = {
      host: IssueTracker.resolveIssueTrackingBaseUrl(
        this.issueTracking.baseUrl,
        this.issueTracking.provider,
      ),
      authentication: {
        basic: {
          email: this.issueTracking.email,
          apiToken: this.issueTracking.accessToken,
        },
      },
    }

    this.client = new Version3Client(clientConfig)
    this.agileClient = new AgileClient(clientConfig)
  }

  public async check(): Promise<[ boolean, string ]> {
    if (!this.issueTracking.email?.trim()) {
      console.debug('Jira check failed because email is missing', {
        issueTrackingId: this.issueTracking.id,
      })
      return [ false, 'Jira email is required' ]
    }

    if (!this.issueTracking.accessToken?.trim()) {
      console.debug('Jira check failed because access token is missing', {
        issueTrackingId: this.issueTracking.id,
      })
      return [ false, 'Jira access token is required' ]
    }

    if (!this.issueTracking.targetBoard?.trim()) {
      console.debug('Jira check failed because target board is missing', {
        issueTrackingId: this.issueTracking.id,
      })
      return [ false, 'Jira target board is required' ]
    }

    try {
      await this.client.myself.getCurrentUser()
    }
    catch (error) {
      if (error instanceof HttpException && error.status === 401) {
        const errorMessage = 'Jira authentication failed. Check the email and access token.'

        console.debug('Jira issue tracking check failed due to authentication', {
          issueTrackingId: this.issueTracking.id,
        })
        return [ false, errorMessage ]
      }

      const errorMessage = error instanceof Error
        ? error.message
        : 'Unable to validate Jira credentials'

      console.debug('Jira issue tracking check failed', {
        issueTrackingId: this.issueTracking.id,
        error,
      })
      return [ false, errorMessage ]
    }

    const [ isBoardValid, boardError ] = await this.validateTargetBoard()
    if (!isBoardValid) {
      return [ false, boardError ]
    }

    return [ true, '' ]
  }

  public async listIssues(
    params: IssueTrackingListIssuesParams = {},
  ): Promise<IssueTrackingIssueList> {
    const page = params.page ?? 1
    const limit = params.limit ?? 50
    const searchMode = params.mode ?? 'text'

    if (!this.issueTracking.targetBoard?.trim()) {
      console.debug('Jira listIssues cannot run without targetBoard', {
        issueTrackingId: this.issueTracking.id,
      })
      return super.listIssues(params)
    }

    const startAt = (page - 1) * limit
    const searchJql = this.buildSearchJql(params.q, searchMode)

    if (this.isNumericBoardId(this.issueTracking.targetBoard)) {
      const boardId = Number(this.issueTracking.targetBoard)

      try {
        const response = await this.agileClient.board.getIssuesForBoard({
          boardId,
          startAt,
          maxResults: limit,
          jql: searchJql || undefined,
          fields: [ 'summary', 'status', 'labels' ],
        })

        return this.mapIssueListResponse(response, page, limit)
      }
      catch (error) {
        console.debug('Jira listIssues failed for board id search', {
          issueTrackingId: this.issueTracking.id,
          boardId,
          search: params.q ?? null,
          page,
          limit,
          error,
        })
        return super.listIssues(params)
      }
    }

    const projectKey = this.issueTracking.targetBoard.trim()
    const jqlParts = [ `project = "${this.escapeJqlValue(projectKey)}"` ]
    if (searchJql) {
      jqlParts.push(`(${searchJql})`)
    }

    try {
      const response = await this.client.issueSearch.searchForIssuesUsingJql({
        jql: jqlParts.join(' AND '),
        startAt,
        maxResults: limit,
        fields: [ 'summary', 'status', 'labels' ],
      })

      return this.mapIssueListResponse(response, page, limit)
    }
    catch (error) {
      const statusCode = this.resolveHttpStatusCode(error)
      if (statusCode === 410) {
        console.debug('Jira listIssues legacy search endpoint is gone, retrying with enhanced search', {
          issueTrackingId: this.issueTracking.id,
          projectKey,
          search: params.q ?? null,
          page,
          limit,
        })

        return this.listIssuesViaEnhancedSearch({
          jql: jqlParts.join(' AND '),
          page,
          limit,
          projectKey,
          search: params.q ?? null,
        })
      }

      console.debug('Jira listIssues failed for project key search', {
        issueTrackingId: this.issueTracking.id,
        projectKey,
        search: params.q ?? null,
        page,
        limit,
        error,
      })
      return super.listIssues(params)
    }
  }

  public async getIssueById(issueId: string): Promise<IssueTrackingIssue | null> {
    console.debug('Jira getIssueById not implemented', {
      issueTrackingId: this.issueTracking.id,
      issueId,
    })
    return super.getIssueById(issueId)
  }

  public async listLabels(): Promise<IssueTrackingLabel[]> {
    console.debug('Jira listLabels not implemented', {
      issueTrackingId: this.issueTracking.id,
    })
    return super.listLabels()
  }

  public async listStatusTypes(): Promise<IssueTrackingStatusType[]> {
    console.debug('Jira listStatusTypes not implemented', {
      issueTrackingId: this.issueTracking.id,
    })
    return super.listStatusTypes()
  }

  public async updateIssueById(
    issueId: string,
    update: IssueTrackingIssueUpdate,
  ): Promise<IssueTrackingIssue | null> {
    console.debug('Jira updateIssueById not implemented', {
      issueTrackingId: this.issueTracking.id,
      issueId,
      update,
    })
    return super.updateIssueById(issueId, update)
  }

  private isNumericBoardId(value: string) {
    return /^\d+$/.test(value)
  }

  private buildSearchJql(
    searchQuery: string | undefined,
    searchMode: IssueTrackingSearchMode,
  ) {
    const normalizedSearchQuery = searchQuery?.trim()
    if (!normalizedSearchQuery) {
      return ''
    }

    if (searchMode === 'jql') {
      return normalizedSearchQuery
    }

    const escaped = this.escapeJqlValue(normalizedSearchQuery)
    return `text ~ "${escaped}"`
  }

  private escapeJqlValue(value: string) {
    return value
      .replace(/\\/g, '\\\\')
      .replace(/"/g, '\\"')
  }

  private mapIssueListResponse(
    payload: {
      issues?: unknown[]
      total?: number
    },
    page: number,
    limit: number,
  ): IssueTrackingIssueList {
    const payloadItems = Array.isArray(payload.issues)
      ? payload.issues
      : []

    const items: IssueTrackingIssue[] = []

    for (const payloadItem of payloadItems) {
      const issue = this.mapIssuePayload(payloadItem)
      if (!issue) {
        continue
      }

      items.push(issue)
    }

    return {
      items,
      page,
      limit,
      totalCount: Number.isFinite(payload.total)
        ? Number(payload.total)
        : items.length,
    }
  }

  private async listIssuesViaEnhancedSearch(params: {
    jql: string
    page: number
    limit: number
    projectKey: string
    search: string | null
  }): Promise<IssueTrackingIssueList> {
    try {
      const enhancedSearchResponse = await this.searchIssuesWithPaginationToken({
        jql: params.jql,
        page: params.page,
        limit: params.limit,
      })

      const totalCount = await this.countIssuesForJql(params.jql)

      return this.mapIssueListResponse({
        issues: enhancedSearchResponse.issues,
        total: totalCount ?? undefined,
      }, params.page, params.limit)
    }
    catch (error) {
      console.debug('Jira listIssues failed for enhanced project key search', {
        issueTrackingId: this.issueTracking.id,
        projectKey: params.projectKey,
        search: params.search,
        page: params.page,
        limit: params.limit,
        error,
      })

      return super.listIssues({
        q: params.search ?? undefined,
        page: params.page,
        limit: params.limit,
      })
    }
  }

  private async searchIssuesWithPaginationToken(params: {
    jql: string
    page: number
    limit: number
  }): Promise<{
    issues?: unknown[]
  }> {
    let nextPageToken: string | undefined

    for (let currentPage = 1; currentPage <= params.page; currentPage += 1) {
      const response = await this.client.issueSearch.searchForIssuesUsingJqlEnhancedSearch({
        jql: params.jql,
        nextPageToken,
        maxResults: params.limit,
        fields: [ 'summary', 'status', 'labels' ],
      })

      if (currentPage === params.page) {
        return response
      }

      if (!response.nextPageToken) {
        return { issues: []}
      }

      nextPageToken = response.nextPageToken
    }

    return { issues: []}
  }

  private async countIssuesForJql(jql: string): Promise<number | null> {
    try {
      const response = await this.client.issueSearch.countIssues({ jql })
      if (!Number.isFinite(response.count)) {
        return null
      }

      return Number(response.count)
    }
    catch (error) {
      console.debug('Jira listIssues failed to resolve issue count for enhanced search', {
        issueTrackingId: this.issueTracking.id,
        jql,
        error,
      })

      return null
    }
  }

  private resolveHttpStatusCode(error: unknown): number | null {
    if (error instanceof HttpException && Number.isFinite(error.status)) {
      return error.status
    }

    if (!error || typeof error !== 'object') {
      return null
    }

    const errorRecord = error as {
      status?: unknown
      response?: {
        status?: unknown
      }
      cause?: {
        response?: {
          status?: unknown
        }
      }
    }

    if (Number.isFinite(errorRecord.status)) {
      return Number(errorRecord.status)
    }

    if (Number.isFinite(errorRecord.response?.status)) {
      return Number(errorRecord.response?.status)
    }

    if (Number.isFinite(errorRecord.cause?.response?.status)) {
      return Number(errorRecord.cause?.response?.status)
    }

    return null
  }

  private mapIssuePayload(payload: unknown): IssueTrackingIssue | null {
    if (!payload || typeof payload !== 'object') {
      console.debug('Jira listIssues received non-object issue payload', { payload })
      return null
    }

    const issue = payload as {
      id?: string | number
      key?: string
      fields?: {
        summary?: string
        status?: {
          id?: string | number
        }
        labels?: unknown
      }
    }

    const issueId = String(issue.id || '').trim()
    const issueKey = issue.key?.trim()
    const issueSummary = issue.fields?.summary?.trim()
    const issueStatusId = String(issue.fields?.status?.id || '').trim()

    if (!issueId || !issueKey || !issueSummary || !issueStatusId) {
      console.debug('Jira listIssues payload missing expected issue fields', {
        issueTrackingId: this.issueTracking.id,
        issueId,
        issueKey,
        issueSummary,
        issueStatusId,
      })
      return null
    }

    const issueLabels = Array.isArray(issue.fields?.labels)
      ? issue.fields?.labels.filter((label): label is string => typeof label === 'string')
      : []

    return {
      id: issueId,
      key: issueKey,
      summary: issueSummary,
      statusId: issueStatusId,
      labels: issueLabels,
    }
  }

  private async validateTargetBoard(): Promise<[ boolean, string ]> {
    const targetBoard = this.issueTracking.targetBoard.trim()

    if (this.isNumericBoardId(targetBoard)) {
      return this.validateBoardById(Number(targetBoard))
    }

    return this.validateBoardByProjectKey(targetBoard)
  }

  private async validateBoardById(boardId: number): Promise<[ boolean, string ]> {
    try {
      await this.agileClient.board.getBoard({ boardId })
      return [ true, '' ]
    }
    catch (error) {
      const handled = this.handleBoardValidationError(error, {
        issueTrackingId: this.issueTracking.id,
        targetBoard: String(boardId),
        targetBoardType: 'board-id',
      })
      if (handled) {
        return handled
      }

      return [ false, 'Unable to validate Jira board ID' ]
    }
  }

  private async validateBoardByProjectKey(projectKey: string): Promise<[ boolean, string ]> {
    try {
      const response = await this.agileClient.board.getAllBoards({
        projectKeyOrId: projectKey,
        maxResults: 1,
      })

      if (!response.values?.length) {
        return [ false, 'Jira project key did not match any boards' ]
      }

      return [ true, '' ]
    }
    catch (error) {
      const handled = this.handleBoardValidationError(error, {
        issueTrackingId: this.issueTracking.id,
        targetBoard: projectKey,
        targetBoardType: 'project-key',
      })
      if (handled) {
        return handled
      }

      return [ false, 'Unable to validate Jira project key' ]
    }
  }

  private handleBoardValidationError(
    error: unknown,
    context: {
      issueTrackingId: string
      targetBoard: string
      targetBoardType: 'board-id' | 'project-key'
    },
  ): [ boolean, string ] | null {
    if (error instanceof HttpException && error.status === 404) {
      const message = context.targetBoardType === 'board-id'
        ? 'Jira board ID was not found'
        : 'Jira project key was not found'

      console.debug('Jira target board not found', {
        ...context,
      })
      return [ false, message ]
    }

    if (error instanceof HttpException && error.status === 403) {
      const message = context.targetBoardType === 'board-id'
        ? 'Jira board access denied'
        : 'Jira project access denied'

      console.debug('Jira target board access denied', {
        ...context,
      })
      return [ false, message ]
    }

    if (error instanceof HttpException && error.status === 401) {
      console.debug('Jira target board authentication failed', {
        ...context,
      })
      return [ false, 'Jira authentication failed. Check the email and access token.' ]
    }

    console.debug('Jira target board validation failed', {
      ...context,
      error,
    })

    return null
  }
}
