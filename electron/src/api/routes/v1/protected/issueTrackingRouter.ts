// Copyright © 2026 Jalapeno Labs

import type { Router } from 'express'

// Core
import { Router as createRouter } from 'express'

// Misc
import { handleDeleteIssueTrackingRequest } from './issueTracking/deleteIssueTrackingRoute'
import { handleGetIssueTrackingRequest } from './issueTracking/getIssueTrackingRoute'
import { handleListIssueTrackingIssuesRequest } from './issueTracking/listIssuesRoute'
import { handleListIssueTrackingRequest } from './issueTracking/listIssueTrackingRoute'
import { handleUpsertIssueTrackingRequest } from './issueTracking/upsertIssueTrackingRoute'

export function createIssueTrackingRouter(): Router {
  const issueTrackingRouter = createRouter()

  // /api/v1/protected/issue-tracking
  issueTrackingRouter.post(
    '/',
    handleUpsertIssueTrackingRequest,
  )

  // /api/v1/protected/issue-tracking
  issueTrackingRouter.get(
    '/',
    handleListIssueTrackingRequest,
  )

  // /api/v1/protected/issue-tracking/:issueTrackingId
  issueTrackingRouter.get(
    '/:issueTrackingId',
    handleGetIssueTrackingRequest,
  )

  // /api/v1/protected/issue-tracking/:issueTrackingId/issues
  issueTrackingRouter.get(
    '/:issueTrackingId/issues',
    handleListIssueTrackingIssuesRequest,
  )

  // /api/v1/protected/issue-tracking/:issueTrackingId
  issueTrackingRouter.post(
    '/:issueTrackingId',
    handleUpsertIssueTrackingRequest,
  )

  // /api/v1/protected/issue-tracking/:issueTrackingId
  issueTrackingRouter.delete(
    '/:issueTrackingId',
    handleDeleteIssueTrackingRequest,
  )

  return issueTrackingRouter
}
