// Copyright © 2026 Jalapeno Labs

import type { VoiceServerMessage } from '@common/voice/protocol'

export type Options = {
  websocketUrl: string
  onWords: (words: string) => void
  onError: (errorMessage: string) => void
  onActiveChange: (isActive: boolean) => void
}

const VoiceChunkMilliseconds = 1500 as const

export class FrontendVoiceSession {
  private readonly websocketUrl: string
  private readonly onWords: (words: string) => void
  private readonly onError: (errorMessage: string) => void
  private readonly onActiveChange: (isActive: boolean) => void

  private websocket: WebSocket | null = null
  private mediaRecorder: MediaRecorder | null = null
  private mediaStream: MediaStream | null = null

  constructor(options: Options) {
    this.websocketUrl = options.websocketUrl
    this.onWords = options.onWords
    this.onError = options.onError
    this.onActiveChange = options.onActiveChange
  }

  public async start(): Promise<void> {
    if (this.mediaRecorder?.state === 'recording') {
      return
    }

    this.mediaStream = await navigator.mediaDevices.getUserMedia({ audio: true })
    this.websocket = await this.openSocket()

    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        cleanup()
        reject(new Error('Voice websocket did not become ready in time'))
      }, 5000)

      const cleanup = () => {
        clearTimeout(timeout)
        this.websocket?.removeEventListener('message', handleReadyMessage)
        this.websocket?.removeEventListener('close', handleEarlyClose)
      }

      const handleReadyMessage = (event: MessageEvent) => {
        const payload = this.parseServerMessage(event.data)
        if (!payload) {
          return
        }

        if (payload.type === 'ready') {
          cleanup()
          resolve()
          return
        }

        if (payload.type === 'error') {
          cleanup()
          reject(new Error(payload.message))
        }
      }

      const handleEarlyClose = () => {
        cleanup()
        reject(new Error('Voice websocket closed before ready signal'))
      }

      this.websocket?.addEventListener('message', handleReadyMessage)
      this.websocket?.addEventListener('close', handleEarlyClose)
    })

    const preferredMimeType = this.getPreferredRecorderMimeType()
    this.mediaRecorder = preferredMimeType
      ? new MediaRecorder(this.mediaStream, { mimeType: preferredMimeType })
      : new MediaRecorder(this.mediaStream)

    this.mediaRecorder.addEventListener('dataavailable', (event) => {
      void this.sendAudioChunk(event.data)
    })

    this.websocket.addEventListener('message', (event) => {
      const payload = this.parseServerMessage(event.data)
      if (!payload) {
        return
      }

      if (payload.type === 'words') {
        this.onWords(payload.words)
        return
      }

      if (payload.type === 'error') {
        this.onError(payload.message)
      }
    })

    this.websocket.addEventListener('close', () => {
      this.stop()
    })

    this.mediaRecorder.start(VoiceChunkMilliseconds)
    this.onActiveChange(true)
  }

  public stop(): void {
    if (this.mediaRecorder?.state !== 'inactive') {
      this.mediaRecorder.stop()
    }

    if (this.mediaStream) {
      for (const track of this.mediaStream.getTracks()) {
        track.stop()
      }
    }

    if (
      this.websocket
      && this.websocket.readyState !== WebSocket.CLOSED
      && this.websocket.readyState !== WebSocket.CLOSING
    ) {
      this.websocket.close()
    }

    this.websocket = null
    this.mediaRecorder = null
    this.mediaStream = null
    this.onActiveChange(false)
  }

  private async openSocket(): Promise<WebSocket> {
    return await new Promise<WebSocket>((resolve, reject) => {
      const websocket = new WebSocket(this.websocketUrl)

      const handleOpen = () => {
        cleanup()
        resolve(websocket)
      }

      const handleError = () => {
        cleanup()
        reject(new Error('Voice websocket failed to open'))
      }

      const cleanup = () => {
        websocket.removeEventListener('open', handleOpen)
        websocket.removeEventListener('error', handleError)
      }

      websocket.addEventListener('open', handleOpen)
      websocket.addEventListener('error', handleError)
    })
  }

  private async sendAudioChunk(audioChunk: Blob): Promise<void> {
    if (!audioChunk || audioChunk.size === 0) {
      return
    }

    if (!this.websocket || this.websocket.readyState !== WebSocket.OPEN) {
      return
    }

    const bytes = new Uint8Array(await audioChunk.arrayBuffer())
    let binaryString = ''
    for (const byte of bytes) {
      binaryString += String.fromCharCode(byte)
    }

    this.websocket.send(JSON.stringify({
      type: 'audio-chunk',
      mimeType: audioChunk.type || 'audio/webm',
      dataBase64: btoa(binaryString),
    }))
  }

  private getPreferredRecorderMimeType(): string | undefined {
    const candidateMimeTypes = [
      'audio/webm;codecs=opus',
      'audio/webm',
      'audio/mp4',
    ] as const

    for (const candidateMimeType of candidateMimeTypes) {
      if (MediaRecorder.isTypeSupported(candidateMimeType)) {
        return candidateMimeType
      }
    }

    return undefined
  }

  private parseServerMessage(raw: unknown): VoiceServerMessage | null {
    if (typeof raw !== 'string') {
      return null
    }

    let parsed: unknown = null
    try {
      parsed = JSON.parse(raw)
    }
    catch {
      return null
    }

    if (!parsed || typeof parsed !== 'object') {
      return null
    }

    const parsedMessage = parsed as Partial<VoiceServerMessage>
    if (parsedMessage.type === 'ready') {
      return { type: 'ready' }
    }

    if (parsedMessage.type === 'words' && typeof parsedMessage.words === 'string') {
      return { type: 'words', words: parsedMessage.words }
    }

    if (parsedMessage.type === 'error' && typeof parsedMessage.message === 'string') {
      return { type: 'error', message: parsedMessage.message }
    }

    return null
  }
}
