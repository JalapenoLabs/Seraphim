// Copyright © 2026 Jalapeno Labs

import type { FileChangeType, FileContentType, Prisma } from '@prisma/client'
import type { TaskInstance } from './taskInstance'

// Lib
import { detect } from 'chardet'
import iconv from 'iconv-lite'

// Misc
import { DOCKER_WORKDIR } from '@common/constants'

type FileChangeDraft = Omit<Prisma.FileChangeCreateManyInput, 'taskId'>

type FileStats = {
  additions: number
  removals: number
}

type FileSnapshot = {
  content: string | null
  contentType: FileContentType | null
}

type CommandExpectation = {
  description: string
  allowedExitCodes?: number[]
}

const NULL_CHARACTER = '\u0000'

export async function collectTaskFileChanges(taskInstance: TaskInstance): Promise<FileChangeDraft[]> {
  const sourceGitBranch = taskInstance.data.sourceGitBranch?.trim()
  if (!sourceGitBranch) {
    console.debug('collectTaskFileChanges called without source git branch', {
      taskId: taskInstance.id,
      sourceGitBranch: taskInstance.data.sourceGitBranch,
    })
    throw new Error('Task source git branch is required to evaluate file diffs')
  }

  const baseRef = `origin/${sourceGitBranch}`
  await assertBaseRefExists(taskInstance, baseRef)

  const [
    addedPathsOutput,
    modifiedPathsOutput,
    deletedPathsOutput,
    typeChangedPathsOutput,
    trackedNumstatOutput,
    untrackedPathsOutput,
  ] = await Promise.all([
    runRequiredCommand(
      taskInstance,
      `cd ${toShellLiteral(DOCKER_WORKDIR)} && git diff --diff-filter=A --name-only -z ${toShellLiteral(baseRef)} --`,
      { description: 'collect added file paths' },
    ),
    runRequiredCommand(
      taskInstance,
      `cd ${toShellLiteral(DOCKER_WORKDIR)} && git diff --diff-filter=M --name-only -z ${toShellLiteral(baseRef)} --`,
      { description: 'collect modified file paths' },
    ),
    runRequiredCommand(
      taskInstance,
      `cd ${toShellLiteral(DOCKER_WORKDIR)} && git diff --diff-filter=D --name-only -z ${toShellLiteral(baseRef)} --`,
      { description: 'collect deleted file paths' },
    ),
    runRequiredCommand(
      taskInstance,
      `cd ${toShellLiteral(DOCKER_WORKDIR)} && git diff --diff-filter=T --name-only -z ${toShellLiteral(baseRef)} --`,
      { description: 'collect type-changed file paths' },
    ),
    runRequiredCommand(
      taskInstance,
      `cd ${toShellLiteral(DOCKER_WORKDIR)} && git diff --numstat -z --no-renames ${toShellLiteral(baseRef)} --`,
      { description: 'collect tracked file stats' },
    ),
    runRequiredCommand(
      taskInstance,
      `cd ${toShellLiteral(DOCKER_WORKDIR)} && git ls-files --others --exclude-standard -z --`,
      { description: 'collect untracked file paths' },
    ),
  ])

  const changeTypeByPath = new Map<string, FileChangeType>()
  appendPaths(changeTypeByPath, parseNullDelimitedList(addedPathsOutput), 'Added')
  appendPaths(changeTypeByPath, parseNullDelimitedList(modifiedPathsOutput), 'Modified')
  appendPaths(changeTypeByPath, parseNullDelimitedList(deletedPathsOutput), 'Deleted')
  appendPaths(changeTypeByPath, parseNullDelimitedList(typeChangedPathsOutput), 'TypeChanged')

  const trackedStatsByPath = parseNumstatOutput(trackedNumstatOutput)
  for (const path of parseNullDelimitedList(untrackedPathsOutput)) {
    if (changeTypeByPath.has(path)) {
      continue
    }

    changeTypeByPath.set(path, 'Untracked')
  }

  const sortedPaths = [ ...changeTypeByPath.keys() ].sort((leftPath, rightPath) => leftPath.localeCompare(rightPath))
  const fileChanges: FileChangeDraft[] = []

  for (const path of sortedPaths) {
    const changeType = changeTypeByPath.get(path)
    if (!changeType) {
      console.debug('collectTaskFileChanges skipped missing change type for path', {
        taskId: taskInstance.id,
        path,
      })
      continue
    }

    let fileStats = trackedStatsByPath.get(path) ?? { additions: 0, removals: 0 }
    if (changeType === 'Untracked') {
      fileStats = await collectUntrackedFileStats(taskInstance, path)
    }

    const originalSnapshot = await collectOriginalFileSnapshot(taskInstance, baseRef, path, changeType)
    const newSnapshot = await collectNewFileSnapshot(taskInstance, path, changeType)

    fileChanges.push({
      path,
      changeType,
      additions: fileStats.additions,
      removals: fileStats.removals,
      originalContent: originalSnapshot.content,
      originalContentType: originalSnapshot.contentType,
      newContent: newSnapshot.content,
      newContentType: newSnapshot.contentType,
    })
  }

  return fileChanges
}

