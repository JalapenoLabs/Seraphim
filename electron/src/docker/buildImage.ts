// Copyright © 2026 Jalapeno Labs

import type { Workspace } from '@prisma/client'
import type { TaskWithFullContext } from '@common/types'
import type { Cloner } from '@common/cloning/polymorphism/cloner'

// Docker
import { requireDatabaseClient } from '@electron/database'
import { getDockerClient } from '@electron/docker/docker'
import { buildDockerfileContents } from '@electron/docker/image'
import { pullWithProgress } from './pullWithProgress'
import { waitForBuildVersion1, waitForBuildVersion2 } from './waitForBuild'
import { createCodexConfig, createCodexAuthFile } from '@common/llms/codexConfig'

// Node.js
import { mkdtemp, writeFile, rm, readdir, readFile, mkdir } from 'node:fs/promises'
import { cpSync, mkdirSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve, relative } from 'node:path'
import { utilsDir } from '@electron/lib/internalFiles'

// Utility
import { v7 as uuid } from 'uuid'
import { writeSetupScriptFile } from '@common/writeSetupScriptFile'
import { getCloner } from '@common/cloning/getCloner'

// Misc
import {
  DEFAULT_DOCKER_BASE_IMAGE,
  SETUP_SCRIPT_NAME,
  VALIDATE_SCRIPT_NAME,
  DOCKER_USE_BUILDKIT,
} from '@common/constants'
import { getSecrets } from '@electron/tasks/getSecrets'

const contextFiles = [
  'Dockerfile',
  'AGENTS.md',
  'docs',
  `utils`,
  SETUP_SCRIPT_NAME,
  VALIDATE_SCRIPT_NAME,
  'codex_config.toml',
  'codex_auth.json',
] as const satisfies string[]

type BuildImageResult = {
  success: boolean
  imageTag?: string
  errors?: string[]
}

