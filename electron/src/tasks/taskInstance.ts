// Copyright © 2026 Jalapeno Labs

import type { Message, Prisma, Turn } from '@prisma/client'
import type { LlmUsage, TaskWithFullContext } from '@common/types'
import type { Container } from 'dockerode'
import type { SetOptional } from 'type-fest'
import type { ClientRequest, ClientNotification, ServerNotification } from '@common/vendor/codex-protocol'
import type {
  RateLimitSnapshot,
  ThreadTokenUsage,
  ThreadStartResponse,
} from '@common/vendor/codex-protocol/v2'

// Core
import { requireDatabaseClient } from '@electron/database'
import packageJson from '../../package.json'
import chalk from 'chalk'
import stripAnsi from 'strip-ansi'

// Docker
import { getDockerClient } from '@electron/docker/docker'
import { getDockerSocketMount } from '@electron/docker/dockerSocket'
import { teardownTask } from '@electron/jobs/teardownTask'
import { buildImage } from '@electron/docker/buildImage'
import { getCloner } from '@common/cloning/getCloner'

// Node.js
import { EventEmitter } from 'node:events'
import { createInterface } from 'node:readline'
import { PassThrough } from 'node:stream'

// Utility
import { convertEnvironmentToStringArray } from '@common/envKit'
import { broadcastSseChange } from '@electron/api/sse/sseEvents'
import { safeParseJson } from '@common/json'
import { CODEX_WORKDIR, SETUP_FAILURE_LINE, SETUP_SUCCESS_LINE } from '@common/constants'

type TaskInstanceOptions = {
  task: TaskWithFullContext
  containerExists: boolean
}

type InboundCodexEvent = ServerNotification

type EventMap = {
  'stdout': [ Buffer ]
  'stderr': [ Buffer ]
  'line': [ string ]
  'message': [ InboundCodexEvent ]
}

type ExecuteCommandResult = {
  command: string
  stdout: string
  stderr: string
  stdall: string
  exitCode: number
}

type CreateMessage = SetOptional<Omit<Message, 'id' | 'createdAt' | 'taskId' | 'turnId'>, 'media' | 'meta' | 'start'>

export class TaskInstance extends EventEmitter<EventMap> {
  private task: TaskWithFullContext
  private containerExists: boolean
  private container: Container | null = null
  private isAttached = false

  private codexThreadId: string | null = null
  private codexStream: NodeJS.ReadWriteStream | null = null
  private nextRequestId: number = 1

  private rateLimits: RateLimitSnapshot | null = null
  private usage: ThreadTokenUsage | null = null
  private userMessageQueue: CreateMessage[] = []
  private codexTurnId: string | null = null

  constructor(options: TaskInstanceOptions) {
    super()
    this.task = options.task
    this.containerExists = options.containerExists
    this.codexThreadId = options.task.threadId
  }

  get id() {
    return this.task.id
  }

  get data() {
    return this.task
  }

  get usageData(): LlmUsage {
    return {
      taskId: this.task.id,
      llmId: this.task.llm.id,
      usage: this.usage,
      rateLimits: this.rateLimits,
    }
  }

  get containerId(): string | null {
    const containerId = this.task.container?.trim()
    if (!containerId) {
      console.debug('TaskInstance container id not found', {
        taskId: this.task.id,
        containerId,
      })
      return null
    }

    return containerId
  }

  get hasContainer() {
    return this.containerExists
  }

  private getRequestId(): number {
    return this.nextRequestId++
  }

  // //////////////////////// //
  //        Public API        //
  // //////////////////////// //

  public async refreshContainerStatus(): Promise<boolean> {
    const containerId = this.containerId
    if (!containerId) {
      console.debug('TaskInstance missing container id during refresh', {
        taskId: this.task.id,
        containerId,
      })
      this.containerExists = false
      return false
    }

    try {
      const dockerClient = getDockerClient()
      this.container ||= dockerClient.getContainer(containerId)
      await this.container.inspect()

      this.containerExists = true
      return true
    }
    catch (error) {
      console.debug('Container existence check failed', {
        containerId,
        error,
      })

      this.containerExists = false
      return false
    }
  }

