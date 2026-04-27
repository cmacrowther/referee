import { Sparkles, Zap, Monitor, Lock, Layers, ServerOff, SlidersHorizontal, MonitorPlay } from "lucide-react"

const developerFeatures = [
  {
    icon: ServerOff,
    title: "Ship Smaller Streams",
    description: "Distribute low-bitrate, lower-resolution video and offload the upscaling to your users' own GPUs. Reduce bandwidth, CDN costs, storage, and server-side processing in one shot.",
  },
  {
    icon: SlidersHorizontal,
    title: "User-Controlled Quality",
    description: "Users choose their own upscaling level based on what their hardware can handle. They get the best possible picture — you don't have to serve it.",
  },
]

const clientFeatures = [
  {
    icon: Sparkles,
    title: "Super Resolution",
    description: "Leverage dedicated NVIDIA RTX and/or AMD RX hardware to upscale lower resolution video content in real-time with AI-powered enhancement.",
  },
  {
    icon: Layers,
    title: "Frame Generation",
    description: "Generate intermediate frames using RTX or FSR technology for smoother playback, ideal for cinematic content or lower frame-rate sources.",
  },
  {
    icon: Monitor,
    title: "TrueHDR Enhancement",
    description: "Convert SDR content to HDR using AI-powered tone mapping that adds genuine dynamic range to standard video streams.",
  },
  {
    icon: Zap,
    title: "Low Latency, Lightweight Service",
    description: "Runs quietly in your system tray consuming minimal resources. A local hardware-encoder pipeline keeps added buffering low between source ingest and enhanced playback.",
  },
  {
    icon: Lock,
    title: "Secure Local Processing",
    description: "All video processing happens locally on your hardware. No frames leave your machine, and CORS-protected streams remain secure.",
  },
  {
    icon: MonitorPlay,
    title: "Open in Any Media Player",
    description: "Stream enhanced video directly into your favourite media player. VLC, MPV, PotPlayer, and any other player that supports HLS can tune into the locally upscaled output.",
  },
]

export function Features() {
  return (
    <section id="features" className="py-24 px-6 border-t border-border">
      <div className="max-w-6xl mx-auto">
        <div className="text-center mb-16">
          <h2 className="text-3xl font-bold tracking-tight mb-4">
            The Best{" "}
            <span className="relative inline-flex px-1 text-accent">
              <span className="relative z-10">Call</span>
              <span
                aria-hidden="true"
                className="absolute inset-x-0 bottom-1 h-3 rounded-full bg-accent/15"
              />
              <span
                aria-hidden="true"
                className="absolute -bottom-1 left-0 h-[3px] w-full rounded-full bg-gradient-to-r from-accent/30 via-accent to-accent/60"
              />
            </span>{" "}
            You’ll Make for Your Content
          </h2>
          <p className="text-muted-foreground max-w-2xl mx-auto">
            Keep your streaming costs low. Give your users control over their own playback quality.
          </p>
        </div>

        <div className="mb-12">
          <div className="flex items-center gap-3 mb-6">
            <span className="text-xs font-semibold uppercase tracking-widest text-accent">For Developers</span>
            <div className="flex-1 h-px bg-accent/20" />
          </div>
          <div className="grid sm:grid-cols-2 gap-6">
            {developerFeatures.map((feature) => (
              <div
                key={feature.title}
                className="p-6 rounded-lg border border-accent/20 bg-accent/5 hover:border-accent/40 transition-colors"
              >
                <feature.icon className="h-8 w-8 text-accent mb-4" />
                <h3 className="font-semibold mb-2">{feature.title}</h3>
                <p className="text-sm text-muted-foreground leading-relaxed">
                  {feature.description}
                </p>
              </div>
            ))}
          </div>
        </div>

        <div>
          <div className="flex items-center gap-3 mb-6">
            <span className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">For Users</span>
            <div className="flex-1 h-px bg-border" />
          </div>
          <div className="flex flex-wrap justify-center gap-6">
            {clientFeatures.map((feature) => (
              <div
                key={feature.title}
                className="w-full sm:w-[calc(50%-12px)] lg:w-[calc(33.333%-16px)] p-6 rounded-lg border border-border bg-card hover:border-accent/50 transition-colors"
              >
                <feature.icon className="h-8 w-8 text-accent mb-4" />
                <h3 className="font-semibold mb-2">{feature.title}</h3>
                <p className="text-sm text-muted-foreground leading-relaxed">
                  {feature.description}
                </p>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}
