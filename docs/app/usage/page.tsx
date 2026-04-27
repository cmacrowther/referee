import { Navigation } from "@/components/navigation"
import { PageBreadcrumb } from "@/components/page-breadcrumb"
import { UsageSidebar } from "@/components/usage/usage-sidebar"
import { QuickStart } from "@/components/usage/quick-start"
import { ConnectPlayer } from "@/components/usage/connect-player"
import { QualitySettings } from "@/components/usage/quality-settings"
import { GpuFeatures } from "@/components/usage/gpu-features"
import { TrayMode } from "@/components/usage/tray-mode"
import { LanRelay } from "@/components/usage/lan-relay"
import { PerformanceReference } from "@/components/usage/performance-reference"
import { Troubleshooting } from "@/components/usage/troubleshooting"
import { Footer } from "@/components/footer"

export const metadata = {
  title: "Using REFEREE — User Guide",
  description: "How to install, set up, and get the most out of REFEREE — a guide for everyday users.",
}

export default function UsagePage() {
  return (
    <main className="min-h-screen">
      <Navigation />
      <div className="pt-16">
        <div className="mx-auto flex max-w-6xl gap-8 px-6 xl:gap-12">
          <aside className="hidden w-60 shrink-0 self-stretch border-r border-border pr-6 xl:block">
            <div className="sticky top-20 max-h-[calc(100vh-5rem)] overflow-y-auto">
              <UsageSidebar />
            </div>
          </aside>
          <div className="min-w-0 flex-1">
            <PageBreadcrumb page="User Guide" className="pt-8 pb-0" />
            <div className="pt-8 pb-2">
              <h1 className="text-3xl font-bold tracking-tight sm:text-4xl">Using REFEREE</h1>
              <p className="mt-3 text-base leading-7 text-muted-foreground">
                Everything you need to get REFEREE installed, connected, and streaming — no technical
                background required.
              </p>
            </div>
            <QuickStart />
            <ConnectPlayer />
            <QualitySettings />
            <GpuFeatures />
            <TrayMode />
            <LanRelay />
            <PerformanceReference />
            <Troubleshooting />
          </div>
        </div>
      </div>
      <Footer />
    </main>
  )
}
