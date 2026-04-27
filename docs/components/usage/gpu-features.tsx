import { RefereeWordmark } from "@/components/referee-wordmark"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Sparkles, AlertCircle } from "lucide-react"

export function GpuFeatures() {
  return (
    <section id="gpu-features" className="scroll-mt-28 mt-16 border-t border-border/60 pt-14">
      <div className="mb-5 flex items-center gap-3">
        <Sparkles className="h-5 w-5 flex-shrink-0 text-accent" />
        <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">Frame Generation &amp; TrueHDR</h2>
      </div>
      <p className="text-sm leading-7 text-muted-foreground sm:text-base">
        Two optional GPU-accelerated enhancements that go beyond upscaling — both toggled in the <strong>Settings</strong> panel.
      </p>

      <Alert className="mt-6 border-accent/20 bg-accent/5">
        <AlertCircle className="h-4 w-4 text-accent" />
        <AlertDescription className="text-sm text-muted-foreground">
          <p>Frame Generation and TrueHDR are <strong className="text-foreground">NVIDIA RTX only</strong>. These options are automatically greyed out on AMD GPUs and will not appear if your system doesn&apos;t support them.</p>
        </AlertDescription>
      </Alert>

      <div className="mt-8 space-y-6">
        {/* Frame Generation */}
        <div className="overflow-hidden rounded-xl border border-border/60">
          <div className="border-b border-border/60 bg-secondary/50 px-5 py-3">
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-sm font-semibold">Frame Generation</h3>
              <span className="shrink-0 rounded-full border border-accent/25 bg-accent/8 px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-accent">
                NVIDIA RTX only
              </span>
            </div>
          </div>
          <div className="space-y-4 px-5 py-4 text-sm leading-7 text-muted-foreground">
            <p>
              Frame generation uses AI to synthesise additional frames between the real ones, effectively
              doubling the frame rate of your stream. If your source is 30 fps, the output becomes ~60 fps;
              a 60 fps source becomes ~120 fps.
            </p>
            <p>
              The result is noticeably smoother motion — especially helpful for sports, action scenes, or
              any content with fast movement. It roughly doubles the GPU workload, so enable it only if
              your GPU can comfortably handle the chosen resolution and quality level first.
            </p>
            <p>
              <strong className="text-foreground">When to enable:</strong> high-end NVIDIA RTX GPU (RTX 30
              series or newer recommended), source content at 30 fps or less, smooth motion is a priority.
            </p>
          </div>
        </div>

        {/* TrueHDR */}
        <div className="overflow-hidden rounded-xl border border-border/60">
          <div className="border-b border-border/60 bg-secondary/50 px-5 py-3">
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-sm font-semibold">TrueHDR</h3>
              <span className="shrink-0 rounded-full border border-accent/25 bg-accent/8 px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-accent">
                NVIDIA RTX only
              </span>
            </div>
          </div>
          <div className="space-y-4 px-5 py-4 text-sm leading-7 text-muted-foreground">
            <p>
              TrueHDR uses an AI tone-mapping model to convert a standard dynamic range (SDR) stream into
              HDR output in real time. Bright highlights gain more headroom and shadows retain more detail,
              giving the picture more depth and vibrancy on an HDR display.
            </p>
            <p>
              <strong className="text-foreground">Requirements:</strong> an HDR-capable display connected and
              HDR enabled in your operating system display settings. Without those, enabling TrueHDR will
              not make a visible difference.
            </p>
            <p>
              <strong className="text-foreground">When to enable:</strong> you have an HDR TV or monitor,
              HDR is active in Windows HDR settings, and you want richer colours and contrast on your stream.
            </p>
          </div>
        </div>

        {/* AMD note */}
        <div className="rounded-xl border border-border/60 bg-secondary/40 p-4 text-sm leading-7 text-muted-foreground">
          <strong className="text-foreground">AMD GPU users —</strong> Frame Generation and TrueHDR rely on
          NVIDIA&apos;s proprietary SDKs and are not available on AMD hardware. AMD GPUs still support
          full AI upscaling via FidelityFX Super Resolution, so you still get a significant quality
          improvement — just without these two extra features. Check the{" "}
          <a href="#performance" className="text-accent hover:underline">
            Performance Reference
          </a>{" "}
          for AMD-specific benchmarks.
        </div>
      </div>
    </section>
  )
}