function appendPaths(
  changeTypeByPath: Map<string, FileChangeType>,
  paths: string[],
  changeType: FileChangeType,
): void {
  for (const path of paths) {
    changeTypeByPath.set(path, changeType)
  }
}

function parseNullDelimitedList(output: string): string[] {
  return output
    .split(NULL_CHARACTER)
    .filter((value) => Boolean(value))
}

function parseNumstatOutput(output: string): Map<string, FileStats> {
  const statsByPath = new Map<string, FileStats>()
  const records = parseNullDelimitedList(output)

  for (const record of records) {
    const match = /^([^\t]+)\t([^\t]+)\t([\s\S]*)$/u.exec(record)
    if (!match) {
      console.debug('parseNumstatOutput received unexpected record', {
        record,
      })
      continue
    }

    const [ , additionsRaw, removalsRaw, path ] = match
    statsByPath.set(path, {
      additions: additionsRaw === '-'
        ? 0
        : Number.parseInt(additionsRaw, 10),
      removals: removalsRaw === '-'
        ? 0
        : Number.parseInt(removalsRaw, 10),
    })
  }

  return statsByPath
}

async function assertBaseRefExists(taskInstance: TaskInstance, baseRef: string): Promise<void> {
  await runRequiredCommand(
    taskInstance,
    `cd ${toShellLiteral(DOCKER_WORKDIR)} && git rev-parse --verify --quiet ${toShellLiteral(`${baseRef}^{commit}`)}`,
    { description: `verify git base ref ${baseRef}` },
  )
}

async function collectOriginalFileSnapshot(
  taskInstance: TaskInstance,
  baseRef: string,
  path: string,
  changeType: FileChangeType,
): Promise<FileSnapshot> {
  if (changeType === 'Added' || changeType === 'Untracked') {
    return {
      content: null,
      contentType: null,
    }
  }

  return collectGitBlobSnapshot(taskInstance, `${baseRef}:${path}`, path)
}

async function collectNewFileSnapshot(
  taskInstance: TaskInstance,
  path: string,
  changeType: FileChangeType,
): Promise<FileSnapshot> {
  if (changeType === 'Deleted') {
    return {
      content: null,
      contentType: null,
    }
  }

  const result = await taskInstance.executeCmd(
    `cd ${toShellLiteral(DOCKER_WORKDIR)} && base64 -w0 -- ${toShellLiteral(path)}`,
  )

  if (result.exitCode !== 0) {
    console.debug('collectNewFileSnapshot failed to read working tree file', {
      taskId: taskInstance.id,
      path,
      exitCode: result.exitCode,
      stderr: result.stderr,
    })

    return {
      content: null,
      contentType: 'Unknown',
    }
  }

  return decodeBase64Snapshot(result.stdout)
}

async function collectGitBlobSnapshot(
  taskInstance: TaskInstance,
  blobSpec: string,
  path: string,
): Promise<FileSnapshot> {
  const result = await taskInstance.executeCmd(
    `set -o pipefail && cd ${toShellLiteral(DOCKER_WORKDIR)} && git show ${toShellLiteral(blobSpec)} | base64 -w0`,
  )

  if (result.exitCode !== 0) {
    console.debug('collectGitBlobSnapshot failed to read base file blob', {
      taskId: taskInstance.id,
      path,
      blobSpec,
      exitCode: result.exitCode,
      stderr: result.stderr,
    })

    return {
      content: null,
      contentType: 'Unknown',
    }
  }

  return decodeBase64Snapshot(result.stdout)
}

async function collectUntrackedFileStats(taskInstance: TaskInstance, path: string): Promise<FileStats> {
  const result = await taskInstance.executeCmd(
    `cd ${toShellLiteral(DOCKER_WORKDIR)} && git diff --no-index --numstat -- /dev/null ${toShellLiteral(path)}`,
  )

  if (result.exitCode !== 0 && result.exitCode !== 1) {
    console.debug('collectUntrackedFileStats failed to diff untracked file', {
      taskId: taskInstance.id,
      path,
      exitCode: result.exitCode,
      stderr: result.stderr,
    })

    return {
      additions: 0,
      removals: 0,
    }
  }

  const match = /^([^\t]+)\t([^\t]+)/u.exec(result.stdout)
  if (!match) {
    console.debug('collectUntrackedFileStats received unexpected numstat output', {
      taskId: taskInstance.id,
      path,
      stdout: result.stdout,
    })

    return {
      additions: 0,
      removals: 0,
    }
  }

  const [ , additionsRaw, removalsRaw ] = match
  return {
    additions: additionsRaw === '-'
      ? 0
      : Number.parseInt(additionsRaw, 10),
    removals: removalsRaw === '-'
      ? 0
      : Number.parseInt(removalsRaw, 10),
  }
}

