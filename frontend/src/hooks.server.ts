import type { Handle } from '@sveltejs/kit'

import { env } from '$env/dynamic/private'

// In production the SvelteKit node server fronts everything, so it proxies
// `/api/*` to the Rust backend. (Over Tailscale, `tailscale serve` routes `/api`
// straight to the API and this never fires; for direct host-port access it does.)
const API_ORIGIN = env.API_ORIGIN ?? 'http://localhost:8080'

export const handle: Handle = async ({ event, resolve }) => {
  if (!event.url.pathname.startsWith('/api')) {
    return resolve(event)
  }

  const target = API_ORIGIN + event.url.pathname + event.url.search
  const isBodyless = event.request.method === 'GET' || event.request.method === 'HEAD'

  return fetch(target, {
    method: event.request.method,
    headers: event.request.headers,
    body: isBodyless ? undefined : await event.request.arrayBuffer(),
    // Required by undici when a request carries a body.
    // @ts-expect-error - `duplex` is valid at runtime but missing from the DOM types.
    duplex: 'half'
  })
}
