// Copyright © 2026 Jalapeno Labs

import type { LlmWithRateLimits } from '@common/types'
import type { LlmType } from '@prisma/client'

// Core
import { useEffect, useMemo } from 'react'
import useLocalStorageState from 'use-local-storage-state'
import { useSelector } from '@frontend/framework/store'

// User interface
import { Autocomplete, AutocompleteItem, cn } from '@heroui/react'

type Props = {
  onSelectionChange: (llm: LlmWithRateLimits) => void
  className?: string
  isDisabled?: boolean
  selectedLlmId?: string
  selectionStorageKey?: string
  allowedLlmTypes?: readonly LlmType[]
  errorMessage?: string
}

export function SearchLlmAccounts(props: Props) {
  const allLlms = useSelector((state) => state.llms.items)
  const [ persistedSelection, setPersistedSelection ] = useLocalStorageState<string>(
    props.selectionStorageKey || 'search-llm-accounts-selection',
    { defaultValue: '' },
  )

  const llms = useMemo(() => {
    if (!props.allowedLlmTypes?.length) {
      return allLlms
    }

    const allowedTypes = new Set(props.allowedLlmTypes)
    return allLlms.filter((llm) => allowedTypes.has(llm.type))
  }, [ allLlms, props.allowedLlmTypes ])

  const selectedLlmId = props.selectedLlmId ?? persistedSelection

  useEffect(() => {
    if (!selectedLlmId) {
      return
    }

    const llmById = Object.fromEntries(
      llms.map((llm) => [ llm.id, llm ]),
    )
    const selectedLlm = llmById[selectedLlmId]

    if (!selectedLlm) {
      console.debug('SearchLlmAccounts could not restore LLM from selection key on initial load', {
        selectedLlmId,
      })
      return
    }

    props.onSelectionChange(selectedLlm)
  }, [])

  return <Autocomplete
    fullWidth
    label='LLM account'
    placeholder='Select an LLM account'
    className={cn(props.className)}
    isDisabled={props.isDisabled}
    selectedKey={selectedLlmId}
    errorMessage={props.errorMessage}
    onSelectionChange={(selectionKey) => {
      const selectedLlmIdFromUi = String(selectionKey || '')

      const llmById = Object.fromEntries(
        llms.map((llm) => [ llm.id, llm ]),
      )
      const selectedLlm = llmById[selectedLlmIdFromUi]

      if (!selectedLlm) {
        console.debug('SearchLlmAccounts could not resolve llm from selection key', {
          selectionKey,
        })
        return
      }

      props.onSelectionChange(selectedLlm)
      setPersistedSelection(selectedLlmIdFromUi)
    }}
  >{ llms.map((llm) => (
    <AutocompleteItem
      key={llm.id}
      textValue={llm.name}
    >{
      llm.name
    }</AutocompleteItem>
  ))
  }</Autocomplete>
}
