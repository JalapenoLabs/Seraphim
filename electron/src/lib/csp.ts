// Copyright © 2026 Jalapeno Labs

import { API_PORT } from '@common/constants'

/* CSP notes:
 *
 * style-src
 *    'self' --> Allows loading <link rel="stylesheet"> from the same origin.
 *    'seraphim://app' --> This is our production protocol and location origin
 *    'unsafe-inline' --> Grants permission for any inline <style> blocks or style="…" attributes.
 *    IN PRODUCTION, we use vite.build.cssCodeSplit to avoid inline styles.
 *        We use a SHA256 to allow a very specific inline style. It's the contents of the string (trimmed)
 *        This is because HeroUI/TailwindCSS uses an Aria hook that injects it's own custom style.
 *        The hash will register the inline style as safe to use.
 * script-src
 *    'self' --> Allows loading <script> from the same origin.
 *    'unsafe-inline' --> Grants permission for any inline <script> blocks or script="…" attributes.
 *         The SHA256 used points to the `var global = {}` script defined in index.html, as a HeroUI bug workaround.
 *    'unsafe-eval' --> Allows the use of eval() and similar methods for executing code strings.
 *         We must use unsafe-eval in development mode for HMR (Hot Module Replacement).
 *         Unfortunately, HeroUI uses eval() in their React hooks so we must allow it in production as well :(
 */

export const devCsp: string[] = [
  // everything from our own origin…
  'default-src \'self\'',
  // …but load & run scripts (including inline & eval’d HMR code) from localhost:5173
  `script-src 'self' http://localhost:5173 http://localhost:${API_PORT} 'unsafe-inline' 'unsafe-eval'`,
  // styles (including inline HMR style updates) from localhost:5173
  `style-src 'self' http://localhost:5173 http://localhost:${API_PORT} 'unsafe-inline' https://fonts.googleapis.com`,
  'font-src http://localhost:5173 https://fonts.gstatic.com',
  // images from localhost:5173 and data URIs
  `img-src 'self' http://localhost:5173 http://localhost:${API_PORT} https://avatars.githubusercontent.com data:`,
  // HMR websocket & XHR
  `connect-src 'self' http://localhost:5173 http://localhost:${API_PORT} ws://localhost:5173 ws://localhost:${API_PORT}`,
]

export const prodCsp: string[] = [
  // lock everything down to your custom protocol
  'default-src \'self\' seraphim://app',
  // only run bundled scripts
  `script-src 'self' seraphim://app http://localhost:${API_PORT} 'unsafe-inline' 'unsafe-eval'`,
  // allow inline styles (for bundled styles) (unsafe inline needed for Monaco editor styles)
  `style-src 'self' seraphim://app http://localhost:${API_PORT} 'unsafe-inline' https://fonts.googleapis.com`,
  'font-src seraphim://app https://fonts.gstatic.com',
  // images via protocol or data URIs
  'img-src \'self\' seraphim://app https://avatars.githubusercontent.com data:',
]
