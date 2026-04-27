"use client"

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { CircleHelp } from "lucide-react"

const faqs = [
  {
    question: "My /v1/stream/start call is timing out (504). What's wrong?",
    answer: (
      <>
        A <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">504</code> means
        the pipeline timed out before producing HLS output. Common causes:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>The source URL is unreachable or requires authentication — verify the URL loads in a browser tab.</li>
          <li>The GPU is already busy with another process — check GPU utilization before starting.</li>
          <li>Cold-start encoder binary download is still in progress — this only affects the very first run. Wait a few minutes and retry.</li>
        </ul>
        Use <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">GET /v1/status</code> to confirm{" "}
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">gpuReady: true</code> before
        calling <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">/v1/stream/start</code>.
      </>
    ),
  },
  {
    question: "What happens if I stop sending heartbeats?",
    answer: (
      <>
        Two timers govern session cleanup:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li>Once heartbeats have started: the session is killed after <strong>15 seconds</strong> of silence. GPU resources are released immediately.</li>
          <li>Before the first heartbeat arrives: a <strong>5-minute</strong> grace period applies, giving your client time to start its heartbeat loop after <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">/v1/stream/start</code> returns.</li>
        </ul>
        In either case, HLS segments are deleted and the session disappears from{" "}
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">GET /v1/status</code>.
        <br /><br />
        You will naturally observe a{" "}
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">404</code> on the next
        heartbeat attempt once cleanup has occurred. Handle it by stopping the heartbeat interval and
        falling back to native playback or prompting the user to restart.
      </>
    ),
  },
  {
    question: "Can multiple users share one REFEREE instance simultaneously?",
    answer: (
      <>
        No. <RefereeWordmark variant="inline" /> supports <strong>one active session at a time</strong>.
        Starting a new session via <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">POST /v1/stream/start</code> automatically
        stops any existing one.
        <br /><br />
        For multi-user scenarios you have two options:
        <ul className="mt-2 space-y-1 list-disc list-inside text-muted-foreground">
          <li><strong>One instance per user</strong> — each user runs their own desktop app or headless container on their own machine. This is the intended model.</li>
          <li><strong>Shared session (read-only)</strong> — if multiple clients want to watch the same stream, they can all load the same HLS URL (<code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">/v1/tmp/{"{"}sessionId{"}"}/index.m3u8</code>), but only the session owner should send heartbeats and call <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">/v1/stream/stop</code>.</li>
        </ul>
      </>
    ),
  },
]

export function IntegrateFAQ() {
  return (
    <section id="faq" className="scroll-mt-28 mt-16 border-t border-border/60 mb-12 pt-10 pb-0">
      <div>
        <div className="mb-10">
          <div className="mb-4 flex items-center gap-3">
            <CircleHelp className="h-5 w-5 flex-shrink-0 text-accent" />
            <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">FAQ</h2>
          </div>
          <p className="text-sm leading-7 text-muted-foreground sm:text-base">
            Common questions from developers integrating <RefereeWordmark variant="inline" />.
          </p>
        </div>

        <Accordion type="single" collapsible className="space-y-4">
          {faqs.map((faq, index) => (
            <AccordionItem
              key={index}
              value={`faq-${index}`}
              className="overflow-hidden rounded-xl border border-border/60 bg-background/80 px-5 sm:px-6"
            >
              <AccordionTrigger className="py-5 text-left hover:no-underline">
                <span className="text-base font-medium leading-7">{faq.question}</span>
              </AccordionTrigger>
              <AccordionContent className="pb-6 text-sm leading-7 text-muted-foreground [&_li]:pl-1 [&_ul]:mt-3 [&_ul]:list-disc [&_ul]:space-y-2 [&_ul]:pl-5">
                {faq.answer}
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </div>
    </section>
  )
}
