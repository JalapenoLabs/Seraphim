// Copyright © 2026 Jalapeno Labs

import { Cloner } from './cloner'

export class GithubCloner extends Cloner {
  public getCloneUrl(): string {
    const owner = this.orgName?.trim()
    const repositoryName = this.repoName?.trim()
    if (!owner || !repositoryName) {
      console.debug('GithubCloner could not parse owner/repo, using source URL as-is', {
        sourceRepoUrl: this.sourceRepoUrl,
        owner: owner ?? null,
        repo: repositoryName ?? null,
      })
      return this.sourceRepoUrl
    }

    const repositoryPath = `${owner}/${repositoryName}`
    if (this.token) {
      const encodedToken = encodeURIComponent(this.token)
      return `https://x-access-token:${encodedToken}@github.com/${repositoryPath}.git`
    }

    return `https://github.com/${repositoryPath}.git`
  }
}