  public async createContainer(): Promise<this> {
    try {
      const dockerClient = getDockerClient()
      if (!dockerClient) {
        throw new Error('Docker client is not available')
      }

      const prisma = requireDatabaseClient('TaskInstance createContainer')

      const {
        workspace,
        gitAccount,
      } = this.task

      const sourceRepoUrl = workspace.sourceRepoUrl?.trim()
      if (!sourceRepoUrl) {
        console.debug('TaskInstance missing workspace source repo URL', {
          taskId: this.task.id,
          workspaceId: workspace.id,
        })
        throw new Error('Workspace source repository is required')
      }

      const [ initialTurn ] = await Promise.all([
        prisma.turn.create({
          data: {
            taskId: this.task.id,
          },
          include: {
            messages: true,
          },
        }),
        this.updateTask({
          state: 'Creating',
        }),
      ])

      this.task.turns.push(initialTurn)

      const cloner = getCloner(
        gitAccount.provider,
        sourceRepoUrl,
        gitAccount.accessToken,
      )

      const canClone = await cloner.checkIfCanClone()

      if (!canClone) {
        console.error('Cannot clone repository with provided URL and credentials', {
          repository: workspace.sourceRepoUrl,
          authProvider: gitAccount.provider,
        })
        throw new Error('Cannot clone repository with provided URL and credentials')
      }

      const buildImageResult = await buildImage(
        workspace,
        this.task,
        cloner,
        (stdout: string) => this.saveCodexMessage({
          role: 'Docker',
          type: 'text',
          content: stdout,
        }),
      )

      if (!buildImageResult.success) {
        console.error('Image build failed', {
          errors: buildImageResult.errors,
        })
        throw new Error(`Image build failed: ${buildImageResult.errors?.join('; ')}`)
      }

      const Image = buildImageResult.imageTag
      const Env = convertEnvironmentToStringArray(workspace.envEntries)

      Env.push(`CODEX_HOME=${CODEX_WORKDIR}`)

      const socketMount = getDockerSocketMount()
      const volumes = []
      if (socketMount) {
        volumes.push(`${socketMount.source}:${socketMount.target}`)
      }

      console.debug('Creating Docker container for task')

      const abortController = new AbortController()
      const buildCompleteScriptPromise = this.waitForStdout(SETUP_SUCCESS_LINE, -1, abortController.signal)
      const buildFailureScriptPromise = this.waitForStdout(SETUP_FAILURE_LINE, -1, abortController.signal)

      this.container = await dockerClient.createContainer({
        name: this.task.containerName,
        Image,
        Env,
        HostConfig: {
          Binds: volumes,
          NetworkMode: 'host',
        },
        // The following are necessary for codex app-server to work properly
        Tty: false,
        OpenStdin: true,
        AttachStdin: true,
        StdinOnce: false,
        AttachStderr: true,
        AttachStdout: true,
      })

      console.debug('Docker container created for task', {
        containerId: this.container.id,
        containerName: this.task.containerName,
        taskId: this.task.id,
      })

      await this.updateTask({
        container: this.container.id,
        state: 'SettingUp',
      })

      this.containerExists = true

      console.debug('Starting Docker container for task')

      let setupScriptOutput: string = ''
      function onStdout(line: string) {
        setupScriptOutput += stripAnsi(line) + '\n'
      }

      this.on('line', onStdout)

      await this.container.start()
      await this.attachToContainer()

      // This only guarantees that the setup script has completed
      // This does *NOT* guarantee that the `codex app-server` process has fully started.
      // Typically it's started immediately AFTER this gets logged
      const completed = await Promise.race([
        buildCompleteScriptPromise
          .then(() => ({ type: 'success' as const }))
          .catch((error) => ({ type: 'success-error' as const, error })),
        buildFailureScriptPromise
          .then(() => ({ type: 'failure' as const }))
          .catch((error) => ({ type: 'failure-error' as const, error })),
      ])

      const now = Date.now()
      await Promise.all([
        this.saveCodexMessage({
          role: 'Docker',
          type: 'text',
          content: setupScriptOutput,
        }),
        this.updateTurn(initialTurn.id, {
          timeTaken: now - initialTurn.createdAt.getTime(),
          finishedAt: new Date(now),
        }),
      ])

      this.off('line', onStdout)

      if (completed.type !== 'success') {
        throw new Error(`Setup script did not complete successfully: ${completed.type}`)
      }

      await this.startCodexAppServer()
    }
    catch (error) {
      console.log('TaskInstance createContainer failed', {
        taskId: this.task.id,
        error,
      })
      await this.updateTask({
        state: 'Failed',
      })
    }

    return this
  }

