// Copyright © 2026 Jalapeno Labs

import type { IssueTracking, IssueTrackingProvider } from '@prisma/client'

// Core
import { IssueTracker } from './polymorphism/issueTracker'
import { JiraIssueTracker } from './polymorphism/jira'
import { GithubIssueTracker } from './polymorphism/github'

const IssueTrackingProviderToTrackerMap: Record<
  IssueTrackingProvider,
  new (issueTracking: IssueTracking) => IssueTracker
> = {
  Jira: JiraIssueTracker,
  Github: GithubIssueTracker,
}

export function getIssueTracker(issueTracking: IssueTracking): IssueTracker {
  const TrackerClass = IssueTrackingProviderToTrackerMap[issueTracking.provider]
  if (!TrackerClass) {
    console.debug('Unsupported issue tracking provider, using base client', {
      provider: issueTracking.provider,
    })
    return new IssueTracker(issueTracking)
  }

  return new TrackerClass(issueTracking)
}
