// Copyright © 2026 Jalapeno Labs

import type { AudioFor } from '@prisma/client'

// Lib
import { PrismaClient } from '@prisma/client'
import chalk from 'chalk'

// Core
import { existsSync } from 'node:fs'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { extname, join } from 'node:path'
import { tmpdir } from 'node:os'

// Utility
import { ChildProcess } from './ChildProcess'
import { superResolvePath } from './superResolve'

export type PlayAudioForOptions = {
  fallbackMediaUrl?: string
  prismaClient?: PrismaClient
}

export type PlayAudioForResult = {
  played: boolean
  source: 'database' | 'fallback' | null
  mediaPath: string | null
}

type PlayCommand = {
  executable: string
  args: string[]
}

type DatabaseAudioFile = {
  fileName: string
  mimeType: string
  data: Uint8Array<ArrayBufferLike>
}

/**
 * Plays audio for the provided AudioFor tag.
 *
 * Priority:
 * 1) User-uploaded Prisma audio file
 * 2) Optional fallbackMediaUrl
 */
export async function playAudioFor(
  audioFor: AudioFor,
  options: PlayAudioForOptions = {},
): Promise<PlayAudioForResult> {
  const databaseClient = options.prismaClient ?? new PrismaClient()
  const shouldDisconnectClient = !options.prismaClient

  try {
    const prismaResult = await playPrismaAudioFor(audioFor, {
      prismaClient: databaseClient,
    })
    if (prismaResult.source === 'database') {
      return prismaResult
    }

    if (!options.fallbackMediaUrl) {
      return {
        played: false,
        source: null,
        mediaPath: null,
      }
    }

    return playMediaUrl(options.fallbackMediaUrl)
  }
  finally {
    if (shouldDisconnectClient) {
      await databaseClient.$disconnect()
    }
  }
}

/**
 * Attempts to find and play a Prisma-backed user audio file for the requested tag.
 */
export async function playPrismaAudioFor(
  audioFor: AudioFor,
  options: Pick<PlayAudioForOptions, 'prismaClient'>,
): Promise<PlayAudioForResult> {
  let temporaryDirectoryPath: string | null = null

  try {
    const user = await options.prismaClient.user.findFirst({
      orderBy: { createdAt: 'asc' },
      select: { id: true },
    })
    if (!user) {
      console.debug('Cannot play Prisma audio because no user exists', {
        audioFor,
      })
      return {
        played: false,
        source: null,
        mediaPath: null,
      }
    }

    const databaseAudioFile = await options.prismaClient.audioFile.findUnique({
      where: {
        userId_audioFor: {
          userId: user.id,
          audioFor,
        },
      },
      select: {
        fileName: true,
        mimeType: true,
        data: true,
      },
    })

    if (!databaseAudioFile) {
      console.debug('No Prisma audio file found for requested tag', {
        audioFor,
      })
      return {
        played: false,
        source: null,
        mediaPath: null,
      }
    }

    temporaryDirectoryPath = await mkdtemp(join(tmpdir(), 'seraphim-audio-'))
    const mediaPath = join(
      temporaryDirectoryPath,
      `audio${resolveAudioFileExtension(databaseAudioFile)}`,
    )
    await writeFile(mediaPath, databaseAudioFile.data)

    const played = await playMediaFile(mediaPath)
    return {
      played,
      source: 'database',
      mediaPath,
    }
  }
  catch (error) {
    console.error('Failed while trying to play Prisma audio file', {
      audioFor,
      error,
    })
    return {
      played: false,
      source: null,
      mediaPath: null,
    }
  }
  finally {
    if (temporaryDirectoryPath) {
      await removeTemporaryDirectory(temporaryDirectoryPath)
    }
  }
}

/**
 * Attempts to play an audio file directly from a disk path.
 */
export async function playMediaUrl(mediaUrl: string): Promise<PlayAudioForResult> {
  try {
    const mediaPath = superResolvePath(mediaUrl)
    if (!existsSync(mediaPath)) {
      console.debug('Fallback media file did not exist on disk', {
        requestedMediaUrl: mediaUrl,
        resolvedMediaPath: mediaPath,
      })
      return {
        played: false,
        source: null,
        mediaPath: null,
      }
    }

    const played = await playMediaFile(mediaPath)
    return {
      played,
      source: 'fallback',
      mediaPath,
    }
  }
  catch (error) {
    console.error('Failed while trying to play fallback media URL', {
      mediaUrl,
      error,
    })
    return {
      played: false,
      source: null,
      mediaPath: null,
    }
  }
}

function resolveAudioFileExtension(databaseAudioFile: DatabaseAudioFile): string {
  const extensionFromFileName = extname(databaseAudioFile.fileName)
  if (extensionFromFileName) {
    return extensionFromFileName
  }

  if (databaseAudioFile.mimeType === 'audio/mpeg') {
    return '.mp3'
  }

  if (
    databaseAudioFile.mimeType === 'audio/wav'
    || databaseAudioFile.mimeType === 'audio/x-wav'
  ) {
    return '.wav'
  }

  console.debug('Unknown audio mime type, defaulting to .wav extension', {
    mimeType: databaseAudioFile.mimeType,
    fileName: databaseAudioFile.fileName,
  })
  return '.wav'
}

async function playMediaFile(mediaPath: string): Promise<boolean> {
  const ffplayCommand: PlayCommand = {
    executable: 'ffplay',
    args: [ '-nodisp', '-autoexit', mediaPath ],
  }

  const didPlay = await runPlayCommand(ffplayCommand)
  if (!didPlay) {
    console.error(chalk.red(
      'Failed to play audio with ffplay. Please ensure ffplay is installed and available in PATH.',
    ))
  }

  return didPlay
}

async function runPlayCommand(command: PlayCommand): Promise<boolean> {
  const playerProcess = new ChildProcess(command.executable, {
    args: command.args,
    windowsHide: true,
    stdio: [ 'ignore', 'pipe', 'pipe' ],
  })

  const exitCode = await playerProcess.waitForExit()
  if (exitCode === 0) {
    return true
  }

  console.debug('Audio play command exited with non-zero code', {
    command,
    exitCode,
    stderr: playerProcess.err,
  })
  return false
}

async function removeTemporaryDirectory(temporaryDirectoryPath: string): Promise<void> {
  try {
    await rm(temporaryDirectoryPath, { recursive: true, force: true })
  }
  catch (error) {
    console.debug('Failed to clean temporary audio directory', {
      temporaryDirectoryPath,
      error,
    })
  }
}