export async function buildImage(
  workspace: Workspace,
  task?: TaskWithFullContext,
  cloner?: Cloner,
  toStdout?: (data: string) => void,
): Promise<BuildImageResult> {
  let contextDir: string | null = null

  try {
    const prisma = requireDatabaseClient('buildImage')

    const dockerClient = getDockerClient()
    const buildTag = `seraphim-${workspace.id}-${uuid()}`

    // /////////////////////////// //
    //          SETUP INFO         //
    // /////////////////////////// //

    if (!cloner && task?.gitAccount) {
      cloner = getCloner(
        task.gitAccount.provider,
        workspace.sourceRepoUrl || '',
        task.gitAccount.accessToken,
      )
    }

    const dockerfileContents = buildDockerfileContents({
      customCommands: workspace.customDockerfileCommands,
    })

    // /////////////////////////// //
    //     PREPARE CONTEXT DIR     //
    // /////////////////////////// //

    contextDir = await mkdtemp(
      resolve(tmpdir(), 'seraphim-task-'),
    )
    console.debug('Created Docker build context directory', {
      contextDir,
    })

    if (!existsSync(contextDir)) {
      mkdirSync(contextDir, { recursive: true })
    }

    if (!existsSync(utilsDir)) {
      console.debug('Utils resources directory not found', {
        utilsDir,
      })
      throw new Error('Utils resources directory is missing')
    }

    console.debug('Copying Docker build context resources', {
      from: utilsDir,
      to: contextDir,
    })

    const utilsTargetDir = resolve(contextDir, 'utils')
    mkdirSync(utilsTargetDir, { recursive: true })

    // Copy *contents* of resourcesDir into contextDir by copying the utils folder from within resourcesDir
    cpSync(utilsDir, utilsTargetDir, {
      recursive: true,
      force: true, // overwrite existing files
      errorOnExist: false,
    })

    console.debug('Initializing security for stdout redaction')
    const [ secrets, userSettings ] = await Promise.all([
      getSecrets(),
      // TODO: Include user settings in task with full context!
      prisma.userSettings.findFirst({
        select: {
          id: true,
          customAgentInstructions: true,
          customAgentsFile: true,
        },
      }),
      mkdir(
        resolve(contextDir, 'docs'),
      ),
    ])

    let customAgentInstructionsContent: string = userSettings.customAgentInstructions || ''
    if (userSettings.customAgentsFile) {
      try {
        const AGENTSmd = await readFile(userSettings.customAgentsFile, 'utf-8')
        customAgentInstructionsContent = AGENTSmd
      }
      catch (error) {
        console.error('Failed to read custom agents file', {
          error,
          file: userSettings.customAgentsFile,
        })
      }
      try {
        const AGENTSDocsDir = resolve(
          dirname(userSettings.customAgentsFile),
          'docs',
        )
        if (existsSync(AGENTSDocsDir)) {
          // Copy the docs directory if it exists into the context dir as <contextDir>/docs
          cpSync(
            AGENTSDocsDir,
            resolve(contextDir, 'docs'),
            {
              recursive: true,
              force: true,
              errorOnExist: false,
            },
          )
          console.debug('Copied custom agents docs directory into Docker build context', {
            from: AGENTSDocsDir,
            to: resolve(contextDir, 'docs'),
          })
        }
      }
      catch (error) {
        console.error('Failed to read custom agents docs directory', {
          error,
          dir: userSettings.customAgentsFile,
        })
      }
    }

    console.debug('Writing Dockerfile, setup script, and validate script to context directory...')

    await Promise.all([
      writeSetupScriptFile(
        contextDir,
        task,
        cloner,
        secrets,
      ),
      writeFile(
        resolve(
          contextDir,
          VALIDATE_SCRIPT_NAME,
        ),
        workspace.postScript,
      ),
      writeFile(
        resolve(contextDir, 'Dockerfile'),
        dockerfileContents,
        'utf-8',
      ),
      writeFile(
        resolve(contextDir, 'codex_config.toml'),
        createCodexConfig(task!),
        'utf-8',
      ),
      writeFile(
        resolve(contextDir, 'codex_auth.json'),
        createCodexAuthFile(task!),
        'utf-8',
      ),
      writeFile(
        resolve(contextDir, 'AGENTS.md'),
        customAgentInstructionsContent,
        'utf-8',
      ),
    ])

    const contextDirFiles = await readdir(
      contextDir,
      {
        withFileTypes: true,
        recursive: true,
      },
    )

    console.debug(
      `Context directory files after setup: ${contextDir}`,
      contextDirFiles.map((file) => relative(contextDir, file.parentPath)),
    )

    // /////////////////////////// //
    //       BUILD THE IMAGE       //
    // /////////////////////////// //

    console.debug('Pulling the base Docker image...')
    const pullOutput = await pullWithProgress(DEFAULT_DOCKER_BASE_IMAGE)
    if (pullOutput && toStdout) {
      toStdout(pullOutput)
    }

    console.debug('Starting Docker image build...')

    const buildStream = await dockerClient.buildImage(
      {
        context: contextDir,
        src: contextFiles,
      },
      {
        t: buildTag,
        pull: true,
        // Specify if we should use BuildKit for better performance and output
        version: DOCKER_USE_BUILDKIT
          ? '2'
          : undefined,
      },
    )

    console.debug('Waiting for the Docker build to complete...')

    // Wait for the build to complete!
    if (DOCKER_USE_BUILDKIT) {
      const buildOutput = await waitForBuildVersion2(buildStream, dockerClient)
      if (buildOutput && toStdout) {
        toStdout(buildOutput)
      }
    }
    else {
      const buildOutput = await waitForBuildVersion1(buildStream, dockerClient)
      if (buildOutput && toStdout) {
        toStdout(buildOutput)
      }
    }

    console.debug('Docker build completed successfully')

    return {
      success: true,
      imageTag: buildTag,
    }
  }
  catch (error) {
    console.debug('Error during image build', {
      error,
    })

    const dockerBuildOutput = typeof error === 'object' && error !== null
      ? Reflect.get(error, 'output')
      : null

    if (typeof dockerBuildOutput === 'string' && dockerBuildOutput.trim().length > 0 && toStdout) {
      toStdout(dockerBuildOutput)
    }

    const message = (error as Error).message || 'Unknown error during image build'

    if (toStdout) {
      toStdout(message)
    }

    return {
      success: false,
      errors: [ message ],
    }
  }
  finally {
    // Cleanup the context dir!
    await rm(
      contextDir,
      {
        recursive: true,
        force: true,
      },
    )
  }
}
