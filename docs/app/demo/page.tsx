import { Navigation } from "@/components/navigation"
import { PageBreadcrumb } from "@/components/page-breadcrumb"
import { ComparisonPlayer } from "@/components/comparison-player"
import { Footer } from "@/components/footer"
import { Button } from "@/components/ui/button"
import { Terminal } from "lucide-react"
import Link from "next/link"

export const metadata = {
  title: "Demo — REFEREE",
  description: "Try REFEREE with Big Buck Bunny - see AI upscaling in action",
}

export default function DemoPage() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Navigation />
      <PageBreadcrumb page="Demo" />
      <main className="pt-8 pb-20">
        <div className="max-w-6xl mx-auto px-6">
          <div className="mb-12">
            <h1 className="text-3xl sm:text-4xl font-bold tracking-tight mb-4">
              Live Demo
            </h1>
            <p className="text-muted-foreground text-pretty">
              Experience <strong>REFEREE</strong> in action with Big Buck Bunny or Tears of Steel.
              This demo shows how web-based video players can leverage our companion application for hardware-accelerated upscaling.
            </p>
          </div>
          
          <ComparisonPlayer />

          <div className="mt-16 grid md:grid-cols-2 gap-8">
            <div className="relative overflow-hidden bg-card border border-border rounded-lg p-6">
              <div className="absolute inset-0 bg-black/70" />
              <div className="relative z-10">
                <h3 className="font-semibold mb-3">How This Demo Works</h3>
                <p className="text-sm text-muted-foreground leading-relaxed">
                  When <strong>REFEREE</strong> is running on your system, this video
                  player automatically connects to the local companion app. If REFEREE is
                  hosted elsewhere, you can also paste a remote URL into the player and use
                  that endpoint instead. Video frames are sent to the connected compatible GPU for
                  real-time AI upscaling, then displayed back in the browser.
                </p>
              </div>
            </div>
            <div className="relative overflow-hidden bg-card border border-border rounded-lg p-6">
              <div className="absolute inset-0 bg-black/70" />
              <div className="relative z-10">
                <h3 className="font-semibold mb-3">Requirements</h3>
                <ul className="text-sm text-muted-foreground space-y-2">
                  <li className="flex items-start gap-2">
                    <span className="text-accent mt-0.5">1.</span>
                    <span><strong>REFEREE</strong> running locally and configured</span>
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-accent mt-0.5">2.</span>
                    <span>Compatible NVIDIA RTX or AMD Radeon GPU (or REFEREE Relay configured)</span>
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-accent mt-0.5">3.</span>
                    <span>Chrome, Edge, or Firefox browser for best support</span>
                  </li>
                </ul>
              </div>
            </div>
          </div>

          <div className="mt-12 p-6 bg-secondary/50 border border-border rounded-lg">
            <h3 className="font-semibold mb-4">Build Your Own Integration</h3>
            <p className="text-sm text-muted-foreground mb-4 leading-relaxed">
              Looking to add <strong>REFEREE</strong> to your own video player?
              Visit the integration guide for setup steps, code examples, and the
              full API reference.
            </p>
            <Button variant="outline" asChild>
              <Link href="/developers">
                <Terminal className="h-4 w-4" />
                Open Integration Guide
              </Link>
            </Button>
          </div>

          <div className="mt-8 p-6 bg-card border border-border rounded-lg">
            <h3 className="font-semibold mb-4">Legal Notice</h3>
            <p className="text-sm text-muted-foreground leading-relaxed">
              <strong>REFEREE </strong> is provided &quot;as is&quot; without
              warranties. It is a neutral local utility and does not host or distribute media.
              Use is solely your responsibility, including compliance with provider Terms of
              Service and DRM rules. <strong>REFEREE</strong> is not affiliated with
              or endorsed by Advanced Micro Devices, Inc., NVIDIA Corporation, or streaming service providers.
            </p>
          </div>
        </div>
      </main>
      <Footer />
    </div>
  )
}
