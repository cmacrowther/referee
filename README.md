<p align="center">
  <img src="assets/whistle_orange.svg" alt="REFEREE Logo" width="120">
</p>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/cmacrowther/referee/release.yml" alt="Build status">
  <img src="https://img.shields.io/badge/License-GPL3-blue.svg" alt="License">
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-0078d7.svg" alt="Platform">
  <img src="https://img.shields.io/badge/GPU-NVIDIA%20RTX%20%7C%20AMD%20Radeon-76b900.svg" alt="GPU Support">
  <img src="https://img.shields.io/badge/Built%20with-Rust-24C8DB.svg" alt="Built with Rust">
</p>

<p align="center">
  <a href="https://referee.craw.ca/demo"><img src="https://img.shields.io/badge/Live%20Demo-▶%20Try%20it%20out-ff6b35.svg?style=for-the-badge" alt="Live Demo"></a>
  &nbsp;
  <a href="https://referee.craw.ca/"><img src="https://img.shields.io/badge/Documentation-📖%20Read%20the%20docs-4a9eff.svg?style=for-the-badge" alt="Documentation"></a>
</p>

# REFEREE

> Bring GPU upscaling to your web media.

REFEREE lets you ship low-bitrate video streams and offload upscaling to your users' own hardware. It runs as a lightweight background service on the user's machine, intercepting streams from your web player and enhancing them in real-time using supported NVIDIA RTX and AMD Radeon hardware — AI upscaling, frame generation (where supported), and HDR conversion — before returning the result for playback.

**The pitch for developers:** you don't need to encode, store, or serve high-resolution video. Distribute a 480p or 720p stream and let REFEREE handle the rest on-device. Users with RTX hardware get 4K-quality playback; you save on bandwidth, CDN egress, storage, and server-side transcoding. Users also get control over their own quality settings — as high as their GPU can handle.

