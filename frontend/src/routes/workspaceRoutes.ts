// Copyright © 2026 Jalapeno Labs

import type { WorkspaceWithEnv } from '@common/types'
import type { UpsertWorkspaceRequest } from '@common/schema/workspace'

// Core
import { parseRequestBeforeSend } from '@common/api'
import { frontendClient } from '@frontend/framework/api'

// Redux
import { workspaceActions } from '@frontend/framework/redux/stores/workspaces'
import { dispatch } from '@frontend/framework/store'

// Schema
import { upsertWorkspaceSchema } from '@common/schema/workspace'

// /////////////////////////////// //
//         List Workspaces         //
// /////////////////////////////// //

type ListWorkspacesResponse = {
  workspaces: WorkspaceWithEnv[]
}

export async function listWorkspaces() {
  const response = await frontendClient
    .get('v1/protected/workspaces')
    .json<ListWorkspacesResponse>()

  dispatch(
    workspaceActions.setWorkspaces(response.workspaces),
  )

  return response
}


// /////////////////////////////// //
//         Get Workspace          //
// /////////////////////////////// //

type GetWorkspaceResponse = {
  workspace: WorkspaceWithEnv
}

export async function getWorkspace(workspaceId: string) {
  const response = await frontendClient
    .get(`v1/protected/workspaces/${workspaceId}`)
    .json<GetWorkspaceResponse>()

  dispatch(
    workspaceActions.upsertWorkspace(response.workspace),
  )

  return response
}

// /////////////////////////////// //
//         Upsert Workspace        //
// /////////////////////////////// //

type UpsertWorkspaceResponse = {
  workspace: WorkspaceWithEnv
}

export async function upsertWorkspace(
  workspaceId: string = '',
  raw: UpsertWorkspaceRequest,
) {
  const json = parseRequestBeforeSend(upsertWorkspaceSchema, raw)

  const response = await frontendClient
    .post(`v1/protected/workspaces/${workspaceId}`, { json })
    .json<UpsertWorkspaceResponse>()

  dispatch(
    workspaceActions.upsertWorkspace(response.workspace),
  )

  return response
}

// /////////////////////////////// //
//         Delete Workspace        //
// /////////////////////////////// //

type DeleteWorkspaceRequest = {
  id: string
}

type DeleteWorkspaceResponse = {
  deleted: boolean
  workspace: WorkspaceWithEnv
}

export async function deleteWorkspace(request: DeleteWorkspaceRequest) {
  const response = await frontendClient
    .delete(`v1/protected/workspaces/${request.id}`)
    .json<DeleteWorkspaceResponse>()

  dispatch(
    workspaceActions.removeWorkspace(response.workspace),
  )

  return response
}
