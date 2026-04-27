import { Button } from "@/components/ui/button"
import { Download, Zap } from "lucide-react"
import Link from "next/link"
import { RefereeWordmark } from "@/components/referee-wordmark"

export function DownloadCTA() {
  return (
    <section className="relative overflow-hidden py-28 px-6">
      {/* dot wave background */}
      <div aria-hidden="true" className="hero-dot-wave-bg" />
      {/* background gradient */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-gradient-to-br from-[#FD690B]/10 via-background/30 to-background/30"
      />
      {/* glow orb */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute left-1/2 top-0 h-[28rem] w-[44rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#FD690B]/10 blur-[120px]"
      />

      <div className="relative max-w-3xl mx-auto text-center">
        <p className="text-xs font-semibold uppercase tracking-widest text-[#FD690B] mb-4">
          Final Whistle
        </p>

        <h2 className="text-4xl sm:text-5xl font-bold tracking-tight mb-6 leading-tight">
          The Call Has Been Made.{" "}
          <span className="relative inline-flex px-1 text-[#FD690B]">
            <span className="relative z-10">Download Now.</span>
            <span
              aria-hidden="true"
              className="absolute -bottom-1 left-0 h-[3px] w-full rounded-full bg-gradient-to-r from-[#FD690B]/30 via-[#FD690B] to-[#FD690B]/60"
            />
          </span>
        </h2>

        <p className="text-lg text-muted-foreground mb-10 max-w-xl mx-auto">
          Stop overpaying for bandwidth. Give your users GPU-powered{" "}
          <RefereeWordmark variant="inline" /> quality — right from their own machine.
        </p>

        <div className="flex flex-col sm:flex-row items-center justify-center gap-3 mb-12">
          <Button
            size="lg"
            variant="referee"
            className="w-full sm:w-auto"
            asChild
          >
            <Link href="/downloads">
              <Download className="mr-2 h-5 w-5" />
              Download Referee
            </Link>
          </Button>
        </div>
      </div>
    </section>
  )
}
