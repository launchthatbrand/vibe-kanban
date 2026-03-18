# Remote Preview Tunnel Checklist

Use this checklist to validate remote-style preview behavior before deploying to a hosted domain.

## 1) Start local stack

1. From `packages/vibekanban/managed/vendor/vibe-kanban`, run:
   - `pnpm dev:bypass-auth`
2. Confirm frontend and backend start cleanly.
3. Confirm backend reports a `preview_proxy_port` in `/api/info`.

## 2) Start Cloudflare tunnel

1. In a second terminal, run:
   - `cloudflared tunnel --url "http://localhost:${FRONTEND_PORT:-3000}"`
2. Copy the `https://*.trycloudflare.com` URL.

## 3) Allow tunnel origin in backend

1. Set `VK_ALLOWED_ORIGINS` to include both local and tunnel origins:
   - `http://localhost:${FRONTEND_PORT}`
   - `https://<your-tunnel-domain>`
2. Restart backend/dev stack after updating env.

## 4) Validate remote-style preview mode

1. Open the tunnel URL and go to workspace preview.
2. Start the preview dev server from the preview panel.
3. Confirm iframe `src` uses path mode:
   - `https://<current-host>/__vk_preview/<devPort>/...`
4. Navigate within the app and confirm URL bar tracks target app URL (not proxy URL).
5. Validate full-reload route navigation still works.
6. Confirm hot reload still works after a source edit.
7. Toggle inspect mode and verify component payload appears in chat.
8. Toggle devtools/Eruda and confirm open/close works.

## 5) Localhost regression check

1. Open local app directly on localhost.
2. Confirm preview still uses subdomain mode:
   - `http://<devPort>.localhost:<previewProxyPort>/...`
3. Confirm inspect/devtools/navigation continue to work.

## 6) Quick failure triage

- **Iframe blank on tunnel URL:** verify Vite proxy for `/__vk_preview` is active and backend preview proxy is running.
- **Origin blocked errors:** verify exact `https://<tunnel-domain>` exists in `VK_ALLOWED_ORIGINS`.
- **HMR not updating:** check websocket upgrade through `__vk_preview` and tunnel connectivity.
