import { app, shell, BrowserWindow, ipcMain } from 'electron'
import { join } from 'path'
import { electronApp, optimizer, is } from '@electron-toolkit/utils'
import { createHostEmitter, registerHostIpc, trustedFrames } from '@agentos/sdk/host/main'
import icon from '../../resources/icon.png?asset'
import { startPlatform, type Platform } from './platform'

let mainWindow: BrowserWindow | undefined
let platform: Platform | undefined

const rendererUrl = is.dev ? process.env['ELECTRON_RENDERER_URL'] : undefined
const isTrustedFrame = trustedFrames(rendererUrl)
const emit = createHostEmitter(() => mainWindow?.webContents)

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1440,
    height: 900,
    minWidth: 960,
    minHeight: 640,
    show: false,
    backgroundColor: '#2a2d6b',
    ...(process.platform === 'darwin'
      ? { titleBarStyle: 'hiddenInset' as const, trafficLightPosition: { x: 16, y: 13 } }
      : {}),
    ...(process.platform === 'linux' ? { icon } : {}),
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      // The renderer gets no Node access; it only sees the SDK bridge.
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      webviewTag: false
    }
  })
  mainWindow = win

  const sendFullScreen = (): void =>
    emit('window:fullscreen-changed', { fullScreen: win.isFullScreen() })
  win.on('enter-full-screen', sendFullScreen)
  win.on('leave-full-screen', sendFullScreen)
  win.on('closed', () => {
    if (mainWindow === win) mainWindow = undefined
  })
  win.on('ready-to-show', () => win.show())

  // Never navigate the host window away from the renderer; open http(s) links externally.
  win.webContents.on('will-navigate', (event, url) => {
    if (!isTrustedUrl(url)) event.preventDefault()
  })
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//.test(url)) void shell.openExternal(url)
    return { action: 'deny' }
  })

  if (rendererUrl) void win.loadURL(rendererUrl)
  else void win.loadFile(join(__dirname, '../renderer/index.html'))
}

function isTrustedUrl(url: string): boolean {
  try {
    const target = new URL(url)
    return rendererUrl ? target.origin === new URL(rendererUrl).origin : target.protocol === 'file:'
  } catch {
    return false
  }
}

app.whenReady().then(async () => {
  electronApp.setAppUserModelId('com.agentos.app')

  // F12 opens DevTools in development; reload shortcuts are ignored in production.
  app.on('browser-window-created', (_, window) => optimizer.watchWindowShortcuts(window))

  platform = await startPlatform(emit)
  const { handlers } = platform

  // The whole host protocol, registered once: exhaustive, validated, sender-checked.
  registerHostIpc(
    ipcMain,
    {
      invoke: {
        ...handlers.invoke,
        'window:is-fullscreen': () => mainWindow?.isFullScreen() ?? false
      },
      send: {
        ...handlers.send,
        'window:toggle-fullscreen': () => mainWindow?.setFullScreen(!mainWindow.isFullScreen()),
        'app:quit': () => app.quit()
      }
    },
    { isTrustedSender: isTrustedFrame }
  )

  createWindow()

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('before-quit', () => {
  void platform?.shutdown()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
