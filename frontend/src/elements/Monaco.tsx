// Copyright © 2026 Jalapeno Labs

import type { OnMount, OnChange } from '@monaco-editor/react'
import type { Monaco, MonacoFileLanguages, MonacoContext } from '@frontend/framework/monaco'

// Core
import { useCallback } from 'react'
import Editor from '@monaco-editor/react'

// Theme
import { useSystemTheme } from '@frontend/hooks/useSystemTheme'
import { SERAPHIM_DARK_THEME, SERAPHIM_LIGHT_THEME } from '@frontend/framework/monaco'

export type MonacoRequiredProps = {
  value: string
  onChange: OnChange
  height: string
  fileLanguage?: MonacoFileLanguages
}

export type MonacoOptionalProps = {
  autoFocus?: boolean
  minimapOverride?: boolean
  readOnly?: boolean
  getMonaco?: (context: MonacoContext) => void
  fontSize?: number
  options?: Parameters<Monaco['editor']['create']>[2]
}

export type MonacoProps = MonacoRequiredProps & MonacoOptionalProps

export function Monaco(props: MonacoProps) {
  const theme = useSystemTheme()

  const handleMount: OnMount = useCallback((editor, monaco) => {
    if (props.autoFocus) {
      editor.focus()
    }

    if (props.getMonaco) {
      props.getMonaco({
        editor,
        monaco,
      })
    }
  }, [])

  return <Editor
    height={props.height}
    language={props.fileLanguage}
    value={props.value}
    onChange={!props.readOnly
      ? props.onChange
      : undefined
    }
    theme={theme === 'dark'
      ? SERAPHIM_DARK_THEME
      : SERAPHIM_LIGHT_THEME
    }
    loading={<></>}
    onMount={handleMount}
    options={{
      automaticLayout: true,
      minimap: {
        enabled: props.minimapOverride ?? true,
      },
      scrollBeyondLastLine: true,
      readOnly: props.readOnly ?? false,
      fontSize: props.fontSize,
      ...props.options,
    }}
  />
}
