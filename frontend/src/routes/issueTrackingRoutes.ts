// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@common/types'
import type { UpsertIssueTrackingRequest } from '@common/schema/issueTracking'
import type { IssueTrackingIssueList } from '@common/issueTracking/types'
import type { ListIssueTrackingIssuesRequest } from '@common/schema/issueTracking'

// Core
import { buildUrlParams, parseRequestBeforeSend } from '@common/api'
import { frontendClient } from '@frontend/framework/api'

// Redux
import { issueTrackingActions } from '@frontend/framework/redux/stores/issueTracking'
import { dispatch } from '@frontend/framework/store'

// Schema
import { upsertIssueTrackingSchema } from '@common/schema/issueTracking'
import { listIssueTrackingIssuesRequestSchema } from '@common/schema/issueTracking'

// /////////////////////////////// //
//     List Issue Tracking Accounts //
// /////////////////////////////// //

type ListIssueTrackingResponse = {
  issueTracking: IssueTracking[]
}

// This route intentionally has no pagination
export async function listIssueTracking(): Promise<ListIssueTrackingResponse> {
  const response = await frontendClient
    .get('v1/protected/issue-tracking')
    .json<ListIssueTrackingResponse>()

  dispatch(
    issueTrackingActions.setIssueTracking(response.issueTracking),
  )

  return response
}


// /////////////////////////////// //
//        List provider issues         //
// /////////////////////////////// //

type ListIssueTrackingIssuesResponse = IssueTrackingIssueList & {
  issueTrackingId: string
}

export function listIssueTrackingIssues(request: ListIssueTrackingIssuesRequest) {
  const query = parseRequestBeforeSend(listIssueTrackingIssuesRequestSchema, request)

  return frontendClient
    .get(`v1/protected/issue-tracking/${query.issueTrackingId}/issues`, {
      searchParams: buildUrlParams({
        q: query.q,
        mode: query.mode,
        page: query.page,
        limit: query.limit,
      }),
    })
    .json<ListIssueTrackingIssuesResponse>()
}

// /////////////////////////////// //
//      Upsert Issue Tracking       //
// /////////////////////////////// //

type UpsertIssueTrackingResponse = {
  issueTracking: IssueTracking
}

export async function upsertIssueTracking(
  issueTrackingId: string = '',
  raw: UpsertIssueTrackingRequest,
) {
  const json = parseRequestBeforeSend(upsertIssueTrackingSchema, raw)

  const response = await frontendClient
    .post(`v1/protected/issue-tracking/${issueTrackingId}`, { json })
    .json<UpsertIssueTrackingResponse>()

  dispatch(
    issueTrackingActions.upsertIssueTracking(response.issueTracking),
  )

  return response
}

// /////////////////////////////// //
//      Delete Issue Tracking       //
// /////////////////////////////// //

type DeleteIssueTrackingResponse = {
  issueTrackingId: string
  deleted: boolean
}

export async function deleteIssueTracking(issueTracking: IssueTracking) {
  const response = await frontendClient
    .delete(`v1/protected/issue-tracking/${issueTracking.id}`)
    .json<DeleteIssueTrackingResponse>()

  dispatch(
    issueTrackingActions.removeIssueTracking(issueTracking),
  )

  return response
}