async function runRequiredCommand(
  taskInstance: TaskInstance,
  command: string,
  expectation: CommandExpectation,
): Promise<string> {
  const result = await taskInstance.executeCmd(command)
  const allowedExitCodes = expectation.allowedExitCodes || [ 0 ]

  if (!allowedExitCodes.includes(result.exitCode)) {
    console.debug('runRequiredCommand received unexpected exit code', {
      taskId: taskInstance.id,
      description: expectation.description,
      command,
      exitCode: result.exitCode,
      stderr: result.stderr,
    })

    throw new Error(`Failed to ${expectation.description}`)
  }

  return result.stdout
}

function toShellLiteral(value: string): string {
  return `'${value.replaceAll('\'', '\'"\'"\'')}'`
}

function decodeBase64Snapshot(output: string): FileSnapshot {
  const normalizedOutput = output.trim()
  const contentBuffer = Buffer.from(normalizedOutput, 'base64')
  return decodeBuffer(contentBuffer)
}

function decodeBuffer(contentBuffer: Buffer): FileSnapshot {
  if (!contentBuffer.length) {
    return {
      content: '',
      contentType: 'Utf8',
    }
  }

  if (looksBinary(contentBuffer)) {
    return {
      content: null,
      contentType: 'Binary',
    }
  }

  const detectedEncoding = detect(contentBuffer)
  if (!detectedEncoding?.trim()) {
    return {
      content: null,
      contentType: 'Unknown',
    }
  }

  const normalizedEncoding = normalizeEncodingName(detectedEncoding)
  if (!normalizedEncoding) {
    return {
      content: null,
      contentType: 'Unknown',
    }
  }

  return decodeWithEncoding(contentBuffer, normalizedEncoding)
}

function normalizeEncodingName(detectedEncoding: string): string | null {
  const normalizedEncoding = detectedEncoding.trim().toLowerCase()

  const encodingAliases: Record<string, string> = {
    'ascii': 'utf-8',
    'utf8': 'utf-8',
    'utf-8': 'utf-8',
    'utf16le': 'utf-16le',
    'utf-16le': 'utf-16le',
    'utf16be': 'utf-16be',
    'utf-16be': 'utf-16be',
  }

  const mappedEncoding = encodingAliases[normalizedEncoding] || normalizedEncoding
  if (!iconv.encodingExists(mappedEncoding)) {
    console.debug('decodeBuffer detected unsupported encoding', {
      detectedEncoding,
      mappedEncoding,
    })

    return null
  }

  return mappedEncoding
}

function decodeWithEncoding(
  contentBuffer: Buffer,
  encoding: string,
): FileSnapshot {
  try {
    const decodedContent = iconv.decode(contentBuffer, encoding, {
      stripBOM: true,
    })

    const encodedRoundTrip = iconv.encode(decodedContent, encoding)
    if (!encodedRoundTrip.equals(contentBuffer)) {
      return {
        content: null,
        contentType: 'Unknown',
      }
    }

    const contentType = getContentTypeForEncoding(encoding)

    return {
      content: decodedContent,
      contentType,
    }
  }
  catch {
    return {
      content: null,
      contentType: 'Unknown',
    }
  }
}

function getContentTypeForEncoding(encoding: string): FileContentType {
  if (encoding === 'utf-8') {
    return 'Utf8'
  }

  if (encoding === 'utf-16le') {
    return 'Utf16Le'
  }

  if (encoding === 'utf-16be') {
    return 'Utf16Be'
  }

  return 'Unknown'
}

function looksBinary(contentBuffer: Buffer): boolean {
  if (contentBuffer.includes(0)) {
    return true
  }

  const sampleLength = Math.min(contentBuffer.length, 8_000)
  let suspiciousByteCount = 0

  for (let index = 0; index < sampleLength; index++) {
    const currentByte = contentBuffer[index]
    const isAsciiControl = currentByte < 32
    const isAllowedControl = currentByte === 9 || currentByte === 10 || currentByte === 13 || currentByte === 12

    if (isAsciiControl && !isAllowedControl) {
      suspiciousByteCount++
    }
  }

  return suspiciousByteCount > sampleLength * 0.3
}
