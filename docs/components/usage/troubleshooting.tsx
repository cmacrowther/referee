"use client"

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { LifeBuoy } from "lucide-react"

const issues = [
  {
    question: `REFEREE says “Unknown Hardware” or won't start setup`,
    answer: (
      <>
        This means <RefereeWordmark variant="inline" /> could not find a compatible GPU. Check that:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Your GPU is NVIDIA RTX 20-series or newer, or AMD Radeon RX 6000-series or newer.</li>
          <li>Your GPU drivers are up to date — download the latest from NVIDIA or AMD&apos;s website.</li>
          <li>Your GPU is the active display adapter (not disabled in Device Manager).</li>
        </ul>
        If setup was interrupted partway through, click <strong>Retry</strong> on the setup screen.
      </>
    ),
  },
  {
    question: `Setup shows \u201cSetup Paused\u201d or the download stalls`,
    answer: (
      <>
        The first-run download requires around 1–2 GB. Common causes of a stall:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>No internet connection — check that your network is working.</li>
          <li>Insufficient disk space — free up at least 3 GB and retry.</li>
          <li>Antivirus or firewall blocking the download — temporarily pause it and retry, then add an exception for <RefereeWordmark variant="inline" /> afterwards.</li>
        </ul>
        Click <strong>Retry</strong> on the setup screen to resume from where it left off.
      </>
    ),
  },
  {
    question: "The stream takes a long time to start (or never starts)",
    answer: (
      <>
        A slow start on the very first run after install is normal — the GPU libraries are still being downloaded.
        Wait a few minutes and try again.
        <br /><br />
        After setup is complete, if a stream still won&apos;t start:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Check that the source video URL is reachable — try opening it directly in your browser.</li>
          <li>Make sure the GPU isn&apos;t already at full load from another application.</li>
          <li>Restart <RefereeWordmark variant="inline" /> and try again.</li>
        </ul>
      </>
    ),
  },
  {
    question: "The stream has noticeable delay or is choppy",
    answer: (
      <>
        A 3–6 second delay is normal for HLS streaming and is not specific to <RefereeWordmark variant="inline" />.
        If the stream is choppy or dropping frames:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Lower the <strong>Output Resolution</strong> (try 1080p instead of 4K).</li>
          <li>Reduce the <strong>Upscaling Quality</strong> setting (try level 2 or 3).</li>
          <li>Disable <strong>Frame Generation</strong> if enabled — it significantly increases GPU workload.</li>
          <li>Check that your local network has enough bandwidth for the configured output bitrate.</li>
          <li>Ensure no other heavy GPU processes are running at the same time.</li>
        </ul>
      </>
    ),
  },
  {
    question: "Frame Generation or TrueHDR is greyed out",
    answer: (
      <>
        These features require an <strong>NVIDIA RTX GPU</strong>. They are unavailable on AMD hardware and
        will be greyed out automatically.
        <br /><br />
        If you have an NVIDIA RTX GPU and the options are still greyed out:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Make sure your NVIDIA drivers are fully up to date.</li>
          <li>Confirm that setup completed successfully — reopen <RefereeWordmark variant="inline" /> and check the status screen.</li>
          <li>For TrueHDR specifically, also ensure that HDR is enabled in Windows display settings.</li>
        </ul>
      </>
    ),
  },
  {
    question: "Auto-launch player doesn't open, or opens the wrong app",
    answer: (
      <>
        If the auto-launch player feature isn&apos;t working:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Open <strong>Settings → Player</strong> and confirm <strong>Auto-Launch Player</strong> is toggled on.</li>
          <li>Check the selected player in the dropdown — if your player isn&apos;t listed, choose <strong>Custom</strong> and browse to the player&apos;s executable file.</li>
          <li>If the selected player is listed but doesn&apos;t open, try clicking <strong>Detect Players</strong> to refresh the list.</li>
        </ul>
      </>
    ),
  },
  {
    question: "The player or website can't connect to REFEREE",
    answer: (
      <>
        If a website or player can&apos;t find <RefereeWordmark variant="inline" />:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Make sure <RefereeWordmark variant="inline" /> is actually running — check your system tray.</li>
          <li>Check that your firewall isn&apos;t blocking local connections on port 14002. Try temporarily disabling it to test.</li>
          <li>If you dismissed a consent prompt, the site may need you to refresh the page and try again.</li>
          <li>If you&apos;re on a browser, check the browser console for a CORS error — this usually means the browser is serving the page over HTTPS while trying to connect to the local HTTP server. Either switch the page to HTTP or use a reverse proxy.</li>
        </ul>
      </>
    ),
  },
  {
    question: "A relay peer isn't showing up in the Network Peers list",
    answer: (
      <>
        If the scan doesn&apos;t find your other device:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Confirm that both devices are on the <strong>same local network</strong> (same router/switch, not separated by a VPN or guest Wi-Fi).</li>
          <li>Make sure <RefereeWordmark variant="inline" /> is running on the other device.</li>
          <li>Click the <strong>Scan</strong> (refresh) button again — discovery can take a few seconds.</li>
          <li>Check that your firewall on the other device allows incoming connections on port 14002.</li>
        </ul>
      </>
    ),
  },
]

export function Troubleshooting() {
  return (
    <section id="troubleshooting" className="scroll-mt-28 mt-16 border-t border-border/60 mb-12 pt-10 pb-0">
      <div>
        <div className="mb-10">
          <div className="mb-4 flex items-center gap-3">
            <LifeBuoy className="h-5 w-5 flex-shrink-0 text-accent" />
            <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Troubleshooting</h2>
          </div>
          <p className="text-sm leading-7 text-muted-foreground sm:text-base">
            Common problems and how to fix them.
          </p>
        </div>

        <Accordion type="single" collapsible className="space-y-4">
          {issues.map((item, index) => (
            <AccordionItem
              key={index}
              value={`issue-${index}`}
              className="overflow-hidden rounded-xl border border-border/60 bg-background/80 px-5 sm:px-6"
            >
              <AccordionTrigger className="py-5 text-left hover:no-underline">
                <span className="text-base font-medium leading-7">{item.question}</span>
              </AccordionTrigger>
              <AccordionContent className="pb-6 text-sm leading-7 text-muted-foreground [&_li]:pl-1 [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:space-y-2 [&_ul]:pl-5">
                {item.answer}
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </div>
    </section>
  )
}
