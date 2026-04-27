import { Cpu, HardDrive, Info } from "lucide-react"
import { RefereeWordmark } from "@/components/referee-wordmark"

const requirements = [
  {
    icon: Cpu,
    title: "Graphics Card",
    items: [
      "NVIDIA GeForce RTX 20-series or newer",
      "AMD Radeon RX 6000-series or newer",
      "Frame generation availability depends on GPU backend and driver support",
    ],
  },
  {
    icon: HardDrive,
    title: "System",
    items: [
      "Windows 10/11 (64-bit) or modern Linux (x64)",
      "8GB+ system RAM",
      "Latest NVIDIA or AMD graphics drivers",
    ],
  },
]

export function Requirements() {
  return (
    <section id="requirements" className="py-24 px-6">
      <div className="max-w-4xl mx-auto">
        <div className="text-center mb-16">
          <h2 className="text-3xl font-bold tracking-tight mb-4">
            System Requirements
          </h2>
          <p className="text-muted-foreground max-w-2xl mx-auto">
            <RefereeWordmark variant="inline" /> requires a supported NVIDIA or AMD GPU for
            local hardware acceleration
          </p>
        </div>

        <div className="grid md:grid-cols-2 gap-6 mb-8">
          {requirements.map((req) => (
            <div
              key={req.title}
              className="p-6 rounded-lg border border-border bg-card"
            >
              <req.icon className="h-8 w-8 text-accent mb-4" />
              <h3 className="font-semibold mb-4">{req.title}</h3>
              <ul className="space-y-2">
                {req.items.map((item, index) => (
                  <li
                    key={index}
                    className="text-sm text-muted-foreground flex items-start gap-2"
                  >
                    <span className="w-1 h-1 rounded-full bg-accent mt-2 flex-shrink-0" />
                    {item}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>

        <div className="flex items-start gap-3 p-4 rounded-lg bg-accent/5 border border-accent/20">
          <Info className="h-5 w-5 text-accent flex-shrink-0 mt-0.5" />
          <p className="text-sm text-muted-foreground">
            REFEREE uses dedicated media acceleration paths from NVIDIA and AMD. Up-to-date drivers are required for full compatibility and performance. Frame generation and TrueHDR tone mapping features are currently only available on supported NVIDIA GPUs with compatible drivers.{" "}
            <a href="/roadmap#compatibility" className="underline text-accent hover:text-accent/80">View the Compatibility Chart</a>{" "}
            to see which features are available for each platform and GPU vendor.
          </p>
        </div>
      </div>
    </section>
  )
}
