import { Gauge } from "lucide-react"

export function PerformanceReference() {
  return (
    <section id="performance" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <Gauge className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Performance Reference</h2>
      </div>
      <p className="mb-6 text-sm leading-7 text-muted-foreground sm:text-base">
        Typical values for reference. Actual results vary by GPU model, output resolution, and enabled features.
      </p>
      <div className="overflow-hidden rounded-xl border border-border/60">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[36rem] text-sm">
            <thead className="bg-secondary/50">
              <tr>
                <th className="px-4 py-3 text-left font-medium">What you&apos;re measuring</th>
                <th className="px-4 py-3 text-left font-medium">Typical value</th>
                <th className="px-4 py-3 text-left font-medium">Notes</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              <tr>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Delay between source and playback</td>
                <td className="px-4 py-3 align-top font-mono text-accent">3–6 s</td>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">
                  Built-in to the HLS streaming format — not something REFEREE adds. Live streams have
                  this regardless.
                </td>
              </tr>
              <tr>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Time until stream appears</td>
                <td className="px-4 py-3 align-top font-mono text-accent">&lt; 5 s warm · up to 3 min cold</td>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">
                  Cold start only happens on the very first run while libraries download. Every launch after
                  that is fast.
                </td>
              </tr>
              <tr>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">GPU memory used</td>
                <td className="px-4 py-3 align-top font-mono text-accent">300 MB – 1 GB</td>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">
                  Scales with output resolution. 1080p uses roughly 400 MB; 4K uses roughly 900 MB.
                </td>
              </tr>
              <tr>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Stream data rate</td>
                <td className="px-4 py-3 align-top font-mono text-accent">4–20 Mbps</td>
                <td className="px-4 py-3 align-top leading-6 text-muted-foreground">
                  Configurable in settings. Higher means sharper output but requires more bandwidth between
                  your PC and your player.
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </section>
  )
}
