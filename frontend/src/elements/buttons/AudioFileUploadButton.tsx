// Copyright © 2026 Jalapeno Labs

import type { AudioFor } from '@prisma/client'
import type { ChangeEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'

// UI
import { Button } from '@heroui/react'
import { UploadIcon } from '@frontend/elements/graphics/IconNexus'
import { fileToBase64 } from '@common/fileToBase64'

// Misc
import { DONE_SOUND_FILE_EXTENSIONS } from '@common/constants'
import {
  getCurrentUserAudioFile,
  type UserAudioFile,
  upsertCurrentUserAudioFile,
} from '@frontend/routes/userRoutes'

type Props = {
  audioFor: AudioFor
  label: string
  className?: string
}

export function AudioFileUploadButton(props: Props) {
  const fileInputRef = useRef<HTMLInputElement | null>(null)

  const [ currentAudioFile, setCurrentAudioFile ] = useState<UserAudioFile | null>(null)
  const [ isLoadingCurrentAudioFile, setIsLoadingCurrentAudioFile ] = useState(true)
  const [ isUploadingAudioFile, setIsUploadingAudioFile ] = useState(false)

  async function pullCurrentAudioFile() {
    setIsLoadingCurrentAudioFile(true)

    try {
      const response = await getCurrentUserAudioFile(props.audioFor)
      setCurrentAudioFile(response.audioFile)
    }
    catch (error) {
      console.error('Failed to fetch current user audio file', {
        audioFor: props.audioFor,
        error,
      })
    }
    finally {
      setIsLoadingCurrentAudioFile(false)
    }
  }

  useEffect(() => {
    pullCurrentAudioFile()
  }, [ props.audioFor ])

  async function onFileInputChange(event: ChangeEvent<HTMLInputElement>) {
    const selectedFile = event.currentTarget.files?.[0]
    if (!selectedFile) {
      return
    }

    setIsUploadingAudioFile(true)

    try {
      const isSupportedDoneSoundMimeType = selectedFile.type === 'audio/mpeg'
        || selectedFile.type === 'audio/wav'
        || selectedFile.type === 'audio/x-wav'
      if (!isSupportedDoneSoundMimeType) {
        console.debug('Attempted to upload unsupported audio mime type', {
          audioFor: props.audioFor,
          fileName: selectedFile.name,
          mimeType: selectedFile.type,
        })
        return
      }

      const dataBase64 = await fileToBase64(selectedFile)
      await upsertCurrentUserAudioFile({
        audioFor: props.audioFor,
        file: {
          name: selectedFile.name,
          mimeType: selectedFile.type,
          sizeBytes: selectedFile.size,
          dataBase64,
        },
      })

      await pullCurrentAudioFile()
    }
    catch (error) {
      console.error('Failed to upload user audio file', {
        audioFor: props.audioFor,
        fileName: selectedFile.name,
        error,
      })
    }
    finally {
      event.currentTarget.value = ''
      setIsUploadingAudioFile(false)
    }
  }

  const isBusy = isLoadingCurrentAudioFile || isUploadingAudioFile
  const buttonText = isBusy
    ? `Loading ${props.label}`
    : !currentAudioFile
      ? `Upload ${props.label}`
      : `Change ${props.label} (${currentAudioFile.fileName})`

  return <>
    <input
      ref={fileInputRef}
      className='hidden'
      type='file'
      accept={DONE_SOUND_FILE_EXTENSIONS.join(',')}
      onChange={onFileInputChange}
    />
    <Button
      className={props.className}
      color='default'
      variant='bordered'
      isLoading={isBusy}
      isDisabled={isBusy}
      startContent={<UploadIcon />}
      onPress={() => {
        fileInputRef.current?.click()
      }}
    >
      <span>{buttonText}</span>
    </Button>
  </>
}