  public async teardown(): Promise<void> {
    try {
      await teardownTask(this.task.container)
    }
    catch (error) {
      console.error(error)
    }
    finally {
      const databaseClient = requireDatabaseClient('TaskInstance delete')
      await databaseClient.$transaction(async (transaction) => {
        await transaction.message.deleteMany({
          where: { taskId: this.task.id },
        })

        await transaction.task.delete({
          where: { id: this.task.id },
        })
      })
    }
  }

  public async attachToContainer(): Promise<void> {
    if (this.isAttached) {
      console.debug('TaskInstance attach requested but already attached', {
        taskId: this.task.id,
      })
      return
    }

    const containerId = this.task.container?.trim()
    if (!containerId) {
      console.debug('TaskInstance attach requested without container id', {
        taskId: this.task.id,
      })
      return
    }

    const dockerClient = getDockerClient()
    if (!dockerClient) {
      console.debug('TaskInstance attach requested without docker client', {
        taskId: this.task.id,
        containerId,
      })
      return
    }

    this.container ||= dockerClient.getContainer(containerId)
    const stream = await this.container.attach({
      stream: true,
      stdin: true,
      stdout: true,
      stderr: true,
    })

    const stdoutStream = new PassThrough()
    const stderrStream = new PassThrough()

    dockerClient.modem.demuxStream(stream, stdoutStream, stderrStream)

    this.isAttached = true

    const stdoutReader = createInterface({
      input: stdoutStream,
    })

    function handleStdoutLine(this: TaskInstance, line: string) {
      this.emit('line', line.toString())

      const parsedMessage = safeParseJson<InboundCodexEvent>(line, undefined, true)
      if (parsedMessage) {
        this.emit('message', parsedMessage)
      }
      else if (line.startsWith('{')) {
        console.debug('Failed to parse task container stdout line as JSON', {
          taskId: this.task.id,
          line,
        })
      }
      else {
        console.info(line)
      }
    }

    function handleStdoutData(this: TaskInstance, chunk: Buffer) {
      this.emit('stdout', chunk)
    }

    function handleStderrData(this: TaskInstance, chunk: Buffer) {
      this.emit('stderr', chunk)
      console.debug('Container STDERR', chunk?.toString('utf-8'))
    }

    function handleStreamError(this: TaskInstance, error: Error) {
      console.error('TaskInstance stream error', {
        taskId: this.task.id,
        containerId,
        error,
      })
    }

    stdoutReader.on('line', handleStdoutLine.bind(this))
    stdoutStream.on('data', handleStdoutData.bind(this))
    stderrStream.on('data', handleStderrData.bind(this))
    stream.on('error', handleStreamError.bind(this))
  }

  public sendRequest(message: ClientRequest | ClientNotification): void {
    if (!this.codexStream) {
      console.debug('TaskInstance sendMessage requested without stream', {
        taskId: this.task.id,
      })
      return
    }

    let id: string = ''
    if ('id' in message) {
      id = ` (#${String(message.id)}) `
    }

    console.debug(`US-> CODEX ${message.method}${id}`, message)
    const payload = `${JSON.stringify(message)}\n`
    this.codexStream.write(payload)
  }

  // TODO
  // public async halt(): Promise<void> {

  // }

  // // TODO
  // public async resume(): Promise<void> {

  // }

  public async queueUserMessage(message: CreateMessage): Promise<void> {
    const isQueueEmpty = this.userMessageQueue.length === 0
    this.userMessageQueue.push(message)

    if (isQueueEmpty) {
      this.doNextQueue()
    }
  }

  public async interruptAndStop(): Promise<boolean> {
    this.userMessageQueue = []

    const codexThreadId = this.codexThreadId?.trim()
    if (!codexThreadId) {
      console.debug('TaskInstance interrupt requested without thread id', {
        taskId: this.task.id,
      })
      return false
    }

    const codexTurnId = this.codexTurnId?.trim()
    if (!codexTurnId) {
      console.debug('TaskInstance interrupt requested without active turn id', {
        taskId: this.task.id,
        codexThreadId,
      })
      return false
    }

    const interruptId = this.getRequestId()
    this.sendRequest({
      method: 'turn/interrupt',
      id: interruptId,
      params: {
        threadId: codexThreadId,
        turnId: codexTurnId,
      },
    })

    await this.waitForResponse(interruptId)

    this.codexTurnId = null
    await this.updateTask({
      state: 'AwaitingReview',
    })

    return true
  }

