import { RefereeWordmark } from "@/components/referee-wordmark"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { SlidersHorizontal, AlertCircle } from "lucide-react"

export function QualitySettings() {
  return (
    <section id="quality-settings" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <SlidersHorizontal className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Stream Quality</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        These settings control the resolution and quality of the enhanced stream. All of them can be changed
        in the <strong>Settings</strong> panel and take effect on the next stream session.
      </p>

      <Alert className="mt-6 border-accent/20 bg-accent/5">
        <AlertCircle className="h-4 w-4 text-accent" />
        <AlertDescription className="text-sm text-muted-foreground">
          <p>Quality settings are locked while a stream is active. Stop the current stream before making changes, or adjust them before starting a new session.</p>
        </AlertDescription>
      </Alert>

      <div className="mt-8 space-y-6">
        {/* Output Resolution */}
        <div className="overflow-hidden rounded-xl border border-border/60">
          <div className="border-b border-border/60 bg-secondary/50 px-5 py-3">
            <h3 className="text-sm font-semibold">Output Resolution</h3>
          </div>
          <div className="px-5 py-4 text-sm leading-7 text-muted-foreground">
            <p>
              Sets the resolution <RefereeWordmark variant="inline" />{" "}upscales your stream to. Common options are
              1080p, 1440p, and 4K. Your source stream can be lower quality — that&apos;s the whole point.
            </p>
            <p className="mt-2">
              <strong className="text-foreground">Higher = better picture quality, more GPU work.</strong> If your
              GPU is struggling (high temperatures, choppy output), try dropping to 1080p first.
            </p>
          </div>
        </div>

        {/* Upscaling Quality */}
        <div className="overflow-hidden rounded-xl border border-border/60">
          <div className="border-b border-border/60 bg-secondary/50 px-5 py-3">
            <h3 className="text-sm font-semibold">Upscaling Quality (1–4)</h3>
          </div>
          <div className="px-5 py-4">
            <p className="text-sm leading-7 text-muted-foreground">
              Controls the AI model quality used during upscaling. Think of it as a quality-vs-speed slider.
            </p>
            <div className="mt-4 overflow-hidden rounded-lg border border-border/60">
              <table className="w-full text-sm">
                <thead className="bg-secondary/50">
                  <tr>
                    <th className="px-4 py-2.5 text-left font-medium">Level</th>
                    <th className="px-4 py-2.5 text-left font-medium">Picture quality</th>
                    <th className="px-4 py-2.5 text-left font-medium">GPU load</th>
                    <th className="px-4 py-2.5 text-left font-medium">Best for</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/60">
                  <tr>
                    <td className="px-4 py-2.5 font-mono text-accent">1</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Good</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Lowest</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Older GPUs, low-end hardware</td>
                  </tr>
                  <tr>
                    <td className="px-4 py-2.5 font-mono text-accent">2</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Better</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Moderate</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Everyday streaming</td>
                  </tr>
                  <tr>
                    <td className="px-4 py-2.5 font-mono text-accent">3</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Great</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Higher</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Most mid-range GPUs (default)</td>
                  </tr>
                  <tr>
                    <td className="px-4 py-2.5 font-mono text-accent">4</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Best</td>
                    <td className="px-4 py-2.5 text-muted-foreground">Highest</td>
                    <td className="px-4 py-2.5 text-muted-foreground">High-end GPUs (RTX 40/RX 7000)</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>

        {/* Output Bitrate */}
        <div className="overflow-hidden rounded-xl border border-border/60">
          <div className="border-b border-border/60 bg-secondary/50 px-5 py-3">
            <h3 className="text-sm font-semibold">Output Bitrate</h3>
          </div>
          <div className="px-5 py-4 text-sm leading-7 text-muted-foreground">
            <p>
              Controls how much data the enhanced stream uses. A higher bitrate means sharper details but
              requires more bandwidth between your PC and your media player.
            </p>
            <p className="mt-2">
              The typical range is <strong className="text-foreground">4–20 Mbps</strong>. For most home setups,
              a value between 8–12 Mbps is a good balance. If you see blocking artefacts, try increasing the
              bitrate; if the stream stutters, lower it.
            </p>
          </div>
        </div>
      </div>
    </section>
  )
}
