import type { ReactNode } from "react"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { AlertCircle, CheckCircle, FileCode, KeyRound, Server, Wrench } from "lucide-react"

// packaging/systemd/referee-server.service without its comments; update both together.
const serviceUnit = `[Unit]
Description=REFEREE relay server (headless)
Documentation=https://referee.craw.ca/developers#relay-server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=referee
Group=referee
StateDirectory=referee
Environment=HOME=/var/lib/referee
Environment=LD_LIBRARY_PATH=/var/lib/referee/.local/share/com.referee.proxy/lib
EnvironmentFile=-/etc/referee/referee-server.env

ExecStart=/usr/local/bin/referee-server
KillSignal=SIGINT
TimeoutStopSec=30
Restart=on-failure
RestartSec=5

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target`

const dataDir = "/var/lib/referee/.local/share/com.referee.proxy"

type Step = {
  title: string
  code: string
  note?: ReactNode
}

const installSteps: Step[] = [
  {
    title: "Install build dependencies",
    code: `sudo apt install build-essential pkg-config libssl-dev libgtk-3-dev \\
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \\
  pciutils libvulkan1`,
    note: (
      <>
        Debian/Ubuntu shown. Install Rust with{" "}
        <a href="https://rustup.rs" target="_blank" rel="noopener noreferrer" className="text-accent hover:underline">
          rustup
        </a>
        . <code className="rounded bg-muted px-1 py-0.5 font-mono">tauri-build</code> runs during the build, so the
        WebKitGTK headers are needed even for the headless binary. For other distros, use the package names from{" "}
        <a
          href="https://v2.tauri.app/start/prerequisites/#linux"
          target="_blank"
          rel="noopener noreferrer"
          className="text-accent hover:underline"
        >
          Tauri&apos;s Linux prerequisites
        </a>
        . <code className="rounded bg-muted px-1 py-0.5 font-mono">pciutils</code> and{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">libvulkan1</code> are runtime dependencies for GPU
        detection and libplacebo.
      </>
    ),
  },
  {
    title: "Build referee-server",
    code: `git clone https://github.com/cmacrowther/referee.git && cd referee
cargo build --manifest-path desktop/native/Cargo.toml \\
  --features headless --bin referee-server --release`,
  },
  {
    title: "Install the binary",
    code: "sudo install -m 0755 desktop/native/target/release/referee-server /usr/local/bin/",
    note: (
      <>
        Optional RIFE frame generation: run{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">node scripts/fetch-rife-vendor.js</code>, build{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">desktop/native/rife-worker</code> with{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">cargo build --release --locked</code> (needs CMake
        and git), and install its <code className="rounded bg-muted px-1 py-0.5 font-mono">rife-worker</code> binary
        next to <code className="rounded bg-muted px-1 py-0.5 font-mono">referee-server</code> in{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">/usr/local/bin</code>.
      </>
    ),
  },
  {
    title: "Create the service user",
    code: `sudo useradd --system --home-dir /var/lib/referee --shell /usr/sbin/nologin referee
sudo usermod -aG video,render referee`,
    note: (
      <>
        The <code className="rounded bg-muted px-1 py-0.5 font-mono">video</code> and{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">render</code> groups own{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">/dev/dri</code> and{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">/dev/kfd</code> on most distros. If your distro has
        no <code className="rounded bg-muted px-1 py-0.5 font-mono">render</code> group, leave it out. systemd creates{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">/var/lib/referee</code> on first start.
      </>
    ),
  },
  {
    title: "Install the unit and environment file",
    code: `sudo install -m 0644 packaging/systemd/referee-server.service /etc/systemd/system/
sudo install -D -m 0600 packaging/systemd/referee-server.env /etc/referee/referee-server.env`,
    note: (
      <>
        The environment file holds <code className="rounded bg-muted px-1 py-0.5 font-mono">REFEREE_API_TOKEN</code>{" "}
        and <code className="rounded bg-muted px-1 py-0.5 font-mono">REFEREE_ALLOWED_ORIGINS</code>, so keep it
        readable by root only. systemd reads it before dropping to the{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">referee</code> user.
      </>
    ),
  },
  {
    title: "Approve each viewing device",
    code: `# On the viewing device (the one running the desktop app):
curl -s http://localhost:14002/v1/ping
# Windows PowerShell:
(Invoke-RestMethod http://localhost:14002/v1/ping).instanceId

# On the relay server, add that device to /etc/referee/referee-server.env:
REFEREE_ALLOWED_ORIGINS=https://peer-<instanceId>.referee.invalid`,
    note: (
      <>
        Separate multiple devices with commas. See{" "}
        <a href="#relay-server-approval" className="text-accent hover:underline">
          Linking without a consent dialog
        </a>{" "}
        for why this step is needed.
      </>
    ),
  },
  {
    title: "Start the service",
    code: `sudo systemctl daemon-reload
sudo systemctl enable --now referee-server

# If a firewall is enabled, open port 14002 to your LAN (adjust the subnet):
sudo ufw allow from 192.168.1.0/24 to any port 14002 proto tcp
# or, with firewalld:
sudo firewall-cmd --permanent --add-port=14002/tcp && sudo firewall-cmd --reload`,
    note: (
      <>
        On first start the server downloads encoder binaries before it begins listening, which can take a few minutes.
        Follow progress with <code className="rounded bg-muted px-1 py-0.5 font-mono">journalctl -u referee-server -f</code>.
      </>
    ),
  },
  {
    title: "Link from the viewing device",
    code: "curl -s http://<relay-server-ip>:14002/v1/ping",
    note: (
      <>
        Check that the server reports <code className="rounded bg-muted px-1 py-0.5 font-mono">&quot;gpuReady&quot;: true</code>,
        then open <strong className="text-foreground">Settings → Relay</strong> on the viewing device, click{" "}
        <strong className="text-foreground">Scan</strong>, and <strong className="text-foreground">Link</strong> the
        server. Once linked, choose <strong className="text-foreground">Linked peer</strong> as the relay route.
      </>
    ),
  },
]

function CodeBlock({ code }: { code: string }) {
  return (
    <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 px-4 py-3 text-sm font-mono text-foreground">
      <code>{code}</code>
    </pre>
  )
}

function Subsection({ id, icon, title, children }: { id: string; icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <section id={id} className="scroll-mt-28">
      <div className="mb-4 flex items-center gap-3">
        {icon}
        <h3 className="text-xl font-semibold tracking-tight sm:text-2xl">{title}</h3>
      </div>
      {children}
    </section>
  )
}

export function RelayServerService() {
  return (
    <section id="relay-server" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-4 flex items-center gap-3">
        <Server className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Relay Server (Linux)</h2>
      </div>
      <div className="space-y-5">
        <p className="text-sm leading-7 text-muted-foreground sm:text-base">
          Install <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">referee-server</code> as a systemd
          service to dedicate a Linux machine to Relay processing. It runs only the backend pipeline, with no window or
          tray icon. Viewing devices on the LAN link to it and hand off their streams, while your player keeps talking
          to <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">localhost:14002</code> on the viewing
          device. Integrations need no changes; see{" "}
          <a href="#relay-integration" className="text-accent hover:underline">
            Relay Integration
          </a>
          .
        </p>

        <div className="rounded-xl border border-border/60 bg-background/70 p-4">
          <p className="mb-2 text-sm font-medium">Requirements</p>
          <ul className="space-y-1.5">
            {[
              "Linux x64 with systemd and a supported NVIDIA RTX or AMD Radeon GPU on current vendor drivers (AMD needs AMF drivers; Mesa is not supported).",
              "The same /24 subnet as your viewing devices. Scan probes the viewing device's /24 for peers.",
              "Port 14002 free on the server. Relay always connects to peers on 14002, so don't run the desktop app on the same machine.",
            ].map((req) => (
              <li key={req} className="flex items-start gap-2 text-sm leading-6 text-muted-foreground">
                <CheckCircle className="mt-1 h-3.5 w-3.5 flex-shrink-0 text-accent" />
                {req}
              </li>
            ))}
          </ul>
        </div>

        <ol className="space-y-5">
          {installSteps.map((step, i) => (
            <li key={step.title} className="flex gap-4">
              <span className="mt-0.5 flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full bg-accent/10 text-xs font-semibold text-accent">
                {i + 1}
              </span>
              <div className="min-w-0 flex-1">
                <p className="mb-1.5 text-sm font-medium sm:text-base">{step.title}</p>
                <CodeBlock code={step.code} />
                {step.note && <p className="mt-2 text-xs leading-6 text-muted-foreground">{step.note}</p>}
              </div>
            </li>
          ))}
        </ol>
      </div>

      <div className="mt-12 space-y-12">
        <Subsection
          id="relay-server-approval"
          icon={<KeyRound className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Linking without a consent dialog"
        >
          <div className="space-y-4 text-sm leading-7 text-muted-foreground">
            <p>
              When you click <strong className="text-foreground">Link</strong>, the viewing device calls{" "}
              <code>POST /v1/auth/request</code> on the server with the origin{" "}
              <code>https://peer-&lt;instanceId&gt;.referee.invalid</code>, built from its own instance ID. A desktop
              peer would show a consent dialog. The headless server has none, so it answers unknown origins with{" "}
              <code>403 HEADLESS_MODE</code>, and the Link button shows that error. A pre-approved origin receives the
              server&apos;s API token right away as a persistent approval, which is what linking requires.
            </p>
            <p>
              To approve a device without restarting the service, add its origin through the API on the server:
            </p>
            <CodeBlock
              code={`TOKEN=$(sudo jq -r .apiToken ${dataDir}/config.json)
curl -X POST http://localhost:14002/v1/origins \\
  -H "X-Referee-Token: $TOKEN" -H "Content-Type: application/json" \\
  -d '{"origin":"https://peer-<instanceId>.referee.invalid","appName":"Living room PC"}'`}
            />
            <div className="flex items-start gap-3 rounded-xl border border-accent/20 bg-accent/5 p-4">
              <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-accent" />
              <div className="space-y-2">
                <p>
                  <strong className="text-foreground">Every approved origin receives the same API token.</strong> Approve
                  only relay origins for devices you trust. Any other origin you approve can also use the server directly.
                </p>
                <p>
                  Removing an origin with <code>DELETE /v1/origins/&lt;url-encoded origin&gt;</code> only stops new links.
                  A device that is already linked keeps working with the token it saved. To cut it off, change the token:
                  edit <code>REFEREE_API_TOKEN</code> and restart the service, or call{" "}
                  <code>POST /v1/auth/rotate-token</code> if that variable is unset. Then link your remaining devices
                  again. Entries in <code>REFEREE_ALLOWED_ORIGINS</code> are re-approved on every start, so remove
                  revoked devices there too.
                </p>
              </div>
            </div>
          </div>
        </Subsection>

        <Subsection
          id="relay-server-operate"
          icon={<Wrench className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Operating the service"
        >
          <div className="space-y-5 text-sm leading-7 text-muted-foreground">
            <div>
              <p className="mb-2 font-medium text-foreground">Stream settings</p>
              <p className="mb-2">
                While Relay is active, the viewing device&apos;s stream settings are read-only. The server&apos;s settings
                apply instead. Change them with <code>POST /v1/settings/stream</code> before a session starts:
              </p>
              <CodeBlock
                code={`curl -X POST http://localhost:14002/v1/settings/stream \\
  -H "X-Referee-Token: $TOKEN" -H "Content-Type: application/json" \\
  -d '{"resolution":"3840x2160","quality":4,"hdr":true}'`}
              />
            </div>
            <div>
              <p className="mb-2 font-medium text-foreground">Logs and data</p>
              <p>
                <code>journalctl -u referee-server</code> shows console output. Daily log files, <code>config.json</code>,{" "}
                <code>approved-origins.json</code>, and the downloaded encoder binaries live in{" "}
                <code className="break-all">{dataDir}</code>. Set <code>RUST_LOG=referee=debug</code> in the environment
                file for more detail.
              </p>
            </div>
            <div>
              <p className="mb-2 font-medium text-foreground">Updating</p>
              <CodeBlock
                code={`git pull
cargo build --manifest-path desktop/native/Cargo.toml \\
  --features headless --bin referee-server --release
sudo install -m 0755 desktop/native/target/release/referee-server /usr/local/bin/
sudo systemctl restart referee-server`}
              />
              <p className="mt-2">
                The service checks for encoder dependency updates on every start. Linked devices reconnect on their own;
                the link survives restarts and IP changes as long as the API token stays the same.
              </p>
            </div>
            <div>
              <p className="mb-2 font-medium text-foreground">Uninstalling</p>
              <CodeBlock
                code={`sudo systemctl disable --now referee-server
sudo rm /etc/systemd/system/referee-server.service /usr/local/bin/referee-server
sudo systemctl daemon-reload
# Optional: remove data, config, and the service user
sudo rm -r /var/lib/referee /etc/referee && sudo userdel referee`}
              />
            </div>
          </div>
        </Subsection>

        <Subsection
          id="relay-server-unit"
          icon={<FileCode className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="The service unit"
        >
          <div className="space-y-3 text-sm leading-7 text-muted-foreground">
            <p>
              Shipped as <code>packaging/systemd/referee-server.service</code>. <code>KillSignal=SIGINT</code> lets{" "}
              <RefereeWordmark variant="inline" /> stop active GPU sessions cleanly on shutdown. Don&apos;t add{" "}
              <code>PrivateDevices=</code>: it hides the GPU device nodes.
            </p>
            <CodeBlock code={serviceUnit} />
          </div>
        </Subsection>
      </div>
    </section>
  )
}
