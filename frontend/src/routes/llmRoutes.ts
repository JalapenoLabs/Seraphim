// Copyright © 2026 Jalapeno Labs

import type { Llm, LlmWithRateLimits } from '@common/types'
import type { UpsertLlmRequest } from '@common/schema/llm'

// Core
import { parseRequestBeforeSend } from '@common/api'
import { frontendClient } from '@frontend/framework/api'

// Redux
import { llmActions } from '@frontend/framework/redux/stores/llms'
import { dispatch } from '@frontend/framework/store'

// Schema
import { upsertLlmSchema } from '@common/schema/llm'

// /////////////////////////////// //
//            List LLMs            //
// /////////////////////////////// //

type ListLlmsResponse = {
  llms: LlmWithRateLimits[]
}

// This route intentionally has no pagination
export async function listLlms() {
  const response = await frontendClient
    .get('v1/protected/llms')
    .json<ListLlmsResponse>()

  dispatch(
    llmActions.setLlms(response.llms),
  )

  return response
}


// /////////////////////////////// //
//            Get LLM             //
// /////////////////////////////// //

type GetLlmResponse = {
  llm: LlmWithRateLimits
}

export async function getLlm(llmId: string) {
  const response = await frontendClient
    .get(`v1/protected/llms/${llmId}`)
    .json<GetLlmResponse>()

  dispatch(
    llmActions.upsertLlm(response.llm),
  )

  return response
}

// /////////////////////////////// //
//           Upsert LLM            //
// /////////////////////////////// //

type UpsertLlmResponse = {
  llm: LlmWithRateLimits
}

export async function upsertLlm(id: string = '', raw: UpsertLlmRequest) {
  const json = parseRequestBeforeSend(upsertLlmSchema, raw)

  const response = await frontendClient
    .post(`v1/protected/llms/${id}`, { json })
    .json<UpsertLlmResponse>()

  dispatch(
    llmActions.upsertLlm(response.llm),
  )

  return response
}

// /////////////////////////////// //
//           Delete LLM            //
// /////////////////////////////// //

type DeleteLlmRequest = Llm

export async function deleteLlm(llm: DeleteLlmRequest) {
  const response = await frontendClient
    .delete(`v1/protected/llms/${llm.id}`)

  dispatch(
    llmActions.removeLlm(llm),
  )

  return response
}
