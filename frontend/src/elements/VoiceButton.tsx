// Copyright © 2026 Jalapeno Labs

import type { Color } from '@common/types'
import type { ReactNode, ComponentProps } from 'react'
import type { useVoice } from '@frontend/hooks/useVoice'

// UI
import { Button, Tooltip, cn } from '@heroui/react'
import { BiSolidMicrophoneOff, BiSolidMicrophone } from 'react-icons/bi'

type Props = {
  voice: ReturnType<typeof useVoice>
  size?: 'sm' | 'md' | 'lg'
  variant?: ComponentProps<typeof Button>['variant']
  color?: Color
  className?: string
  children?: ReactNode
}

export function VoiceButton(props: Props) {
  const {
    voice,
    size = 'md',
    variant = 'bordered',
    color = 'primary',
  } = props

  if (voice.isTurnedOff) {
    return <></>
  }

  const tooltip = voice.isDisabled
    ? 'Voice is disabled or not configured correctly'
    : voice.isActive
      ? 'Stop Voice'
      : 'Start Voice (uses your OS default microphone)'

  return <Tooltip content={tooltip}>
    <div className={cn('w-fit mx-auto', props.className)}>
      <Button
        id='voice'
        fullWidth
        isIconOnly={!props.children}
        size={size}
        className='button'
        variant={variant}
        color={color}
        isDisabled={voice.isDisabled}
        onPress={() => {
          if (voice.isActive) {
            voice.stop()
          }
          else {
            voice.start()
          }
        }}
      >
        <span className='icon'>{
          voice.isActive
            ? <BiSolidMicrophone />
            : <BiSolidMicrophoneOff />
        }</span>
        { props.children }
      </Button>
    </div>
  </Tooltip>
}
