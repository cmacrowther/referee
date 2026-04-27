# @cmacrowther/referee-client

Official JavaScript and TypeScript client for the REFEREE local upscaling API. You can find more detailed information in the [Developer Integration Guide](https://referee.craw.ca/developers).

## Install

```sh
npm install @cmacrowther/referee-client
```

## Vanilla JavaScript / TypeScript

```ts
import { RefereeClient } from "@cmacrowther/referee-client"

const referee = new RefereeClient({ appName: "MyApp" })

const status = await referee.getStatus()
if (status.gpuReady !== true) {
  // Fall back to your normal player.
}

const { session, dispose } = await referee.startManagedSession({
  url: "https://example.com/live/stream.m3u8",
  streamTitle: "Friday Night Stream",
})

player.src = session.url

// Stops heartbeat, releases the REFEREE session, and removes unload handlers.
dispose()
```

## React

```tsx
import { useEffect } from "react"
import { useReferee } from "@cmacrowther/referee-client/react"

export function VideoPlayer({ sourceUrl }: { sourceUrl: string }) {
  const referee = useReferee({ appName: "MyApp" })

  useEffect(() => {
    referee.start({ url: sourceUrl, streamTitle: "Live Stream" })
    return () => {
      referee.stop()
    }
  }, [sourceUrl])

  if (referee.status === "unavailable") return <NativePlayer src={sourceUrl} />
  if (referee.playbackUrl) return <HlsPlayer src={referee.playbackUrl} />
  return <LoadingSpinner />
}
```

## Authentication

`GET /v1/status` is unauthenticated. The first authenticated call automatically requests a token with `POST /v1/auth/request`. In desktop mode, REFEREE shows the user a consent dialog. In headless mode, pre-approve origins with `REFEREE_ALLOWED_ORIGINS` or manage them with the origins API.

You can also pass a token directly:

```ts
const referee = new RefereeClient({
  baseUrl: "http://localhost:14002",
  token: process.env.REFEREE_API_TOKEN,
})
```

## Package Exports

```ts
import { RefereeClient, createRefereeClient, RefereeApiError } from "@cmacrowther/referee-client"
import { useReferee } from "@cmacrowther/referee-client/react"
```

## Publishing

From the repository root:

```sh
npm install
npm run typecheck:client
npm run pack:client
npm publish -w @cmacrowther/referee-client
```
