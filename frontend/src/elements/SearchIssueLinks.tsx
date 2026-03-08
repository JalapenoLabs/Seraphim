// Copyright © 2026 Jalapeno Labs

import type { IssueTrackingSearchMode } from '@common/issueTracking/types'
import type { Selection } from '@react-types/shared'

// Core
import useLocalStorageState from 'use-local-storage-state'
import { useState } from 'react'

// User interface
import { Autocomplete, AutocompleteItem, Select, SelectItem, cn } from '@heroui/react'

// Utility
import { useSearchIssues } from '@frontend/hooks/useSearchIssues'

type Props = {
  issueTrackingId: string
  onSelection: (value: string) => void
  page?: number
  limit?: number
  className?: string
  isDisabled?: boolean
}

export function SearchIssueLinks(props: Props) {
  const {
    page = 1,
    limit = 10,
  } = props

  const [ search, setSearch ] = useState<string>('')
  const [ mode, setMode ] = useLocalStorageState<IssueTrackingSearchMode>('search-issue-links-mode', {
    defaultValue: 'text',
  })

  const issueSearch = useSearchIssues(props.issueTrackingId, search, mode, page, limit)

  return <div className={cn('level gap-2 items-end', props.className)}>
    <Autocomplete
      className='w-full'
      fullWidth
      label='Issue link'
      placeholder='PROJ-123'
      allowsCustomValue
      isLoading={issueSearch.isLoading}
      value={search}
      onValueChange={setSearch}
      onSelectionChange={(selectedKey) => {
        if (!selectedKey) {
          return
        }

        const selectedValue = String(selectedKey || '')
        props.onSelection(selectedValue)
      }}
      isDisabled={props.isDisabled || !props.issueTrackingId}
      errorMessage={issueSearch.error ? 'Unable to load issues right now.' : undefined}
      isInvalid={!!issueSearch.error && Boolean(search)}
    >{ issueSearch.issues.map((issue) => (
      <AutocompleteItem
        key={issue.key}
        textValue={issue.key}
      >{
        `${issue.key} - ${issue.summary}`
      }</AutocompleteItem>
    ))
    }</Autocomplete>
    <Select
      className='w-32'
      label='Search'
      disallowEmptySelection
      selectedKeys={[ mode ]}
      onSelectionChange={(selection: Selection) => {
        if (selection === 'all') {
          console.debug('SearchIssueLinks received invalid mode selection "all"')
          return
        }

        const [ selectedKey ] = Array.from(selection)
        if (selectedKey !== 'text' && selectedKey !== 'jql') {
          console.debug('SearchIssueLinks received unknown mode selection key', {
            selectedKey,
          })
          return
        }

        setMode(selectedKey)
      }}
      isDisabled={props.isDisabled || !props.issueTrackingId}
    >
      <SelectItem key='text'>Text</SelectItem>
      <SelectItem key='jql'>JQL</SelectItem>
    </Select>
  </div>
}
