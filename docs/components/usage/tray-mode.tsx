import { RefereeWordmark } from "@/components/referee-wordmark"
import { AppWindow, Bell, Power, Minimize2 } from "lucide-react"

export function TrayMode() {
  return (
    <section id="tray-mode" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <AppWindow className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Background &amp; Tray Mode</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        <RefereeWordmark variant="inline" />{" "}is designed to run quietly in the background. These settings control
        when and how the app shows itself — all found under <strong>Settings → Application</strong>.
      </p>

      <div className="mt-8 space-y-4">
        {[
          {
            icon: <Power className="h-4 w-4 text-accent" />,
            title: "Start on Boot",
            body: "Launches REFEREE silently when Windows starts. The window stays hidden — the app is ready to accept streams as soon as you log in, without you needing to open it manually.",
          },
          {
            icon: <Minimize2 className="h-4 w-4 text-accent" />,
            title: "Minimize to Tray",
            body: "When you click the minimize button, the window hides to the system tray instead of showing on the taskbar. Click the tray icon to bring it back.",
          },
          {
            icon: <Minimize2 className="h-4 w-4 text-accent" />,
            title: "Close to Tray",
            body: "When you click the close (✕) button, REFEREE keeps running in the background instead of quitting. This is recommended — it means the app is always ready to handle streams. To actually quit, right-click the tray icon and choose Exit.",
          },
          {
            icon: <AppWindow className="h-4 w-4 text-accent" />,
            title: "Show on Stream Start",
            body: "Automatically pops the REFEREE window to the front whenever a new stream begins. Useful if you want to monitor GPU usage or adjust settings quickly after a stream starts.",
          },
          {
            icon: <Bell className="h-4 w-4 text-accent" />,
            title: "Stream Notifications",
            body: "Sends a desktop notification when a stream starts or stops. Helpful when REFEREE is minimised and you want a quick heads-up without checking the window.",
          },
        ].map((item) => (
          <div key={item.title} className="flex gap-3 rounded-xl border border-border/60 bg-card p-4">
            <span className="mt-0.5 shrink-0">{item.icon}</span>
            <div>
              <p className="mb-1 text-sm font-semibold">{item.title}</p>
              <p className="text-sm leading-6 text-muted-foreground">{item.body}</p>
            </div>
          </div>
        ))}
      </div>

      <p className="mt-6 text-sm leading-7 text-muted-foreground">
        <strong className="text-foreground">Recommended setup for everyday use:</strong> enable{" "}
        <em>Start on Boot</em>, <em>Close to Tray</em>, and <em>Stream Notifications</em>.{" "}
        <RefereeWordmark variant="inline" /> then starts with Windows, stays out of your way, and lets you
        know when something is happening.
      </p>
    </section>
  )
}
