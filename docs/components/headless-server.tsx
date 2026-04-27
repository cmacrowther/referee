import type { ReactNode } from "react"
import {
  Server,
  Terminal,
  Container,
  CheckCircle,
  AlertCircle,
  Shield,
  Gauge,
  FlaskConical,
} from "lucide-react"

type OpenSectionProps = {
  id?: string
  icon: ReactNode
  title: string
  className?: string
  card?: boolean
  children: ReactNode
}

function OpenSection({ id, icon, title, className = "", card = false, children }: OpenSectionProps) {
  const scrollClass = id ? "scroll-mt-28" : ""

  if (card) {
    return (
      <section
        id={id}
        className={[scrollClass, "rounded-xl border border-border bg-card overflow-hidden", className].filter(Boolean).join(" ")}
      >
        <div className="p-6 border-b border-border flex items-center gap-3">
          {icon}
          <h3 className="text-xl font-semibold tracking-tight sm:text-2xl">{title}</h3>
        </div>
        <div className="p-6">
          {children}
        </div>
      </section>
    )
  }

  const classes = [scrollClass, className].filter(Boolean).join(" ")
  return (
    <section id={id} className={classes}>
      <div className="mb-5 flex items-center gap-3">
        {icon}
        <h3 className="text-xl font-semibold tracking-tight sm:text-2xl">{title}</h3>
      </div>
      {children}
    </section>
  )
}

export function HeadlessServer() {
  return (
    <>
      <HeadlessServerSetup />
      <CorsProxyGuide />
      <PerformanceExpectations />
      <TestingWithoutHardware />
    </>
  )
}

