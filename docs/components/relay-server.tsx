import type { ReactNode } from "react"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { AlertCircle, CheckCircle, FileCode, Hammer, KeyRound, RefreshCw, Server, Wrench } from "lucide-react"

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
const releaseAsset = "https://github.com/cmacrowther/referee/releases/latest/download/referee-server-linux-x86_64.tar.gz"
const linkClass = "text-accent hover:underline"
const inlineCode = "rounded bg-muted px-1 py-0.5 font-mono"

type Step = {
  title: string
  code: string
  note?: ReactNode
}

const installSteps: Step[] = [
  {
    title: "Install runtime dependencies",
    code: "sudo apt install ca-certificates curl jq libvulkan1 pciutils",
    note: (
      <>
        Debian/Ubuntu shown. On Fedora: <code className={inlineCode}>sudo dnf install curl jq vulkan-loader pciutils</code>.{" "}
        <code className={inlineCode}>pciutils</code> and <code className={inlineCode}>libvulkan1</code> are used for GPU
        detection and libplacebo; <code className={inlineCode}>curl</code> and <code className={inlineCode}>jq</code> are
        used by the updater.
      </>
    ),
  },
  {
    title: "Download the latest release",
    code: `curl -fL ${releaseAsset} | tar -xz
cd referee-server-linux-x86_64`,
    note: (
      <>
        Contains <code className={inlineCode}>referee-server</code>, the <code className={inlineCode}>rife-worker</code>{" "}
        frame generation helper, the <code className={inlineCode}>referee-server-update</code> script, and the systemd
        units. To compile it yourself, see{" "}
        <a href="#relay-server-source" className={linkClass}>
          Building from source
        </a>
        .
      </>
    ),
  },
  {
    title: "Install the binaries, updater, and units",
    code: `sudo install -m 0755 referee-server rife-worker /usr/local/bin/
sudo install -m 0755 referee-server-update /usr/local/sbin/
sudo install -m 0644 referee-server.service referee-server-update.service \\
  referee-server-update.timer /etc/systemd/system/
sudo install -D -m 0600 referee-server.env /etc/referee/referee-server.env`,
    note: (
      <>
        The environment file holds <code className={inlineCode}>REFEREE_API_TOKEN</code>,{" "}
        <code className={inlineCode}>REFEREE_ALLOWED_ORIGINS</code>, and{" "}
        <code className={inlineCode}>REFEREE_AUTO_UPDATE</code>, so keep it readable by root only. systemd reads it before
        dropping to the <code className={inlineCode}>referee</code> user.
      </>
    ),
  },
  {
    title: "Create the service user",
    code: `sudo useradd --system --home-dir /var/lib/referee --shell /usr/sbin/nologin referee
sudo usermod -aG video,render referee`,
    note: (
      <>
        The <code className={inlineCode}>video</code> and <code className={inlineCode}>render</code> groups own{" "}
        <code className={inlineCode}>/dev/dri</code> and <code className={inlineCode}>/dev/kfd</code> on most distros. If
        your distro has no <code className={inlineCode}>render</code> group, leave it out. systemd creates{" "}
        <code className={inlineCode}>/var/lib/referee</code> on first start.
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
        <a href="#relay-server-approval" className={linkClass}>
          Linking without a consent dialog
        </a>{" "}
        for why this step is needed. To have updates install automatically, also set{" "}
        <code className={inlineCode}>REFEREE_AUTO_UPDATE=true</code> (see{" "}
        <a href="#relay-server-updates" className={linkClass}>
          Updates
        </a>
        ).
      </>
    ),
  },
  {
    title: "Start the service and update checks",
    code: `sudo systemctl daemon-reload
sudo systemctl enable --now referee-server referee-server-update.timer

# If a firewall is enabled, open port 14002 to your LAN (adjust the subnet):
sudo ufw allow from 192.168.1.0/24 to any port 14002 proto tcp
# or, with firewalld:
sudo firewall-cmd --permanent --add-port=14002/tcp && sudo firewall-cmd --reload`,
    note: (
      <>
        On first start the server downloads encoder binaries before it begins listening, which can take a few minutes.
        Follow progress with <code className={inlineCode}>journalctl -u referee-server -f</code>.
      </>
    ),
  },
  {
    title: "Link from the viewing device",
    code: "curl -s http://<relay-server-ip>:14002/v1/ping",
    note: (
      <>
        Check that the server reports <code className={inlineCode}>&quot;gpuReady&quot;: true</code>, then open{" "}
        <strong className="text-foreground">Settings → Relay</strong> on the viewing device, click{" "}
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

function Labeled({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <p className="mb-2 font-medium text-foreground">{label}</p>
      {children}
    </div>
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
          <a href="#relay-integration" className={linkClass}>
            Relay Integration
          </a>
          .
        </p>

        <div className="rounded-xl border border-border/60 bg-background/70 p-4">
          <p className="mb-2 text-sm font-medium">Requirements</p>
          <ul className="space-y-1.5">
            {[
              "Linux x86_64 with systemd and glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36, or later).",
              "A supported NVIDIA RTX or AMD Radeon GPU on current vendor drivers (AMD needs AMF drivers; Mesa is not supported).",
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
          id="relay-server-updates"
          icon={<RefreshCw className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Updates"
        >
          <div className="space-y-5 text-sm leading-7 text-muted-foreground">
            <p>
              <code>referee-server-update.timer</code> checks GitHub for a newer stable release every 6 hours. By default
              it only logs that an update is available, which you can read with{" "}
              <code>journalctl -u referee-server-update</code>. To install updates automatically, set this in{" "}
              <code>/etc/referee/referee-server.env</code>. The updater reads the file on every check, so nothing needs
              restarting:
            </p>
            <CodeBlock code="REFEREE_AUTO_UPDATE=true" />
            <Labeled label="Updating by hand">
              <CodeBlock
                code={`referee-server-update --check      # report whether an update is available
sudo referee-server-update         # install it now
sudo referee-server-update --now   # install even if streams are active`}
              />
              <p className="mt-2">
                Without <code>--now</code>, a manual update refuses to run while a stream is active, because restarting
                the server ends it.
              </p>
            </Labeled>
            <Labeled label="What an update does">
              <ol className="list-decimal space-y-1.5 pl-5">
                <li>
                  Picks the newest stable release that publishes <code>referee-server-linux-x86_64.tar.gz</code>, and
                  stops if the running server already reports that version on <code>/v1/ping</code>.
                </li>
                <li>
                  Downloads it and checks it against the SHA-256 digest GitHub recorded for the asset. Nothing is
                  installed if the digest or the bundled version doesn&apos;t match.
                </li>
                <li>
                  In automatic mode, waits until no stream has run for 5 minutes. If streams keep running for an hour,
                  it tries again at the next check.
                </li>
                <li>
                  Replaces <code>referee-server</code> and <code>rife-worker</code> in <code>/usr/local/bin</code>,
                  keeping the previous copies in <code>/var/lib/referee-server-update</code>, and restarts the service.
                </li>
                <li>
                  Waits up to 10 minutes for the server to report the new version. If it doesn&apos;t, the previous
                  binaries are restored and the service restarts on them.
                </li>
                <li>
                  Updates <code>referee-server-update</code> itself. Unit files and the environment file are left alone,
                  since you may have changed them. If a release ships different unit files, the log names them and
                  shows a <code>diff</code> command to review the change.
                </li>
              </ol>
            </Labeled>
            <p>
              <code>systemctl list-timers referee-server-update.timer</code> shows when the next check runs. The digest
              check catches a corrupted or incomplete download. Like the desktop app&apos;s updates, it trusts the
              releases published on GitHub.
            </p>
          </div>
        </Subsection>

        <Subsection
          id="relay-server-operate"
          icon={<Wrench className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Operating the service"
        >
          <div className="space-y-5 text-sm leading-7 text-muted-foreground">
            <Labeled label="Stream settings">
              <p className="mb-2">
                While Relay is active, the viewing device&apos;s stream settings are read-only. The server&apos;s settings
                apply instead. Change them with <code>POST /v1/settings/stream</code> before a session starts:
              </p>
              <CodeBlock
                code={`curl -X POST http://localhost:14002/v1/settings/stream \\
  -H "X-Referee-Token: $TOKEN" -H "Content-Type: application/json" \\
  -d '{"resolution":"3840x2160","quality":4,"hdr":true}'`}
              />
            </Labeled>
            <Labeled label="Logs and data">
              <p>
                <code>journalctl -u referee-server</code> shows console output. Daily log files, <code>config.json</code>,{" "}
                <code>approved-origins.json</code>, and the downloaded encoder binaries live in{" "}
                <code className="break-all">{dataDir}</code>. Set <code>RUST_LOG=referee=debug</code> in the environment
                file for more detail. The service checks for encoder dependency updates on every start, and linked
                devices reconnect after restarts and IP changes as long as the API token stays the same.
              </p>
            </Labeled>
            <Labeled label="Uninstalling">
              <CodeBlock
                code={`sudo systemctl disable --now referee-server referee-server-update.timer
sudo rm /etc/systemd/system/referee-server.service \\
  /etc/systemd/system/referee-server-update.service \\
  /etc/systemd/system/referee-server-update.timer \\
  /usr/local/bin/referee-server /usr/local/bin/rife-worker \\
  /usr/local/sbin/referee-server-update
sudo systemctl daemon-reload
# Optional: remove data, config, update backups, and the service user
sudo rm -r /var/lib/referee /var/lib/referee-server-update /etc/referee && sudo userdel referee`}
              />
            </Labeled>
          </div>
        </Subsection>

        <Subsection
          id="relay-server-source"
          icon={<Hammer className="h-5 w-5 flex-shrink-0 text-accent" />}
          title="Building from source"
        >
          <div className="space-y-4 text-sm leading-7 text-muted-foreground">
            <p>
              Build the binary yourself in place of steps 1–3, then continue from step 4. Install Rust with{" "}
              <a href="https://rustup.rs" target="_blank" rel="noopener noreferrer" className={linkClass}>
                rustup
              </a>
              . <code>tauri-build</code> runs during the build, so the WebKitGTK headers are needed even for the
              headless binary. Debian/Ubuntu packages are shown; for other distros, use the names from{" "}
              <a
                href="https://v2.tauri.app/start/prerequisites/#linux"
                target="_blank"
                rel="noopener noreferrer"
                className={linkClass}
              >
                Tauri&apos;s Linux prerequisites
              </a>
              .
            </p>
            <CodeBlock
              code={`sudo apt install build-essential pkg-config libssl-dev libgtk-3-dev \\
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \\
  ca-certificates curl jq libvulkan1 pciutils
git clone https://github.com/cmacrowther/referee.git && cd referee
cargo build --manifest-path desktop/native/Cargo.toml \\
  --features headless --bin referee-server --release

sudo install -m 0755 desktop/native/target/release/referee-server /usr/local/bin/
sudo install -m 0755 packaging/systemd/referee-server-update /usr/local/sbin/
sudo install -m 0644 packaging/systemd/referee-server.service \\
  packaging/systemd/referee-server-update.service \\
  packaging/systemd/referee-server-update.timer /etc/systemd/system/
sudo install -D -m 0600 packaging/systemd/referee-server.env /etc/referee/referee-server.env`}
            />
            <p>
              For RIFE frame generation, run <code>node scripts/fetch-rife-vendor.js</code>, build{" "}
              <code>desktop/native/rife-worker</code> with <code>cargo build --release --locked</code> (needs CMake), and
              install its <code>rife-worker</code> binary into <code>/usr/local/bin</code>.
            </p>
            <p>
              The updater installs release builds. If you want to keep running your own build, leave{" "}
              <code>referee-server-update.timer</code> disabled and update by rebuilding, reinstalling the binary, and
              running <code>sudo systemctl restart referee-server</code>.
            </p>
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
