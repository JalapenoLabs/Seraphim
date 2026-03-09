// Copyright © 2026 Jalapeno Labs

// Core
import { BrowserWindow, shell } from 'electron'
import electronLocalshortcut from 'electron-localshortcut'

// Lib
// https://www.npmjs.com/package/electron-window-state
import windowStateKeeper from 'electron-window-state'
// https://www.npmjs.com/package/electron-serve
import serve from 'electron-serve'

// Misc
import { clamp } from 'lodash-es'
import { isProduction } from './env'
import { browserDir, logoPath, getSourceFile } from './lib/internalFiles'
import { devCsp, prodCsp } from './lib/csp'

const loadDirectory = serve({
  scheme: 'app',
  hostname: 'seraphim',
  directory: browserDir,
})

export function newWindow() {
  console.info('Spawning new window')

  // Load the previous state with fallback to defaults
  const windowStateManager = windowStateKeeper({
    defaultWidth: 1280,
    defaultHeight: 720,
    fullScreen: false,
  })

  // Create the window using the state information
  const window = new BrowserWindow({
    x: windowStateManager.x,
    y: windowStateManager.y,
    minHeight: 500,
    minWidth: 750,
    width: windowStateManager.width,
    height: windowStateManager.height,
    titleBarStyle: 'default',
    frame: true,
    title: 'Seraphim',
    center: false,
    resizable: true,
    autoHideMenuBar: true,
    icon: logoPath,
    webPreferences: {
      webgl: true,
      sandbox: false,
      zoomFactor: 1.0,
      // https://www.electronjs.org/docs/latest/tutorial/security#3-enable-context-isolation
      contextIsolation: true,
      // https://www.electronjs.org/docs/latest/tutorial/security#3-enable-context-isolation
      nodeIntegration: false,
      nodeIntegrationInWorker: false,
      preload: getSourceFile('preload.js'),
    },
  })

  let windowStateSaveTimer: NodeJS.Timeout | null = null

  function scheduleWindowStateSave() {
    if (windowStateSaveTimer) {
      clearTimeout(windowStateSaveTimer)
    }

    windowStateSaveTimer = setTimeout(() => {
      windowStateManager.saveState(window)
      windowStateSaveTimer = null
    }, 350)
  }

  function handleWindowClose() {
    windowStateManager.saveState(window)
  }

  window.once('ready-to-show', () => {
    window.setResizable(true)
  })

  // Removes the default menu bar
  // Although it's "hidden" from "autoHideMenuBar" it still exists and can be accessed with Alt
  // https://github.com/electron/electron/issues/34211#issuecomment-1375073575
  window.removeMenu()

  // What if the user attempts to navigate away from the application?
  // Let's say they click on a link to open "https://jalapenolabs.io" in a new tab.
  // We need to intercept it, and open it in the user's default browser instead of the Electron app.

  // Intercept any window.open or <a target="_blank">
  window.webContents.setWindowOpenHandler((details) => {
    const url = details.url

    // if it’s an http(s) link, open externally
    if (url.startsWith('http')) {
      shell.openExternal(url)
      console.info('Opening external link:', url)
      return { action: 'deny' }
    }

    // otherwise allow (e.g. internal deep-linking, mailto:, etc)
    return { action: 'allow' }
  })

  // Catch in-page navigation (e.g. <a href="http://…">)
  window.webContents.on('will-navigate', (event, url) => {
    // if navigating away from your app’s origin
    if (url !== window.webContents.getURL()) {
      console.info('Intercepting navigation to external link:', url)
      event.preventDefault()
      shell.openExternal(url)
    }
  })

  // When the application reloads, we should reset the zoom factor to 100% (normal)
  window.webContents.on('did-finish-load', () => {
    // setZoomFactor(1) = 100 % zoom
    window.webContents.setZoomFactor(1)
  })

  // Allow renderer microphone requests for voice transcription
  const allowedMediaPermissions = new Set([ 'media', 'audioCapture' ])

  window.webContents.session.setPermissionCheckHandler((_, permission) => {
    if (allowedMediaPermissions.has(permission)) {
      return true
    }

    return false
  })

  window.webContents.session.setPermissionRequestHandler((_, permission, callback) => {
    callback(allowedMediaPermissions.has(permission))
  })

  // Content Security Policy (CSP)
  // https://www.electronjs.org/docs/latest/tutorial/security#7-define-a-content-security-policy
  const csp: string[] = isProduction
    ? prodCsp
    : devCsp

  window.webContents.session.webRequest.onHeadersReceived((details, callback) => {
    callback({
      responseHeaders: {
        ...details.responseHeaders,
        'Content-Security-Policy': [ csp.join('; ') ],
      },
    })
  })

  electronLocalshortcut.register(window, 'Ctrl+Shift+I', () => {
    window.webContents.toggleDevTools()
  })

  electronLocalshortcut.register(window, 'Ctrl+N', () => {
    newWindow()
  })

  electronLocalshortcut.register(window, 'Ctrl+R', () => {
    window.webContents.reload()
  })

  const zoomFactor = 0.1

  // Zoom in:
  electronLocalshortcut.register(window, 'Ctrl+=', () => {
    window.webContents.setZoomFactor(
      clamp(
        window.webContents.getZoomFactor() + zoomFactor,
        0.5,
        3,
      ),
    )
  })

  // Zoom out:
  electronLocalshortcut.register(window, 'Ctrl+-', () => {
    window.webContents.setZoomFactor(
      clamp(
        window.webContents.getZoomFactor() - zoomFactor,
        0.5,
        3,
      ),
    )
  })

  // Reset zoom:
  electronLocalshortcut.register(window, 'Ctrl+0', () => {
    window.webContents.setZoomFactor(1)
  })

  electronLocalshortcut.register(window, 'Ctrl+Shift+R', () => {
    window.webContents.reloadIgnoringCache()
  })

  window.on('resize', scheduleWindowStateSave)
  window.on('move', scheduleWindowStateSave)
  window.on('close', handleWindowClose)

  // https://www.electronjs.org/docs/latest/api/app#appispackaged-readonly
  if (isProduction) {
    console.info('Serving production static build files')
    // Loads the index.html and other files for the app.
    loadDirectory(window)
  }
  else {
    console.info('Serving development localhost:5173 vite dev server')
    // Load the dev server:
    window.loadURL('http://localhost:5173')
    // Open the DevTools automatically
    window.webContents.openDevTools()
  }

  if (windowStateManager.isMaximized) {
    window.maximize()
  }

  // Let the window state manager register listeners on the window, so we can update the state
  // automatically (the listeners will be removed when the window is closed)
  // and restore the maximized or full screen state
  windowStateManager.manage(window)

  return window
}