export function HeadlessServerSetup({ cardSections = false }: { cardSections?: boolean } = {}) {
  return (
    <div id="headless" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-3 flex items-center gap-3">
        <Server className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Headless Server</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        Run only the REFEREE backend pipeline without the desktop GUI. Useful for
        self-hosted deployments, CI environments, or any machine where you want to
        expose the upscaling API without the Tauri application.
      </p>

      <div className={`mt-10 ${cardSections ? "space-y-6" : "space-y-10"}`}>
        <OpenSection
          id="headless-rust-binary"
          icon={<Terminal className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Rust Binary (from source)"
          card={cardSections}
        >
          <div className="space-y-6">
            <p className="text-sm leading-7 text-muted-foreground sm:text-base">
              Build and run the headless server binary directly. Configuration is stored
              at{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
                %APPDATA%\com.referee.proxy
              </code>{" "}
              on Windows and{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
                ~/.local/share/com.referee.proxy
              </code>{" "}
              on Linux.
            </p>
            <ol className="space-y-5">
              {[
                {
                  title: "Clone the repository",
                  code: "git clone https://github.com/cmacrowther/referee.git && cd referee",
                },
                {
                  title: "Build the headless server binary",
                  code: "cargo build --features headless --bin referee-server --release",
                  cwd: "desktop/native/",
                },
                {
                  title: "Run the server",
                  code: "./target/release/referee-server",
                  note: "The server binds to 0.0.0.0:14002 — reachable from any device on your local network. Set REFEREE_API_TOKEN (≥ 32 chars) to use a known token instead of a generated UUID. Set REFEREE_ALLOWED_ORIGINS to a comma-separated list of origins to pre-approve (e.g. https://myapp.com) so browser clients can obtain the token via POST /v1/auth/request without a desktop UI.",
                },
              ].map((step, i) => (
                <li key={i} className="flex gap-4">
                  <span className="mt-0.5 flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full bg-accent/10 text-xs font-semibold text-accent">
                    {i + 1}
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="mb-1.5 text-sm font-medium sm:text-base">{step.title}</p>
                    {step.cwd && (
                      <p className="mb-1 text-xs text-muted-foreground">
                        Run from{" "}
                        <code className="rounded bg-muted px-1 py-0.5 font-mono">{step.cwd}</code>
                      </p>
                    )}
                    <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
                      <code>{step.code}</code>
                    </pre>
                    {step.note && (
                      <p className="mt-2 text-xs leading-6 text-muted-foreground">{step.note}</p>
                    )}
                  </div>
                </li>
              ))}
            </ol>
          </div>
        </OpenSection>

        <OpenSection
          id="headless-docker-compose"
          icon={<Container className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Docker Compose"
          card={cardSections}
        >
          <div className="space-y-6">
            <p className="text-sm leading-7 text-muted-foreground sm:text-base">
              Pre-built Compose stacks for NVIDIA and AMD. Choose the profile that
              matches your hardware. The container listens on port{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">14002</code>{" "}
              and persists data to a named volume. The NVIDIA profile publishes this on host port{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">14003</code>{" "}
              to avoid conflicts with the desktop app; AMD uses{" "}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">14002</code>{" "}
              on the host directly.
            </p>

            <div>
              <p className="mb-3 text-base font-medium">Prerequisites</p>
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="rounded-xl border border-border/60 bg-background/70 p-4">
                  <p className="mb-2 text-sm font-medium">NVIDIA</p>
                  <ul className="space-y-1.5">
                    {[
                      "nvidia-container-toolkit installed on the host",
                      "NVIDIA drivers with NVENC support",
                    ].map((req) => (
                      <li key={req} className="flex items-start gap-2 text-sm text-muted-foreground">
                        <CheckCircle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0 text-accent" />
                        {req}
                      </li>
                    ))}
                  </ul>
                </div>
                <div className="rounded-xl border border-border/60 bg-background/70 p-4">
                  <p className="mb-2 text-sm font-medium">AMD</p>
                  <ul className="space-y-1.5">
                    {[
                      "amdgpu-pro or ROCm driver installed on the host",
                      "/dev/dri and /dev/kfd device nodes available",
                    ].map((req) => (
                      <li key={req} className="flex items-start gap-2 text-sm text-muted-foreground">
                        <CheckCircle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0 text-accent" />
                        {req}
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
            </div>

            <ol className="space-y-5">
              {[
                {
                  title: "Clone the repository",
                  code: "git clone https://github.com/cmacrowther/referee.git && cd referee",
                },
                {
                  title: "Start with your hardware profile",
                  nvidia: "docker compose --profile nvidia up",
                  amd: "docker compose --profile amd up",
                },
              ].map((step, i) => (
                <li key={i} className="flex gap-4">
                  <span className="mt-0.5 flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full bg-accent/10 text-xs font-semibold text-accent">
                    {i + 1}
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="mb-1.5 text-sm font-medium sm:text-base">{step.title}</p>
                    {"code" in step ? (
                      <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
                        <code>{step.code}</code>
                      </pre>
                    ) : (
                      <div className="grid gap-3 sm:grid-cols-2">
                        <div>
                          <p className="mb-1.5 text-xs text-muted-foreground">NVIDIA</p>
                          <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
                            <code>{step.nvidia}</code>
                          </pre>
                        </div>
                        <div>
                          <p className="mb-1.5 text-xs text-muted-foreground">AMD</p>
                          <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
                            <code>{step.amd}</code>
                          </pre>
                        </div>
                      </div>
                    )}
                  </div>
                </li>
              ))}
            </ol>

            <div className="flex items-start gap-3 rounded-xl border border-accent/20 bg-accent/5 p-4">
              <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-accent" />
              <p className="text-sm leading-7 text-muted-foreground">
                Config files, downloaded encoder binaries, and logs are persisted to
                the{" "}
                <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
                  referee_data
                </code>{" "}
                named volume and survive container restarts.
              </p>
            </div>

            <div className="flex items-start gap-3 rounded-xl border border-border/60 bg-secondary/40 p-4">
              <Shield className="mt-0.5 h-4 w-4 flex-shrink-0 text-accent" />
              <div className="space-y-2 text-sm leading-7 text-muted-foreground">
                <p className="font-medium text-foreground">Authentication environment variables</p>
                <p>
                  Add these to the <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">environment:</code> block
                  in your Compose file:
                </p>
                <pre className="overflow-x-auto rounded-xl border border-border/60 bg-background/70 px-4 py-3 text-xs font-mono text-foreground">{`environment:
  REFEREE_API_TOKEN: "your-secret-token-at-least-32-chars"
  REFEREE_ALLOWED_ORIGINS: "https://myapp.com,https://other.com"`}</pre>
                <ul className="space-y-1.5">
                  <li>
                    <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">REFEREE_API_TOKEN</code>
                    {" "}— use a known token instead of a generated UUID. Must be ≥ 32 characters.
                    Recommended for production; use Docker secrets or a{" "}
                    <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">.env</code> file to avoid committing it.
                  </li>
                  <li>
                    <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">REFEREE_ALLOWED_ORIGINS</code>
                    {" "}— comma-separated list of origins pre-approved at startup. Browser clients hitting{" "}
                    <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">POST /v1/auth/request</code>{" "}
                    from these origins receive the token immediately without needing a desktop UI.
                  </li>
                </ul>
              </div>
            </div>
          </div>
        </OpenSection>
      </div>
    </div>
  )
}

export function CorsProxyGuide() {
  return (
    <section id="cors-proxy" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-4 flex items-center gap-3">
        <Shield className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">CORS &amp; Reverse Proxy</h2>
      </div>
      <div className="space-y-5">
        <p className="text-sm leading-7 text-muted-foreground sm:text-base">
          REFEREE sets{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
            Access-Control-Allow-Origin: *
          </code>{" "}
          on <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">GET /v1/status</code>,{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">POST /v1/auth/request</code>, and
          HLS segment responses, so browsers can access those without issue. All other endpoints
          (stream start/stop/heartbeat, origins management) only allow cross-origin requests from
          origins in the approved-origins list — unapproved cross-origin XHR to those endpoints
          will be blocked by the browser.
        </p>
        <div className="flex items-start gap-3 rounded-xl border border-accent/20 bg-accent/5 p-4">
          <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-accent" />
          <p className="text-sm leading-7 text-muted-foreground">
            <strong className="text-foreground">Mixed-content block:</strong> If your web app is served over{" "}
            <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">https://</code>, browsers will block
            requests to <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">http://localhost:14002</code>.
            During development, serve your app over HTTP, or use the nginx reverse proxy below.
          </p>
        </div>
        <div>
          <p className="mb-2 text-base font-medium">nginx reverse proxy (example)</p>
          <p className="mb-2 text-xs text-muted-foreground">
            Place this inside your <code className="rounded bg-muted px-1 py-0.5 font-mono">server</code> block to expose
            REFEREE under your own HTTPS domain.
          </p>
          <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
            <code>{`location /referee/ {
    proxy_pass         http://localhost:14002/;
    proxy_http_version 1.1;
    proxy_set_header   Host $host;
    proxy_set_header   X-Real-IP $remote_addr;

    # Allow the browser to consume the HLS stream cross-origin
    add_header Access-Control-Allow-Origin  "*" always;
    add_header Access-Control-Allow-Methods "GET, POST, OPTIONS" always;
    add_header Access-Control-Allow-Headers "Content-Type, X-Referee-Token" always;

    # Required for streaming TS segments without buffering
    proxy_buffering    off;
    proxy_read_timeout 300s;
}`}</code>
          </pre>
          <p className="mt-2 text-xs text-muted-foreground">
            Then set <code className="rounded bg-muted px-1 py-0.5 font-mono">REFEREE_BASE = &apos;https://yourdomain.com/referee&apos;</code> in
            your integration code.
          </p>
        </div>
      </div>
    </section>
  )
}

export function PerformanceExpectations() {
  return (
    <section id="performance" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-4 flex items-center gap-3">
        <Gauge className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Performance Expectations</h2>
      </div>
      <div>
        <p className="mb-4 text-sm leading-7 text-muted-foreground sm:text-base">
          Typical values for reference. Actual results vary by GPU, source resolution, and enabled features.
        </p>
        <div className="overflow-hidden rounded-xl border border-border/60">
          <div className="overflow-x-auto">
            <table className="w-full min-w-[40rem] text-sm">
              <thead className="bg-secondary/50">
                <tr>
                  <th className="px-4 py-3 text-left font-medium">Metric</th>
                  <th className="px-4 py-3 text-left font-medium">Typical Value</th>
                  <th className="px-4 py-3 text-left font-medium">Notes</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                <tr>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">End-to-end latency</td>
                  <td className="px-4 py-3 align-top font-mono text-accent">3-6 s</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">HLS rolling window of 3 x 1s segments plus player buffer</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">First-start time</td>
                  <td className="px-4 py-3 align-top font-mono text-accent">&lt; 5 s warm / up to 3 min cold</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Cold start downloads encoder binaries on first run only</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">GPU VRAM usage</td>
                  <td className="px-4 py-3 align-top font-mono text-accent">300 MB - 1 GB</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Scales with output resolution; 1080p ~= 400 MB, 4K ~= 900 MB</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Output bitrate</td>
                  <td className="px-4 py-3 align-top font-mono text-accent">4 - 20 Mbps</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Configurable in REFEREE settings; higher = better quality, more LAN bandwidth</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Idle RAM (headless)</td>
                  <td className="px-4 py-3 align-top font-mono text-accent">~10 MB</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Process footprint when no session is active</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  )
}

export function TestingWithoutHardware() {
  return (
    <section id="testing" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-4 flex items-center gap-3">
        <FlaskConical className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Testing Without Hardware</h2>
      </div>
      <div className="space-y-5">
        <p className="text-sm leading-7 text-muted-foreground sm:text-base">
          REFEREE requires a compatible GPU to start. For CI environments or machines without supported hardware,
          run a lightweight mock server that mirrors the API contract.
        </p>
        <div>
          <p className="mb-1.5 text-base font-medium">Minimal mock server (Node.js / Bun)</p>
          <p className="mb-2 text-xs text-muted-foreground">
            Save as <code className="rounded bg-muted px-1 py-0.5 font-mono">mock-referee.mjs</code> and run with{" "}
            <code className="rounded bg-muted px-1 py-0.5 font-mono">node mock-referee.mjs</code> or{" "}
            <code className="rounded bg-muted px-1 py-0.5 font-mono">bun mock-referee.mjs</code>.
          </p>
          <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
            <code>{`import { createServer } from 'node:http';

const SESSION_ID = 'mock-session-0000';

createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type, X-Referee-Token');

  // Handle CORS preflight
  if (req.method === 'OPTIONS') { res.writeHead(204); res.end(); return; }

  if (req.url === '/v1/status' && req.method === 'GET') {
    res.end(JSON.stringify({
      gpuReady: true, gpuName: 'Mock GPU', gpuVendor: 'nvidia',
      gpuUtilization: 0, encoderBackend: 'nvenc',
      encoderHasFramegen: true, encoderHasTruehdr: false,
      activeSessions: 0, sessions: [], primarySession: null,
      settings: { resolution: '1920x1080', quality: 3, framegen: false, hdr: false },
    }));
  } else if (req.url === '/v1/stream/start' && req.method === 'POST') {
    res.end(JSON.stringify({
      sessionId: SESSION_ID,
      url: \`http://localhost:14002/v1/tmp/\${SESSION_ID}/index.m3u8\`,
      resolution: '1920x1080', sourceResolution: '1280x720',
      effectiveQuality: 3, evictedSessions: [],
      appName: 'Mock', streamTitle: 'Mock Stream',
    }));
  } else if (req.url?.startsWith('/v1/stream/heartbeat/') && req.method === 'POST') {
    res.end(JSON.stringify({ status: 'ok' }));
  } else if (req.url === '/v1/stream/stop' && req.method === 'POST') {
    res.end(JSON.stringify({ status: 'stopped' }));
  } else {
    res.writeHead(404); res.end(JSON.stringify({ error: 'Not found', code: 'SESSION_NOT_FOUND' }));
  }
}).listen(14002, () => console.log('Mock REFEREE running on :14002'));`}</code>
          </pre>
        </div>
        <div className="flex items-start gap-3 rounded-xl border border-border/60 bg-secondary/40 p-4">
          <CheckCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-accent" />
          <p className="text-sm leading-7 text-muted-foreground">
            For browser-level testing, use{" "}
            <a
              href="https://mswjs.io/"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              Mock Service Worker (msw)
            </a>{" "}
            to intercept fetch calls to <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">localhost:14002</code> without
            running any server process.
          </p>
        </div>
      </div>
    </section>
  )
}
