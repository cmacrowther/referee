import { RefereeWordmark } from "@/components/referee-wordmark"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Download, Cpu, CheckCircle2, AlertCircle } from "lucide-react"
import Link from "next/link"

export function QuickStart() {
  return (
    <section id="quick-start" className="scroll-mt-28 mt-10 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <Download className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Quick Start</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        Get <RefereeWordmark variant="inline" />{" "}installed and ready in a few minutes.
      </p>

      <div className="mt-8 space-y-8">
        {/* Step 1 */}
        <div>
          <div className="mb-3 flex items-center gap-3">
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-accent/15 text-sm font-bold text-accent">
              1
            </span>
            <h3 className="text-base font-semibold">Download &amp; install</h3>
          </div>
          <p className="ml-10 text-sm leading-7 text-muted-foreground">
            Grab the latest installer from the{" "}
            <Link href="/downloads" className="text-accent hover:underline">
              Downloads page
            </Link>
            . Run the installer and follow the prompts — no configuration needed. <RefereeWordmark variant="inline" />{" "}supports
            Windows 10/11 and Linux.
          </p>
          <div className="ml-10 mt-3 overflow-hidden rounded-xl border border-border/60">
            <div className="bg-secondary/50 px-4 py-2.5 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              System Requirements
            </div>
            <div className="divide-y divide-border/60">
              {[
                ["GPU", "NVIDIA RTX 20-series or newer · AMD Radeon RX 6000-series or newer"],
                ["OS", "Windows 10/11 (64-bit) · Linux (x86_64)"],
                ["RAM", "8 GB or more recommended"],
                ["Drivers", "Up-to-date GPU drivers (NVIDIA Game Ready or AMD Adrenalin)"],
              ].map(([label, value]) => (
                <div key={label} className="flex gap-4 px-4 py-2.5 text-sm">
                  <span className="w-16 shrink-0 font-medium">{label}</span>
                  <span className="text-muted-foreground">{value}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Step 2 */}
        <div>
          <div className="mb-3 flex items-center gap-3">
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-accent/15 text-sm font-bold text-accent">
              2
            </span>
            <h3 className="text-base font-semibold">First launch — automatic setup</h3>
          </div>
          <p className="ml-10 text-sm leading-7 text-muted-foreground">
            When you open <RefereeWordmark variant="inline" />{" "}for the first time, a setup screen walks through three
            phases automatically. You don&apos;t need to do anything — just let it run.
          </p>
          <ol className="ml-10 mt-4 space-y-3">
            {[
              {
                icon: <Cpu className="h-4 w-4 text-accent" />,
                title: "Checking Hardware",
                body: `Detects your GPU model and vendor. If no supported GPU is found, you'll see an “Unknown Hardware” message — check that your drivers are installed and your GPU meets the requirements above.`,
              },
              {
                icon: <Download className="h-4 w-4 text-accent" />,
                title: "Getting Ready",
                body: "Downloads the GPU acceleration libraries for your hardware (NVENC for NVIDIA, AMF for AMD) and the AI upscaling engine. This is a one-time download of around 1–2 GB — it won't happen on future launches.",
              },
              {
                icon: <CheckCircle2 className="h-4 w-4 text-accent" />,
                title: "Setup Complete",
                body: "REFEREE is ready. The main window opens showing the stream status and settings.",
              },
            ].map((step, i) => (
              <li key={i} className="flex gap-3 rounded-xl border border-border/60 bg-card p-4">
                <span className="mt-0.5 shrink-0">{step.icon}</span>
                <div>
                  <p className="mb-1 text-sm font-semibold">{step.title}</p>
                  <p className="text-sm leading-6 text-muted-foreground">{step.body}</p>
                </div>
              </li>
            ))}
          </ol>
          <Alert className="ml-10 mt-4 border-accent/20 bg-accent/5">
            <AlertCircle className="h-4 w-4 text-accent" />
            <AlertDescription className="text-sm text-muted-foreground">
              <p>The first-run download can take a few minutes depending on your internet speed. If it stalls or shows &ldquo;Setup Paused&rdquo;, check your connection and click <strong>Retry</strong>.</p>
            </AlertDescription>
          </Alert>
        </div>

        {/* Step 3 */}
        <div>
          <div className="mb-3 flex items-center gap-3">
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-accent/15 text-sm font-bold text-accent">
              3
            </span>
            <h3 className="text-base font-semibold">Open a stream</h3>
          </div>
          <p className="ml-10 text-sm leading-7 text-muted-foreground">
            Visit any <RefereeWordmark variant="inline" />-compatible website or player with <RefereeWordmark variant="inline" />{" "}running in the background.
            The player will detect the app automatically and ask for your permission (see{" "}
            <a href="#connect-player" className="text-accent hover:underline">
              Connecting a Player
            </a>
            {" "}below). Once connected, your stream will be enhanced in real time.
          </p>
        </div>
      </div>
    </section>
  )
}