  // //////////////////////// //
  //       Internal API       //
  // //////////////////////// //

  private hasNextQueue() {
    return this.userMessageQueue.length > 0
  }

  private async doNextQueue() {
    const prisma = requireDatabaseClient('TaskInstance queueUserMessage')

    const nextQueueItem = this.userMessageQueue.shift()
    if (!nextQueueItem) {
      if (this.task.state !== 'AwaitingReview') {
        await this.updateTask({
          state: 'AwaitingReview',
        })
      }
      return
    }

    const newTurn = await prisma.turn.create({
      data: {
        taskId: this.task.id,
      },
      include: {
        messages: true,
      },
    })

    const newMessage = await prisma.message.create({
      data: {
        role: nextQueueItem.role,
        type: nextQueueItem.type,
        content: nextQueueItem.content,
        media: nextQueueItem.media || [],
        taskId: this.task.id,
        turnId: newTurn.id,
      },
    })

    newTurn.messages = [ newMessage ]
    this.task.turns.push(newTurn)

    await this.updateTask({
      state: 'Working',
    })

    const turnStartId = this.getRequestId()
    this.sendRequest({
      method: 'turn/start',
      id: turnStartId,
      params: {
        threadId: this.codexThreadId,
        input: [{
          type: 'text',
          text: nextQueueItem.content,
          text_elements: [],
        }],
      },
    })
  }

  private async saveCodexMessage(message: CreateMessage): Promise<Message> {
    const prisma = requireDatabaseClient('TaskInstance saveCodexMessage')
    const currentTurn = this.task.turns[this.task.turns.length - 1]

    const newMessage = await prisma.message.create({
      data: {
        turnId: currentTurn.id,
        taskId: this.task.id,
        media: message?.media || [],
        meta: message?.meta || {},
        start: message?.start ?? false,
        ...message,
      },
    })

    this.task.turns[this.task.turns.length - 1].messages.push(newMessage)

    return newMessage
  }

  public async executeCmd(command: string): Promise<ExecuteCommandResult> {
    const containerId = this.containerId
    if (!containerId) {
      console.debug('TaskInstance executeCmd requested without container id', {
        taskId: this.task.id,
        command,
      })
      throw new Error('Cannot execute command without a container id')
    }

    const dockerClient = getDockerClient()
    if (!dockerClient) {
      console.debug('TaskInstance executeCmd requested without docker client', {
        taskId: this.task.id,
        containerId,
        command,
      })
      throw new Error('Docker client is not available')
    }

    this.container ||= dockerClient.getContainer(containerId)
    const execInstance = await this.container.exec({
      AttachStdout: true,
      AttachStderr: true,
      Cmd: [ 'bash', '-lc', command ],
    })

    const stdoutStream = new PassThrough()
    const stderrStream = new PassThrough()
    const stdoutChunks: Buffer[] = []
    const stderrChunks: Buffer[] = []
    const stdallChunks: Buffer[] = []

    function handleStdoutData(chunk: Buffer) {
      stdoutChunks.push(chunk)
      stdallChunks.push(chunk)
    }

    function handleStderrData(chunk: Buffer) {
      stderrChunks.push(chunk)
      stdallChunks.push(chunk)
    }

    function handleStreamError(this: TaskInstance, error: Error) {
      console.debug('TaskInstance executeCmd stream error', {
        taskId: this.task.id,
        containerId,
        command,
        error,
      })
    }

    stdoutStream.on('data', handleStdoutData)
    stderrStream.on('data', handleStderrData)

    const executionStream = await execInstance.start({
      hijack: true,
      stdin: false,
    })

    executionStream.on('error', handleStreamError.bind(this))
    dockerClient.modem.demuxStream(executionStream, stdoutStream, stderrStream)

    await new Promise<void>((resolve, reject) => {
      const done = () => resolve()
      executionStream.once('end', done)
      executionStream.once('close', done)
      executionStream.once('error', reject)
    })

    const inspectionResult = await execInstance.inspect()
    const exitCode = inspectionResult.ExitCode ?? -1
    const stdout = Buffer.concat(stdoutChunks).toString('utf8')
    const stderr = Buffer.concat(stderrChunks).toString('utf8')
    const stdall = Buffer.concat(stdallChunks).toString('utf8')

    return {
      command,
      stdout,
      stderr,
      stdall,
      exitCode,
    }
  }

