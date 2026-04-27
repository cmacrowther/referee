import { RefereeWordmark } from "@/components/referee-wordmark"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Network, Info } from "lucide-react"

export function LanRelay() {
  return (
    <section id="relay" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <Network className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">REFEREE Relay</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        Relay lets you offload stream processing to another <RefereeWordmark variant="inline" />{" "}device on your
        local network. If the computer you&apos;re watching on doesn&apos;t have a supported GPU — or you
        want to keep the processing workload on a separate, more powerful machine — Relay is the solution.
      </p>

      <Alert className="mt-6 border-border/60 bg-secondary/40">
        <Info className="h-4 w-4 text-accent" />
        <AlertDescription className="text-sm text-muted-foreground">
          <p>Both devices must be on the <strong className="text-foreground">same local network</strong> and both must have <RefereeWordmark variant="inline" /> installed and running.</p>
        </AlertDescription>
      </Alert>

      <div className="mt-8 space-y-10">
        {/* Linking a peer */}
        <div>
          <h3 className="mb-3 text-base font-semibold">Linking a peer device</h3>
          <ol className="space-y-3 text-sm leading-7 text-muted-foreground">
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">1.</span>
              <span>Open <strong>Settings → Relay</strong> on the device you want to watch <em>from</em> (your viewing device — this can be any PC on the network, even without a dedicated GPU).</span>
            </li>
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">2.</span>
              <span>Scroll down to <strong>Network Peers</strong> and click the <strong>Scan</strong> (refresh) button. Any other <RefereeWordmark variant="inline" /> instances found on your network will appear in the list.</span>
            </li>
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">3.</span>
              <span>Click <strong>Link</strong> next to the device you want to use for processing. The peer list shows the device&apos;s name, IP address, and GPU readiness — look for <em>Ready for relay</em>.</span>
            </li>
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">4.</span>
              <span>Once linked, the <strong>Relay route</strong> selection will appear at the top of the Relay card, giving you two options (see below).</span>
            </li>
          </ol>
        </div>

        {/* Route selection */}
        <div>
          <h3 className="mb-3 text-base font-semibold">Choosing the route</h3>
          <p className="mb-4 text-sm leading-7 text-muted-foreground">
            After linking a peer, two route options appear at the top of the Relay card:
          </p>
          <div className="space-y-3">
            <div className="rounded-xl border border-border/60 bg-card p-4 text-sm leading-7">
              <p className="mb-1 font-semibold">This REFEREE <span className="text-xs font-normal text-muted-foreground">(Local)</span></p>
              <p className="text-muted-foreground">
                Streams are processed on this device. The linked peer is saved but not used — you can switch to
                it at any time. Stream quality settings remain editable.
              </p>
            </div>
            <div className="rounded-xl border border-accent/25 bg-accent/5 p-4 text-sm leading-7">
              <p className="mb-1 font-semibold">Linked peer <span className="text-xs font-normal text-muted-foreground">(Relay)</span></p>
              <p className="text-muted-foreground">
                Streams are handed off to the linked device for processing. A status pill shows whether the
                peer is <strong className="text-emerald-400">Online</strong> or{" "}
                <strong className="text-red-400">Offline</strong>. Stream settings on this device become
                read-only; they&apos;re managed by the relay device.
              </p>
            </div>
          </div>
        </div>

        {/* Managing links */}
        <div>
          <h3 className="mb-3 text-base font-semibold">Managing trusted devices</h3>
          <p className="text-sm leading-7 text-muted-foreground">
            Linked devices appear under the <strong>Trusted for Relay</strong> section. To remove a link,
            expand that section and click <strong>Unlink</strong> next to the device. The peer will need to
            be linked again to use Relay in the future.
          </p>
        </div>
      </div>
    </section>
  )
}
