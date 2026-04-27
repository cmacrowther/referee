"use client"

import type { ReactNode } from "react"

import { RefereeWordmark } from "@/components/referee-wordmark"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Download, Terminal } from "lucide-react"
import Link from "next/link"

type Step = {
  number: string
  title: string
  description: ReactNode
}

const userSteps: Step[] = [
  {
    number: "01",
    title: "Download REFEREE",
    description:
      "Grab the desktop app for your platform and get it ready on the machine where you want better playback.",
  },
  {
    number: "02",
    title: "Install and Launch It",
    description: (
      <>
        Open the installer, finish setup, and launch REFEREE. It stays ready in the
        background until a compatible player calls on it.
      </>
    ),
  },
  {
    number: "03",
    title: "Open a Compatible Player",
    description:
      "Visit the live demo or another REFEREE-compatible player in your browser. In most cases, it will detect the app automatically.",
  },
  {
    number: "04",
    title: "Watch the Enhanced Stream",
    description:
      "If your system has supported NVIDIA or AMD hardware, REFEREE improves playback locally on your machine while you watch.",
  },
]

const developerSteps: Step[] = [
  {
    number: "01",
    title: "Web Player Connection",
    description: (
      <>
        Your web-based media player detects REFEREE running locally and establishes a
        connection through the local REFEREE API.
      </>
    ),
  },
  {
    number: "02",
    title: "Session Initialization",
    description:
      "The player POSTs the source stream URL to /v1/stream/start with an X-Referee-Token header and receives a session ID plus an absolute local HLS playback URL.",
  },
  {
    number: "03",
    title: "Pipeline Startup",
    description: (
      <>
        REFEREE launches the selected hardware encoder, pulls the source stream directly,
        and keeps the session alive while the player sends heartbeats.
      </>
    ),
  },
  {
    number: "04",
    title: "GPU Enhancement",
    description:
      "Supported NVIDIA RTX and AMD Radeon hardware apply upscaling, optional frame generation, and HDR enhancement in real time.",
  },
  {
    number: "05",
    title: "Enhanced Output",
    description:
      "Your web player loads the returned local HLS URL and can stop the session with /v1/stream/stop when playback ends.",
  },
]

function StepList({ steps }: { steps: Step[] }) {
  return (
    <div className="mx-auto max-w-3xl space-y-5">
      {steps.map((step) => (
        <div
          key={step.number}
          className="grid gap-4 rounded-2xl border border-border/70 bg-background/80 p-5 shadow-[0_8px_24px_rgba(0,0,0,0.05)] sm:grid-cols-[3rem_minmax(0,1fr)] sm:gap-5 sm:p-6"
        >
          <div className="flex-shrink-0">
            <div className="flex h-12 w-12 items-center justify-center rounded-full border border-accent/20 bg-accent/10">
              <span className="text-sm font-mono font-semibold text-accent">{step.number}</span>
            </div>
          </div>
          <div className="sm:pt-1">
            <h3 className="mb-2 text-base font-semibold">{step.title}</h3>
            <p className="max-w-2xl text-sm leading-relaxed text-muted-foreground">
              {step.description}
            </p>
          </div>
        </div>
      ))}
    </div>
  )
}

function FlowCta({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <div className="mx-auto mt-12 max-w-3xl">
      <div className="border-t border-border/70 pt-6">
        <div className="mx-auto flex max-w-sm flex-col items-center gap-4 text-center">
          <p className="text-sm font-medium text-muted-foreground">{label}</p>
          {children}
        </div>
      </div>
    </div>
  )
}

export function HowItWorks() {
  return (
    <section id="how-it-works" className="bg-card/50 px-6 py-24">
      <div className="mx-auto max-w-4xl">
        <div className="mb-16 text-center">
          <h2 className="mb-4 text-3xl font-bold tracking-tight">The Official Review</h2>
          <p className="mx-auto max-w-2xl text-muted-foreground">
            Pick the path that matches how you&apos;re using <RefereeWordmark variant="inline" />.
            Whether you want better playback for yourself or need to wire it into your app,
            here&apos;s how to get started.
          </p>
        </div>

        <Tabs defaultValue="user" className="w-full">
          <TabsList className="mx-auto mb-12 grid h-auto w-full max-w-lg grid-cols-2 gap-2 rounded-xl border border-border bg-background/80 p-2">
            <TabsTrigger
              value="user"
              className="h-auto w-full rounded-lg px-4 py-2.5 text-center data-[state=active]:border-accent/30 data-[state=active]:bg-accent/10 data-[state=active]:text-accent"
            >
              Are you a user?
            </TabsTrigger>
            <TabsTrigger
              value="developer"
              className="h-auto w-full rounded-lg px-4 py-2.5 text-center data-[state=active]:border-accent/30 data-[state=active]:bg-accent/10 data-[state=active]:text-accent"
            >
              Are you a developer?
            </TabsTrigger>
          </TabsList>

          <TabsContent value="user" className="mt-0">
            <StepList steps={userSteps} />
            <FlowCta label="Start enhancing your streams today.">
              <Button size="lg" variant="outline" className="w-full sm:w-auto" asChild>
                <Link href="/downloads">
                  <Download className="h-4 w-4" />
                  Go to Downloads
                </Link>
              </Button>
            </FlowCta>
          </TabsContent>

          <TabsContent value="developer" className="mt-0">
            <StepList steps={developerSteps} />
            <FlowCta label="Ready to build enhanced playback into your app?">
              <Button size="lg" variant="outline" className="w-full sm:w-auto" asChild>
                <Link href="/developers">
                  <Terminal className="h-4 w-4" />
                  Open Integration Guide
                </Link>
              </Button>
            </FlowCta>
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}
