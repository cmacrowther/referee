import { RefereeWordmark } from "@/components/referee-wordmark"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { MonitorPlay, Shield, Info } from "lucide-react"

export function ConnectPlayer() {
  return (
    <section id="connect-player" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <MonitorPlay className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Connecting a Player</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        <RefereeWordmark variant="inline" />{" "}works with any compatible website or video player. Here&apos;s what
        to expect when a player tries to connect.
      </p>

      <div className="mt-8 space-y-10">
        {/* Consent flow */}
        <div>
          <h3 className="mb-3 text-base font-semibold">The consent prompt</h3>
          <p className="text-sm leading-7 text-muted-foreground">
            The first time a website asks to use <RefereeWordmark variant="inline" />, a consent dialog pops up in
            the app showing the site&apos;s name and address. You have two choices:
          </p>
          <div className="mt-4 space-y-3">
            <div className="flex gap-3 rounded-xl border border-border/60 bg-card p-4">
              <Shield className="mt-0.5 h-4 w-4 shrink-0 text-accent" />
              <div>
                <p className="mb-1 text-sm font-semibold">Allow</p>
                <p className="text-sm leading-6 text-muted-foreground">
                  Grants access for this session only. The next time you visit the site it will prompt again.
                </p>
              </div>
            </div>
            <div className="flex gap-3 rounded-xl border border-accent/25 bg-accent/5 p-4">
              <Shield className="mt-0.5 h-4 w-4 shrink-0 text-accent" />
              <div>
                <p className="mb-1 text-sm font-semibold">Always Allow</p>
                <p className="text-sm leading-6 text-muted-foreground">
                  Saves the site permanently. Future visits connect instantly without prompting. You can revoke
                  access at any time under <strong>Settings → Approved Origins</strong>.
                </p>
              </div>
            </div>
          </div>
        </div>

        {/* Media player auto-launch */}
        <div>
          <h3 className="mb-3 text-base font-semibold">Auto-launching a media player</h3>
          <p className="text-sm leading-7 text-muted-foreground">
            <RefereeWordmark variant="inline" />{" "}can automatically open your preferred media player (VLC, MPV,
            PotPlayer, or a custom app) whenever a stream starts. To enable this:
          </p>
          <ol className="mt-4 space-y-2 text-sm leading-7 text-muted-foreground">
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">1.</span>
              Open <strong>Settings</strong> in the <RefereeWordmark variant="inline" />{" "}window.
            </li>
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">2.</span>
              Find the <strong>Player</strong> card and toggle <strong>Auto-Launch Player</strong> on.
            </li>
            <li className="flex gap-2">
              <span className="shrink-0 font-semibold text-foreground">3.</span>
              Select your preferred player from the dropdown. Installed players are detected automatically.
              If yours isn&apos;t listed, choose <strong>Custom</strong> and browse to its executable.
            </li>
          </ol>
          <Alert className="mt-4 border-border/60 bg-secondary/40">
            <Info className="h-4 w-4 text-accent" />
            <AlertDescription className="text-sm text-muted-foreground">
              <p>You can also open the stream manually in any HLS-compatible player by copying the URL shown in the <RefereeWordmark variant="inline" /> status view and pasting it into your player.</p>
            </AlertDescription>
          </Alert>
        </div>

        {/* Approved origins */}
        <div>
          <h3 className="mb-3 text-base font-semibold">Managing approved sites</h3>
          <p className="text-sm leading-7 text-muted-foreground">
            To see which sites have permanent access, open <strong>Settings → Approved Origins</strong>. From
            there you can remove any site — it will have to ask for permission the next time it connects.
          </p>
        </div>
      </div>
    </section>
  )
}
