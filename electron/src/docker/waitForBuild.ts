// Copyright © 2026 Jalapeno Labs

import type { getDockerClient } from './docker'

// Core
import { StatusResponse } from '@common/vendor/buildkit/gen/github.com/moby/buildkit/api/services/control/control'

type BuildFailureError = Error & {
  output?: string
}

function buildFailureError(
  outputParts: string[],
  fallbackMessage: string,
  originalError?: unknown,
): BuildFailureError {
  const resolvedError = originalError instanceof Error
    ? originalError
    : new Error(fallbackMessage)

  if (!resolvedError.message?.trim()) {
    resolvedError.message = fallbackMessage
  }

  const output = outputParts.join('')
  if (output.trim().length > 0) {
    Reflect.set(resolvedError, 'output', output)
  }

  return resolvedError as BuildFailureError
}

export function waitForBuildVersion2(
  buildStream: NodeJS.ReadableStream,
  dockerClient: ReturnType<typeof getDockerClient>,
) {
  return new Promise<string>((resolve, reject) => {
    let hasError = false
    const outputParts: string[] = []

    function appendOutput(message: string) {
      if (!message) {
        return
      }

      const suffix = message.endsWith('\n') ? '' : '\n'
      outputParts.push(`${message}${suffix}`)
    }

    function getEventString(
      event: Record<string, unknown>,
      key: string,
    ) {
      const value = event[key]
      if (typeof value === 'string') {
        return value
      }
      return null
    }

    function getProgressDetail(
      event: Record<string, unknown>,
    ) {
      const value = event.progressDetail
      if (!value || typeof value !== 'object') {
        return null
      }

      const currentValue = Reflect.get(value, 'current')
      const totalValue = Reflect.get(value, 'total')
      if (typeof currentValue !== 'number' || typeof totalValue !== 'number') {
        return null
      }

      return {
        current: currentValue,
        total: totalValue,
      }
    }

    function getProgressLabel(
      event: Record<string, unknown>,
    ) {
      const progress = event.progress
      if (typeof progress === 'string' && progress.trim().length > 0) {
        return progress
      }

      const progressDetail = getProgressDetail(event)
      if (!progressDetail) {
        return null
      }

      return `${progressDetail.current}/${progressDetail.total}`
    }

    function decodeBuildkitTrace(auxBase64: string): StatusResponse | null {
      try {
        const bytes = Buffer.from(auxBase64, 'base64')
        return StatusResponse.decode(bytes)
      }
      catch (error) {
        console.debug('[Docker Build][trace] Failed to decode protobuf', { error })
        return null
      }
    }

    function handleBuildkitStatus(status: StatusResponse) {
      // 1) Logs = the “real” output you probably care about
      if (status.logs?.length) {
        for (const l of status.logs) {
          // In BuildKit, log.msg is bytes (Uint8Array)
          if (!l.msg || l.msg.length === 0) {
            continue
          }

          const text = Buffer.from(l.msg).toString('utf8')
          if (!text) {
            continue
          }

          // Optional: prefix by vertex id for debugging
          // const prefix = l.vertex ? `[${l.vertex}] ` : ''
          // console.debug('[Docker Build][trace]', prefix + text.trimEnd())

          appendOutput(text)
        }
      }

      // 2) Optional: structured progress. Usually too noisy for outputParts,
      // but handy for debug logs / a progress UI.
      if (status.statuses?.length) {
        for (const s of status.statuses) {
          const id = s.ID ?? ''
          const name = s.name ?? ''
          const current = s.current ?? 0
          const total = s.total ?? 0

          if (name) {
            const suffix = total > 0 ? ` ${current}/${total}` : ''
            console.debug(`[Docker Build][progress] ${id} ${name}${suffix}`)
          }
        }
      }

      // 3) Surface vertex errors (if any) as output + mark build as failed
      if (status.vertexes?.length) {
        for (const v of status.vertexes) {
          if (v.error) {
            hasError = true
            appendOutput(v.error)
            console.debug('[Docker Build][vertex error]', {
              name: v.name,
              error: v.error,
            })
          }
        }
      }
    }

    dockerClient.modem.followProgress(
      buildStream,
      function handleBuildFinished(error: unknown) {
        if (error) {
          console.debug('Error during Docker build', { error })
          reject(buildFailureError(outputParts, 'Docker build failed', error))
          return
        }

        if (hasError) {
          reject(buildFailureError(outputParts, 'Docker build failed'))
          return
        }

        resolve(outputParts.join(''))
      },
      function handleBuildProgress(event: Record<string, unknown>) {
        const errorMessage = getEventString(event, 'error')
        if (errorMessage) {
          hasError = true
          appendOutput(errorMessage)
          console.debug('Docker build error', { error: errorMessage })
          return
        }

        const status = getEventString(event, 'status')
        if (status) {
          const id = getEventString(event, 'id')
          const progressLabel = getProgressLabel(event)
          const prefix = id ? ` ${id}` : ''
          const progressSuffix = progressLabel ? ` ${progressLabel}` : ''
          console.debug(`[Docker Build]${prefix} ${status}${progressSuffix}`)
          return
        }

        const stream = getEventString(event, 'stream')
        if (stream) {
          const message = stream.trim()
          if (message) {
            console.debug('[Docker Build]', message)
          }
          appendOutput(stream)
          return
        }

        const id = getEventString(event, 'id')
        if (id === 'moby.buildkit.trace') {
          const aux = event.aux
          if (typeof aux === 'string') {
            const statusResp = decodeBuildkitTrace(aux)
            if (statusResp) {
              handleBuildkitStatus(statusResp)
            }
          }
          return
        }

        if (id === 'moby.image.id') {
          // dockerode types are loose here
          // @ts-ignore
          const aux = event.aux?.ID
          if (typeof aux === 'string') {
            console.debug('[Docker Build] Built image with ID', aux)
            return
          }
        }

        console.debug('[Docker Build] Unrecognized progress event', { event })
      },
    )
  })
}

// For use without buildkit (docker build v1)
export function waitForBuildVersion1(
  buildStream: NodeJS.ReadableStream,
  dockerClient: ReturnType<typeof getDockerClient>,
) {
  return new Promise<string>((resolve, reject) => {
    let hasError = false
    const outputParts: string[] = []

    function appendOutput(message: string) {
      if (!message) {
        return
      }

      const suffix = message.endsWith('\n') ? '' : '\n'
      outputParts.push(`${message}${suffix}`)
    }

    function handleBuildFinished(error: unknown) {
      if (error) {
        console.debug('Error during Docker build', {
          error,
        })
        reject(buildFailureError(outputParts, 'Docker build failed', error))
        return
      }

      if (hasError) {
        reject(buildFailureError(outputParts, 'Docker build failed'))
        return
      }

      resolve(outputParts.join(''))
    }

    function handleBuildProgress(event: Record<string, unknown>) {
      if (typeof event.error === 'string') {
        hasError = true
        appendOutput(event.error)
        console.debug('Docker build error', { error: event.error })
        return
      }

      if (typeof event.stream === 'string') {
        const message = event.stream.trim()
        if (message) {
          console.debug('[Docker Build]', message)
        }
        appendOutput(event.stream)
      }
    }

    dockerClient.modem.followProgress(
      buildStream,
      handleBuildFinished,
      handleBuildProgress,
    )
  })
}
