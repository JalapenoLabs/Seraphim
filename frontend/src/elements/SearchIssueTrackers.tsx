// Copyright © 2026 Jalapeno Labs

import type { IssueTracking } from '@common/types'

// Core
import { useEffect } from 'react'
import useLocalStorageState from 'use-local-storage-state'
import { useSelector } from '@frontend/framework/store'

// User interface
import { Autocomplete, AutocompleteItem, cn } from '@heroui/react'

type Props = {
  onSelectionChange: (issueTracker: IssueTracking) => void
  className?: string
  isDisabled?: boolean
}

export function SearchIssueTrackers(props: Props) {
  const issueTrackers = useSelector((state) => state.issueTracking.items)
  const [ selection, setSelection ] = useLocalStorageState<string>(
    'search-issue-trackers-selection',
    { defaultValue: '' },
  )

  useEffect(() => {
    if (selection) {
      const issueTrackerById = Object.fromEntries(
        issueTrackers.map((issueTracker) => [ issueTracker.id, issueTracker ]),
      )
      const selectedIssueTracker = issueTrackerById[selection]

      if (!selectedIssueTracker) {
        console.debug('SearchIssueTrackers could not restore issue tracker from selection key on initial load', {
          selection,
        })
        return
      }

      props.onSelectionChange(selectedIssueTracker)
    }
  }, [])

  return <Autocomplete
    fullWidth
    label='Issue tracker'
    placeholder='Select an issue tracker'
    className={cn(props.className)}
    isDisabled={props.isDisabled}
    selectedKey={selection}
    onSelectionChange={(selectionKey) => {
      const selectedIssueTrackerId = String(selectionKey || '')

      const issueTrackerById = Object.fromEntries(
        issueTrackers.map((issueTracker) => [ issueTracker.id, issueTracker ]),
      )
      const selectedIssueTracker = issueTrackerById[selectedIssueTrackerId]

      if (!selectedIssueTracker) {
        console.debug('SearchIssueTrackers could not resolve issue tracker from selection key', {
          selectionKey,
        })
        return
      }

      props.onSelectionChange(selectedIssueTracker)
      setSelection(selectedIssueTrackerId)
    }}
  >{ issueTrackers.map((issueTracker) => (
    <AutocompleteItem
      key={issueTracker.id}
      textValue={issueTracker.name}
    >{
      issueTracker.name
    }</AutocompleteItem>
  ))
  }</Autocomplete>
}