  private subscription: ((message: InboundCodexEvent) => void) | null = null
  protected attachCodexSubscriptions() {
    if (this.subscription) {
      this.removeListener('message', this.subscription)
    }

    const handleMessage = async (message: InboundCodexEvent) => {
      const { method, params } = message

      if (!method) {
        if ('result' in message && !params) {
          // Return silently, this is okay
          // This is typically just a turn/start response
          return
        }

        console.debug('Received Codex message without method', {
          taskId: this.task.id,
          message,
        })
        return
      }

      // Notes:
      // "item" = one atomic thing the agent produced
      // "turn" = the whole “job” kicked off by one user input
      switch (method) {
        case 'turn/started':
          this.codexTurnId = params.turn.id
          await this.updateTask({
            state: 'Working',
          })
          break
        case 'turn/completed':
          this.codexTurnId = null
          console.log(
            chalk.green('Task completed!!'),
          )
          console.log('Rate limits:', this.rateLimits)
          console.log('Usage:', this.usage)
          await this.onTurnComplete()
          break
        case 'turn/diff/updated':
          await this.saveCodexMessage({
            role: 'Codex',
            type: 'diff',
            content: params.diff,
            meta: params,
          })
          break
        case 'item/started':
          if (params.item.type === 'agentMessage') {
            if (!params.item.text) {
              break
            }
            console.debug(`CODEX: ${params.item.text}`)
            await this.saveCodexMessage({
              start: true,
              role: 'Codex',
              type: 'agentMessage',
              content: params.item.text,
              meta: params,
            })
          }
          else if (params.item.type === 'commandExecution') {
            if (!params.item.command) {
              break
            }
            console.debug(`CODEX: Started ${params.item.command}`)
            await this.saveCodexMessage({
              start: true,
              role: 'Codex',
              type: 'reasoning',
              content: params.item.command,
              meta: params,
            })
          }
          else if (params.item.type === 'reasoning') {
            if (!params.item.summary?.length) {
              break
            }
            console.debug(`CODEX: Started ${params.item.summary.join('\n')}`)
            await this.saveCodexMessage({
              start: true,
              role: 'Codex',
              type: 'reasoning',
              content: params.item.summary.join('\n'),
              meta: params,
            })
          }
          else if (params.item.type !== 'userMessage') {
            console.debug(`CODEX: Started ${params.item.type}`)
            await this.saveCodexMessage({
              start: true,
              role: 'Codex',
              type: params.item.type,
              content: params.item.id,
              meta: params,
            })
          }
          break
        case 'item/completed':
          if (params.item.type === 'agentMessage') {
            if (!params.item.text) {
              break
            }
            console.debug(`CODEX: ${params.item.text}`)
            await this.saveCodexMessage({
              role: 'Codex',
              type: 'agentMessage',
              content: params.item.text,
              meta: params,
            })
          }
          else if (params.item.type === 'commandExecution') {
            if (!params.item.command) {
              break
            }
            console.debug(`CODEX: Completed ${params.item.command}`)
            await this.saveCodexMessage({
              role: 'Codex',
              type: 'reasoning',
              content: params.item.command,
              meta: params,
            })
          }
          else if (params.item.type === 'reasoning') {
            if (!params.item.summary?.length) {
              break
            }
            console.debug(`CODEX: Completed ${params.item.summary.join('\n')}`)
            await this.saveCodexMessage({
              role: 'Codex',
              type: 'reasoning',
              content: params.item.summary.join('\n'),
              meta: params,
            })
          }
          else if (params.item.type !== 'userMessage') {
            console.debug(`CODEX: Completed ${params.item.type}`)
            await this.saveCodexMessage({
              role: 'Codex',
              type: params.item.type,
              content: params.item.id,
              meta: params,
            })
          }
          break
        case 'item/fileChange/outputDelta':
        case 'item/reasoning/summaryTextDelta':
        case 'item/agentMessage/delta':
          // For partial eager streaming of agent messages
          break
        case 'account/rateLimits/updated':
          this.rateLimits = params.rateLimits
          broadcastSseChange({
            type: 'update',
            kind: 'usage',
            data: this.usageData,
          })
          break
        case 'thread/tokenUsage/updated':
          this.usage = params.tokenUsage
          broadcastSseChange({
            type: 'update',
            kind: 'usage',
            data: this.usageData,
          })
          break
        case 'thread/compacted':
          await this.saveCodexMessage({
            role: 'Codex',
            type: 'threadCompacted',
            content: 'Thread context has been compacted',
            meta: params,
          })
        case 'item/reasoning/summaryPartAdded':
        case 'thread/started':
          // Not much do to here yet! Added here so they're muted from console.
          break
        default:
          if (method.startsWith('codex/event/')) {
            // These are legacy events that codex app-server still emits
            // but we don't need to do anything with them in seraphim
            break
          }

          console.debug('Unhandled Codex message response:', message)
      }
    }

    this.addListener('message', handleMessage)
    this.subscription = handleMessage
  }

