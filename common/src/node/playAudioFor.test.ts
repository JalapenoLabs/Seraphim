// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { join } from 'node:path'

const mockState = vi.hoisted(() => {
  return {
    childExitCode: 0,
    childErr: [] as string[],
    resolvedMediaPath: '/resolved/fallback.mp3',
    fileExists: true,
    mkdtempPath: '/tmp/seraphim-audio-test',
    prismaClientInstances: [] as any[],
  }
})

const mockMkdtemp = vi.hoisted(() => vi.fn(async () => mockState.mkdtempPath))
const mockWriteFile = vi.hoisted(() => vi.fn(async () => undefined))
const mockRm = vi.hoisted(() => vi.fn(async () => undefined))
const mockExistsSync = vi.hoisted(() => vi.fn(() => mockState.fileExists))
const mockSuperResolvePath = vi.hoisted(() => vi.fn(() => mockState.resolvedMediaPath))
const mockChildProcess = vi.hoisted(() => vi.fn((command: string, options: Record<string, unknown>) => {
  return {
    command,
    options,
    err: mockState.childErr,
    waitForExit: vi.fn(async () => mockState.childExitCode),
  }
}))
const mockPrismaClientClass = vi.hoisted(() => vi.fn(() => {
  const prismaClient = {
    user: {
      findFirst: vi.fn(async () => null),
    },
    audioFile: {
      findUnique: vi.fn(async () => null),
    },
    $disconnect: vi.fn(async () => undefined),
  }

  mockState.prismaClientInstances.push(prismaClient)
  return prismaClient
}))

vi.mock('node:fs/promises', () => ({
  mkdtemp: mockMkdtemp,
  writeFile: mockWriteFile,
  rm: mockRm,
}))

vi.mock('node:fs', () => ({
  existsSync: mockExistsSync,
}))

vi.mock('./superResolve', () => ({
  superResolvePath: mockSuperResolvePath,
}))

vi.mock('./ChildProcess', () => ({
  ChildProcess: mockChildProcess,
}))

vi.mock('@prisma/client', () => ({
  PrismaClient: mockPrismaClientClass,
}))

type Context = {
  playAudioFor: typeof import('./playAudioFor').playAudioFor
  playPrismaAudioFor: typeof import('./playAudioFor').playPrismaAudioFor
  playMediaUrl: typeof import('./playAudioFor').playMediaUrl
}

describe('playAudioFor', () => {
  beforeEach<Context>(async (context) => {
    vi.clearAllMocks()
    vi.resetModules()

    mockState.childExitCode = 0
    mockState.childErr = []
    mockState.resolvedMediaPath = '/resolved/fallback.mp3'
    mockState.fileExists = true
    mockState.mkdtempPath = '/tmp/seraphim-audio-test'
    mockState.prismaClientInstances = []

    const playAudioForModule = await import('./playAudioFor')
    context.playAudioFor = playAudioForModule.playAudioFor
    context.playPrismaAudioFor = playAudioForModule.playPrismaAudioFor
    context.playMediaUrl = playAudioForModule.playMediaUrl
  })

  it<Context>('plays Prisma-backed audio and removes the temporary directory', async (context) => {
    const prismaClient = {
      user: {
        findFirst: vi.fn(async () => ({ id: 'user-1' })),
      },
      audioFile: {
        findUnique: vi.fn(async () => ({
          fileName: 'done.mp3',
          mimeType: 'audio/mpeg',
          data: new Uint8Array([ 1, 2, 3 ]),
        })),
      },
    } as any

    const result = await context.playPrismaAudioFor('DONE_SOUND' as any, {
      prismaClient,
    })

    const expectedMediaPath = join('/tmp/seraphim-audio-test', 'audio.mp3')

    expect(result).toEqual({
      played: true,
      source: 'database',
      mediaPath: expectedMediaPath,
    })
    expect(mockWriteFile).toHaveBeenCalledWith(
      expectedMediaPath,
      new Uint8Array([ 1, 2, 3 ]),
    )
    expect(mockRm).toHaveBeenCalledWith('/tmp/seraphim-audio-test', {
      recursive: true,
      force: true,
    })
    expect(mockChildProcess).toHaveBeenCalledWith('ffplay', {
      args: [ '-nodisp', '-autoexit', expectedMediaPath ],
      windowsHide: true,
      stdio: [ 'ignore', 'pipe', 'pipe' ],
    })
  })

  it<Context>('returns no source when user has no Prisma audio file', async (context) => {
    const prismaClient = {
      user: {
        findFirst: vi.fn(async () => ({ id: 'user-1' })),
      },
      audioFile: {
        findUnique: vi.fn(async () => null),
      },
    } as any

    const result = await context.playPrismaAudioFor('DONE_SOUND' as any, {
      prismaClient,
    })

    expect(result).toEqual({
      played: false,
      source: null,
      mediaPath: null,
    })
    expect(mockMkdtemp).not.toHaveBeenCalled()
    expect(mockChildProcess).not.toHaveBeenCalled()
  })

  it<Context>('plays fallback media URL when Prisma source is unavailable', async (context) => {
    const prismaClient = {
      user: {
        findFirst: vi.fn(async () => ({ id: 'user-1' })),
      },
      audioFile: {
        findUnique: vi.fn(async () => null),
      },
      $disconnect: vi.fn(async () => undefined),
    } as any

    const result = await context.playAudioFor('DONE_SOUND' as any, {
      prismaClient,
      fallbackMediaUrl: '/home/user/default-done.mp3',
    })

    expect(result).toEqual({
      played: true,
      source: 'fallback',
      mediaPath: '/resolved/fallback.mp3',
    })
    expect(mockSuperResolvePath).toHaveBeenCalledWith('/home/user/default-done.mp3')
  })

  it<Context>('returns no source when fallback media URL does not exist', async (context) => {
    mockState.fileExists = false

    const result = await context.playMediaUrl('C://sounds//fallback.wav')

    expect(result).toEqual({
      played: false,
      source: null,
      mediaPath: null,
    })
    expect(mockChildProcess).not.toHaveBeenCalled()
  })

  it<Context>('returns fallback source with played false when ffplay exits non-zero', async (context) => {
    mockState.childExitCode = 1
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined)

    const result = await context.playMediaUrl('/home/user/fallback.mp3')

    expect(result).toEqual({
      played: false,
      source: 'fallback',
      mediaPath: '/resolved/fallback.mp3',
    })
    expect(consoleErrorSpy).toHaveBeenCalled()

    consoleErrorSpy.mockRestore()
  })

  it<Context>('creates and disconnects PrismaClient when no client is passed', async (context) => {
    const result = await context.playAudioFor('DONE_SOUND' as any)

    expect(result).toEqual({
      played: false,
      source: null,
      mediaPath: null,
    })
    expect(mockPrismaClientClass).toHaveBeenCalledTimes(1)

    const [ createdPrismaClient ] = mockState.prismaClientInstances
    expect(createdPrismaClient.$disconnect).toHaveBeenCalledTimes(1)
  })
})