Integrate with a few lines of JavaScript. No native code required on your end. **[Try the live demo →](https://referee.craw.ca/demo)**


## Technical Architecture

Built for speed and extreme resource efficiency, REFEREE utilizes a modern, multi-layered stack:

* **Backend:** Written in **Rust** for memory safety and maximum execution performance.
* **System Service:** A highly efficient background process consuming only **10MB of RAM at idle**.
* **Frontend:** Built with **Tauri and Vite**, featuring a heavily optimized web view to ensure a minimal desktop footprint.
* **Headless Containers:** REFEREE can be run without the need for the GUI application as a headless server. Run the headless binary or use the provided Docker compose stacks (making sure to choose the correct profile for your hardware).
* **Session Management:** Intelligent handling ensures that streams are strictly managed; if a stream disconnects, the session is cleared to maintain high system efficiency and release hardware resources.

### Pipeline Architecture

The backend uses a graph-based plan/execute model. Three executor implementations are selected at runtime based on hardware:

* **`NvidiaSpecializedExecutor`** — Windows + NVIDIA, backed by NVEncC for native NGX-VSR upscaling, FRUC frame generation, and TrueHDR.
* **`AmdExecutor`** — AMD path via VCEEncC with AMF-FSR upscaling.
* **`UniversalExecutor`** — Cross-platform FFmpeg/libplacebo path. Supports Anime4K and ArtCNN GLSL shader upscaling, libplacebo HDR tonemapping, NVENC (Linux/NVIDIA), and VAAPI (Linux/AMD).

Each request flows through Preprocess → Execute → Package stages, planned by a `GraphPlanner` that separates capability modelling from executor implementation. See the [developer docs](https://referee.craw.ca/developers) for the full breakdown.


## How It Works

REFEREE runs silently in the background on port `14002`. When your web player wants to stream video, it hands the source URL to REFEREE instead of loading it directly.

1. **Handshake** — The web app POSTs to `/v1/stream/start` with the source stream URL and an `X-Referee-Token` header. REFEREE creates a session and returns a local URL pointing to the enhanced output stream.
2. **Pipeline** — REFEREE launches the selected hardware encoder, which pulls and decodes the source stream directly.
3. **Enhancement** — The user's supported NVIDIA or AMD GPU applies upscaling, optional frame generation (backend-dependent), and/or HDR conversion in real-time.
4. **Playback** — Your web player loads the returned local HLS URL. The user sees enhanced output; your origin served a fraction of the bitrate.
5. **Cleanup** — A heartbeat (`POST /v1/stream/heartbeat/{sessionId}`) keeps the session alive. If it drops for more than 15 seconds, REFEREE automatically terminates the GPU process and clears temporary files. Call `/v1/stream/stop` explicitly when done to release resources immediately.


## Installation
### REFEREE Desktop Application
1. Head over to the releases page and [download the REFEREE installer](https://github.com/cmacrowther/referee/releases) for your platform
2. Install the application and run it (REFEREE will request local network permissions to make sure it's accessible on your LAN)
3. Open up a REFEREE compatible stream. You can try out the demo available on the [REFEREE Docs](https://referee.craw.ca/demo)

### REFEREE Self-Hosted
REFEREE can be self-hosted to run the backend pipeline on a headless server. Configuration files are located at `%APPDATA%\Roaming\com.referee.proxy` on Windows and `~/.local/share/com.referee.proxy` on Linux.

#### Rust Binary
1. Clone this repository: `git clone https://github.com/cmacrowther/referee.git`
2. Run `npm run build`

#### Docker Compose
1. Clone this repository: `git clone https://github.com/cmacrowther/referee.git`
2. Run `docker compose --profile <amd|nvidia> up`

**Headless environment variables:**
- `REFEREE_API_TOKEN` — Pre-set the API token (minimum 32 characters). Required for headless deployments.
- `REFEREE_ALLOWED_ORIGINS` — Comma-separated list of origins to pre-approve without a consent dialog.

## Integration

Install the client library:

```sh
npm install @cmacrowther/referee-client
```

```ts
import { RefereeClient } from '@cmacrowther/referee-client';

const referee = new RefereeClient({ appName: 'MyApp' });

// Probe GPU availability before committing to upscaled playback
const status = await referee.getStatus();
if (!status.gpuReady) return; // graceful fallback to native playback

// Start an upscaling session — blocks until the HLS stream is ready
const session = await referee.startSession({
  url: 'https://cdn.example.com/stream.m3u8',
  streamTitle: 'Live Stream',
});

// Load session.url in your HLS player (hls.js, Video.js, etc.)
player.load(session.url);

// Call stop when done to release GPU resources immediately
await referee.stopSession(session.sessionId);
```

A `useReferee()` React hook is also available from `@cmacrowther/referee-client/react`, handling availability checks, heartbeat keepalives, and cleanup on unmount automatically.

For the full API reference, authentication details, CORS configuration, and reverse-proxy setup, see the [integration docs](https://referee.craw.ca/integrate).


## Features

**Pipeline / Enhancement**
- **AI Upscaling** — NVIDIA NGX-VSR and AMD AMF-FSR for hardware-native upscaling. Anime4K and ArtCNN GLSL shader upscaling via the Universal executor for broader hardware reach.
- **Frame Generation** — Motion-compensated frame interpolation for smoother playback (FRUC on NVIDIA; expanding to AMD/Linux).
- **TrueHDR** — On-the-fly SDR → HDR10/BT.2020 tone mapping.
- **Low Latency** — Hardware-accelerated pipeline; 3–6 s end-to-end with standard HLS buffering.

**Platform & Deployment**
- **REFEREE Relay** — Devices without a compatible GPU can relay sessions to a capable peer on the same LAN.
- **Headless / Self-Hosted** — Run `referee-server` standalone or via the provided Docker Compose stacks (NVIDIA and AMD profiles).
- **Self-Contained** — All encoder dependencies download automatically on first run. No manual SDK configuration.

**Developer Experience**
- **TypeScript client** — `@cmacrowther/referee-client` on npm with full TypeScript types and a `useReferee()` React hook.
- **Mock server** — Test your integration against a local mock without physical GPU hardware.
- **Open output format** — Enhanced streams are standard HLS; play directly in hls.js, Video.js, VLC, or any HLS-compatible player.


## Compatibility

| Platform | Upscaling | Frame Gen | HDR | Relay |
|---|---|---|---|---|
| Windows + NVIDIA | ✅ | ✅ | ✅ | ✅ |
| Windows + AMD | ✅ | 🔜 | ✅ | ✅ |
| Linux + NVIDIA | ✅ | 🔜 | ✅ | ✅ |
| Linux + AMD | ✅ | 🔜 | ✅ | ✅ |
| Docker + NVIDIA | ✅ | 🔜 | ✅ | 🔜 |
| Docker + AMD | ✅ | 🔜 | ✅ | 🔜 |

🔜 = in progress.

**Hardware minimums:** NVIDIA GeForce RTX 20-series or newer; AMD Radeon RX 6000-series or newer; 8 GB+ system RAM.

**OS:** Windows 10/11 (64-bit) or Linux x64 (Ubuntu, Fedora, RedHat-based). macOS support is on the [roadmap](#roadmap).

**Drivers:** Run the latest stable drivers from your GPU vendor. Linux AMD builds require [AMD's AMF drivers](https://github.com/GPUOpen-LibrariesAndSDKs/AMF) — Mesa is not supported.


## Performance

| Metric | Typical Range |
|---|---|
| Idle RAM | ~10 MB |
| VRAM usage | 300 MB – 1 GB (scales with output resolution) |
| End-to-end latency | 3 – 6 s (HLS segment buffer) |
| Output bitrate | 4 – 20 Mbps (configurable) |
| Cold-start time | < 5 s warm; up to ~3 min cold (one-time binary download) |


## Roadmap

Current development priorities. See the [full roadmap](https://referee.craw.ca/roadmap) for the latest status.

- **AMD AMF on Windows** — Full hardware-accelerated upscaling and frame generation for RX 7000/8000 series.
- **macOS** — Metal-accelerated upscaling, Apple Silicon native build.
- **Intel Arc** — XeSS and oneVPL API support.
- **Docker profiles** — Expanded NVIDIA and AMD container configurations.


## License

Distributed under the GPL3 License. See `LICENSE` for more information.


## Credits & Third-Party Software

REFEREE is made possible by the incredible work of the open-source community, including **NVEncC**, **VCEEncC**, and **Tauri**.

NVIDIA RTX features (AI upscaling, frame generation, and TrueHDR) are powered by proprietary NVIDIA SDKs including the NVIDIA Video Codec SDK, NVIDIA NGX SDK, and NVIDIA Optical Flow SDK. NVIDIA, GeForce RTX, and related marks are trademarks of NVIDIA Corporation.

AMD Radeon features (hardware encoding and FSR-based upscaling) are powered by AMD's Advanced Media Framework (AMF) and FidelityFX technologies via VCEEncC. AMD, Radeon, and related marks are trademarks of Advanced Micro Devices, Inc.

For a full list of dependencies, licenses, and attributions, see [CREDITS.md](CREDITS.md).


## Contributing

Contributions are welcome. Pull requests, bug reports, and suggestions help make REFEREE better for everyone.


## Legal & Liability Disclaimer

REFEREE is provided "as is", without warranty of any kind, express or implied.

This project is a neutral, local network utility. It acts solely as a conduit to process data that the user provides. The creator and contributors of REFEREE:

- Do not host, provide, or distribute any media content.
- Are not affiliated with, endorsed by, or connected to NVIDIA Corporation or any streaming service providers.
- Accept absolutely no liability for how end-users choose to utilize this software.

**It is entirely the user's responsibility to ensure they have the legal right to access, process, and manipulate the media streams they pipe through this application. Users must respect the Terms of Service and DRM policies of any content providers they interact with. The creator of this software assumes no responsibility for copyright infringement, account bans, or any other legal or technical repercussions resulting from the use of this tool.**