  public async startCodexAppServer(): Promise<void> {
    const execInstance = await this.container.exec({
    Cmd: [ 'bash', '-lc', 'codex app-server 2> >(tee /proc/1/fd/2 >&2) | tee /proc/1/fd/1' ],
      AttachStdin: true,
      AttachStdout: true,
      AttachStderr: true,
      Tty: false,
    })

    const stream = await execInstance.start({
      hijack: true,
      stdin: true,
    })

    // stream is multiplexed stdout/stderr when Tty=false
    // const stdoutStream = new PassThrough()
    // const stderrStream = new PassThrough()
    // getDockerClient().modem.demuxStream(stream, stdoutStream, stderrStream)

    // IMPORTANT: write requests to `stream` (the hijacked duplex)
    this.codexStream = stream

    // // Read responses from stdoutStream line-by-line
    // const stdoutReader = createInterface({ input: stdoutStream })
    // stdoutReader.on('line', (line) => {
    //   const parsed = safeParseJson(line, undefined, true)
    //   console.log('>>> ' + line)
    //   if (parsed) {
    //     this.emit('message', parsed)
    //   }
    //   else {
    //     this.emit('line', line)
    //   }
    // })

    // // stderr is useful for debugging
    // const stderrReader = createInterface({ input: stderrStream })
    // stderrReader.on('line', (line) => {
    //   this.emit('line', `[codex:stderr] ${line}`)
    //   console.error(`[codex:stderr] ${line}`)
    // })

    const initId = this.getRequestId()
    const initPromise = this.waitForResponse(initId)
    this.sendRequest({
      method: 'initialize',
      id: initId,
      params: {
        clientInfo: {
          name: 'seraphim',
          title: 'Seraphim',
          version: packageJson.version,
        },
        capabilities: {
          experimentalApi: false,
        },
      },
    })
    await initPromise

    this.sendRequest({ method: 'initialized' })

    console.debug('Beginning task codex work...')

    const threadStartId = this.getRequestId()
    this.sendRequest({
      method: 'thread/start',
      id: threadStartId,
      params: {
        experimentalRawEvents: false,
        model: this.task.llm.preferredModel,
      },
    })

    const threadResult = await this.waitForResponse<ThreadStartResponse>(threadStartId)

    console.debug('Received thread/start response from Codex', {
      threadId: threadResult.thread.id,
    })

    this.codexThreadId = threadResult.thread.id
    await this.updateTask({
      threadId: threadResult.thread.id,
      state: 'AwaitingReview',
    })

    this.attachCodexSubscriptions()
  }

  private async waitForResponse<Type = any>(id: number, timeoutMs = 180_000): Promise<Type> {
    return new Promise((resolve, reject) => {
      let timeout: ReturnType<typeof setTimeout> | undefined

      if (timeoutMs > 0) {
        timeout = setTimeout(() => {
          this.off('message', onMessage)
          reject(new Error(`Timeout waiting for response id=${id}`))
        }, timeoutMs)
      }

      const onMessage = (message: any) => {
        if (!message || typeof message !== 'object') {
          return
        }
        if (message.id !== id) {
          return
        }

        clearTimeout(timeout)
        this.off('message', onMessage)

        if (message.error) {
          reject(message.error)
        }
        else {
          resolve(message.result)
        }
      }

      this.on('message', onMessage)
    })
  }

  private async waitForStdout<Type = any>(
    line: string,
    timeoutMs = 30_000,
    signal?: AbortSignal,
  ): Promise<Type> {
    return new Promise((resolve, reject) => {
      let timeout: ReturnType<typeof setTimeout> | undefined

      if (timeoutMs > 0) {
        timeout = setTimeout(() => {
          this.off('line', onMessage)
          reject(new Error(`Timeout waiting for stdout line=${line}`))
        }, timeoutMs)
      }

      const normalizedLine = line.trim().toLowerCase()

      const onMessage = (message: any) => {
        if (!message || !message?.toLowerCase()?.includes(normalizedLine)) {
          return
        }

        clearTimeout(timeout)
        this.off('line', onMessage)

        if (message.error) {
          reject(message.error)
        }
        else {
          resolve(message.result)
        }
      }

      const onAbort = () => {
        clearTimeout(timeout)
        this.off('line', onMessage)
        resolve(undefined as any)
      }

      // If already aborted, finish immediately
      if (signal?.aborted) {
        onAbort()
        return
      }

      this.on('line', onMessage)
      signal?.addEventListener('abort', onAbort, { once: true })
    })
  }

  private async updateTurn(id: string, data: Prisma.TurnUpdateArgs['data']): Promise<Turn> {
    const prisma = requireDatabaseClient('TaskInstance updateTurn')
    return await prisma.turn.update({
      where: { id: id },
      data,
    })
  }

  public async updateTask(data: Prisma.TaskUpdateArgs['data']): Promise<TaskWithFullContext> {
    const databaseClient = requireDatabaseClient('TaskInstance updateSelf')

    const updatedTask = await databaseClient.task.update({
      where: { id: this.task.id },
      data,
    })

    broadcastSseChange({
      kind: 'tasks',
      type: 'update',
      data: updatedTask,
    })

    const hydratedTask = Object.assign(this.task, updatedTask)

    this.task = hydratedTask

    return hydratedTask
  }

  // //////////////////////// //
  //     On task completed    //
  // //////////////////////// //

  private async onTurnComplete(): Promise<void> {
    // Queued user messages always come before automated validation
    if (this.hasNextQueue()) {
      await this.doNextQueue()
      return
    }

    const previousTurn = this.task.turns[this.task.turns.length - 1]
    if (!previousTurn) {
      console.warn('[WEIRD?] TaskInstance onTurnComplete called without an active turn', {
        taskId: this.task.id,
      })
      await this.doNextQueue()
      return
    }

    const now = Date.now()
    await this.updateTurn(
      previousTurn.id,
      {
        timeTaken: now - previousTurn.createdAt.getTime(),
        finishedAt: new Date(now),
      },
    )

    const postRunCommands = this.task.workspace.postScript
      ?.split(/\r?\n/)
      ?.map((line) => line.trim())
      ?.filter((line) => Boolean(line))
      || []

    if (postRunCommands.length) {
      await this.updateTask({
        state: 'Validating',
      })

      const failedCommands: ExecuteCommandResult[] = []

      // Test each command...
      for (const command of postRunCommands) {
        let commandResult: ExecuteCommandResult = {
          command,
          stdout: '',
          stderr: '',
          stdall: '',
          exitCode: -1,
        }

        try {
          commandResult = await this.executeCmd(command)
        }
        catch (error) {
          console.debug('TaskInstance failed to execute post-run validation command', {
            taskId: this.task.id,
            command,
            error,
          })

          commandResult.stderr = String(error)
        }

        if (commandResult.exitCode !== 0) {
          console.debug('TaskInstance post-run validation command failed', {
            taskId: this.task.id,
            command: commandResult.command,
            exitCode: commandResult.exitCode,
          })

          failedCommands.push(commandResult)
        }
      }

      // If any of the commands failed...
      if (failedCommands.length) {
        const commandFailureMessage: string[] = [
          failedCommands.length === 1
            ? 'A post-run validation command failed.'
            : 'Some post-run validation commands failed.',
          'Please fix the workspace so this validation command passes.',
        ]

        for (const result of failedCommands) {
          commandFailureMessage.push(`Command: ${result.command}`)
          commandFailureMessage.push(`Exit code: ${result.exitCode}`)
          if (result.stdall?.length) {
            commandFailureMessage.push(`Stdout:\n\`\`\`\n${result.stdout}\n\`\`\``)
          }
        }

        this.userMessageQueue.push({
          role: 'System',
          type: 'text',
          content: commandFailureMessage
            .filter(Boolean)
            .join('\n'),
        })
      }
    }

    await this.doNextQueue()
  }
}
