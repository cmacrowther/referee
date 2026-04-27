import { Navigation } from "@/components/navigation"
import { PageBreadcrumb } from "@/components/page-breadcrumb"
import { Footer } from "@/components/footer"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import {
  AlertCircle,
  ChevronDown,
  CheckCircle2,
  Circle,
  Clock,
  Loader2,
  Minus,
  Sparkles,
  Cpu,
  Container,
  MonitorPlay,
  Layers,
  Zap,
  Globe,
  Package,
  Sliders,
  ShieldCheck,
} from "lucide-react"
import { siAmd, siNvidia, siLinux, siApple, siIntel } from "simple-icons"

export const metadata = {
  title: "Roadmap — REFEREE",
  description: "See what's being built, what's coming soon, and what's already shipped in REFEREE.",
}

export const revalidate = 3600

async function fetchLatestVersion(): Promise<string | null> {
  try {
    const res = await fetch(
      "https://api.github.com/repos/cmacrowther/referee/releases/latest",
      {
        headers: { Accept: "application/vnd.github+json" },
        next: { revalidate: 3600 },
      }
    )
    if (!res.ok) return null
    const data = await res.json()
    return data.tag_name ?? null
  } catch {
    return null
  }
}

type RoadmapStatus = "in-progress" | "planned" | "shipped"

type RoadmapItem = {
  icon: React.ReactNode
  title: string
  description: string
  status: RoadmapStatus
}

const roadmapItems: RoadmapItem[] = [
  // In Progress
  {
    icon: <Cpu className="h-5 w-5" />,
    title: "AMD AMF Support on Windows",
    description:
      "Full hardware-accelerated upscaling and frame generation on AMD RX 7000 and RX 8000 series GPUs via AMD AMF integration.",
    status: "in-progress",
  },
  {
    icon: <Container className="h-5 w-5" />,
    title: "Docker Container Support",
    description:
      "Expanded Docker profile support to allow running REFEREE in a containerized environment, with separate NVIDIA and AMD configurations.",
    status: "in-progress",
  },
  // Planned
  {
    icon: <AppleSimpleIcon className="h-5 w-5" />,
    title: "macOS Support",
    description:
      "Native macOS application with Metal-accelerated upscaling and Apple Silicon support, bringing REFEREE to Mac.",
    status: "planned",
  },
  {
    icon: <IntelIcon className="h-5 w-5" />,
    title: "Intel Arc GPU Support",
    description:
      "Hardware-accelerated upscaling and processing on Intel Arc discrete GPUs via the XeSS and oneVPL APIs.",
    status: "planned",
  },
]

const inProgress = roadmapItems.filter((i) => i.status === "in-progress")
const planned = roadmapItems.filter((i) => i.status === "planned")

const statusConfig: Record<
  RoadmapStatus,
  { label: string; badgeClass: string; icon: React.ReactNode }
> = {
  "in-progress": {
    label: "In Progress",
    badgeClass:
      "bg-accent/15 text-accent border-accent/25",
    icon: <Loader2 className="h-3.5 w-3.5 animate-spin" />,
  },
  planned: {
    label: "Planned",
    badgeClass:
      "bg-muted text-muted-foreground border-border",
    icon: <Circle className="h-3.5 w-3.5" />,
  },
  shipped: {
    label: "Shipped",
    badgeClass:
      "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
    icon: <CheckCircle2 className="h-3.5 w-3.5" />,
  },
}

// ─── Compatibility table ─────────────────────────────────────────────────────

type CompatSupport = "yes" | "soon" | "no"
type CompatImpl = "ai" | "hardware" | "software"

type CompatFeature = {
  support: CompatSupport
  impl?: CompatImpl | CompatImpl[]
  tech?: string
}

type CompatGpuRow = {
  gpu: string
  gpuIcon: React.ReactNode
  upscaling: CompatFeature
  frameGen: CompatFeature
  trueHDR: CompatFeature
  relay: CompatFeature
}

type CompatPlatformGroup = {
  platform: string
  platformIcon: React.ReactNode
  subtext?: { label: string; description: string }[]
  gpus: CompatGpuRow[]
}

function WindowsIcon({ className }: { className?: string }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d="M3 4.64 10.5 3.5v8.06H3V4.64ZM12 3.29 21 2v9.56h-9V3.29ZM3 12.94h7.5V21.06L3 20v-7.06ZM12 12.94h9V22l-9-1.29v-7.77Z" />
    </svg>
  )
}

function AppleSimpleIcon({ className }: { className?: string }) {
  return (
    <svg role="img" aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d={siApple.path} />
    </svg>
  )
}

function IntelIcon({ className }: { className?: string }) {
  return (
    <svg role="img" aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d={siIntel.path} />
    </svg>
  )
}

function LinuxIcon({ className }: { className?: string }) {
  return (
    <svg role="img" aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d={siLinux.path} />
    </svg>
  )
}

function DockerIcon({ className }: { className?: string }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d="M13.983 11.078h2.119a.186.186 0 0 0 .186-.185V9.006a.186.186 0 0 0-.186-.186h-2.119a.185.185 0 0 0-.185.185v1.888c0 .102.083.185.185.185m-2.954-5.43h2.118a.186.186 0 0 0 .186-.186V3.574a.186.186 0 0 0-.186-.185h-2.118a.185.185 0 0 0-.185.185v1.888c0 .102.082.185.185.185m0 2.716h2.118a.187.187 0 0 0 .186-.186V6.29a.186.186 0 0 0-.186-.185h-2.118a.185.185 0 0 0-.185.185v1.887c0 .102.082.185.185.186m-2.93 0h2.12a.186.186 0 0 0 .184-.186V6.29a.185.185 0 0 0-.185-.185H8.1a.185.185 0 0 0-.185.185v1.887c0 .102.083.185.185.186m-2.964 0h2.119a.186.186 0 0 0 .185-.186V6.29a.185.185 0 0 0-.185-.185H5.136a.186.186 0 0 0-.186.185v1.887c0 .102.084.185.186.186m5.893 2.715h2.118a.186.186 0 0 0 .186-.185V9.006a.186.186 0 0 0-.186-.186h-2.118a.185.185 0 0 0-.185.185v1.888c0 .102.082.185.185.185m-2.93 0h2.12a.185.185 0 0 0 .184-.185V9.006a.185.185 0 0 0-.184-.186h-2.12a.185.185 0 0 0-.184.185v1.888c0 .102.083.185.185.185m-2.964 0h2.119a.185.185 0 0 0 .185-.185V9.006a.185.185 0 0 0-.184-.186h-2.12a.186.186 0 0 0-.186.186v1.887c0 .102.084.185.186.185m-2.92 0h2.12a.185.185 0 0 0 .184-.185V9.006a.185.185 0 0 0-.184-.186h-2.12a.185.185 0 0 0-.185.185v1.888c0 .102.083.185.185.185M23.763 9.89c-.065-.051-.672-.51-1.954-.51-.338.001-.676.03-1.01.087-.248-1.7-1.653-2.53-1.716-2.566l-.344-.199-.226.327c-.284.438-.49.922-.612 1.43-.23.97-.09 1.882.403 2.661-.595.332-1.55.413-1.744.42H.751a.751.751 0 0 0-.75.748 11.376 11.376 0 0 0 .692 4.062c.545 1.428 1.355 2.48 2.41 3.124 1.18.723 3.1 1.137 5.275 1.137.983.003 1.963-.086 2.93-.266a12.248 12.248 0 0 0 3.823-1.389c.98-.567 1.86-1.288 2.61-2.136 1.252-1.418 1.998-2.997 2.553-4.4h.221c1.372 0 2.215-.549 2.68-1.009.309-.293.55-.65.707-1.046l.098-.288Z" />
    </svg>
  )
}

function NvidiaIcon({ className }: { className?: string }) {
  return (
    <svg role="img" aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d={siNvidia.path} />
    </svg>
  )
}

function AmdIcon({ className }: { className?: string }) {
  return (
    <svg role="img" aria-hidden="true" viewBox="0 0 24 24" className={className} fill="currentColor">
      <path d={siAmd.path} />
    </svg>
  )
}

const NvidiaLabel = () => (
  <span className="inline-flex w-20 h-8 items-center justify-center rounded-md border border-[#76b900]/20 bg-[#76b900]/8">
    <NvidiaIcon className="h-4 w-4 text-[#76b900]/70" />
  </span>
)
const AmdLabel = () => (
  <span className="inline-flex w-20 h-8 items-center justify-center rounded-md border border-[#ed1c24]/20 bg-[#ed1c24]/8">
    <AmdIcon className="h-10 w-auto text-[#ed1c24]/70" />
  </span>
)

const compatGroups: CompatPlatformGroup[] = [
  {
    platform: "Windows",
    platformIcon: <WindowsIcon className="h-4 w-4" />,
    subtext: [
      { label: ".exe", description: "Windows 10 / 11" },
    ],
    gpus: [
      {
        gpu: "NVIDIA", gpuIcon: <NvidiaLabel />,
        upscaling: { support: "yes" },
        frameGen:  { support: "yes" },
        trueHDR:   { support: "yes" },
        relay:     { support: "yes" },
      },
      {
        gpu: "AMD",    gpuIcon: <AmdLabel />,
        upscaling: { support: "yes"  },
        frameGen:  { support: "soon" },
        trueHDR:   { support: "yes"  },
        relay:     { support: "yes"  },
      },
    ],
  },
  {
    platform: "Linux",
    platformIcon: <LinuxIcon className="h-4 w-4" />,
    subtext: [
      { label: ".deb", description: "Ubuntu 22.04+ · Debian 12+" },
      { label: ".rpm", description: "Fedora 38+ · RHEL / CentOS Stream 9+" },
    ],
    gpus: [
      {
        gpu: "NVIDIA", gpuIcon: <NvidiaLabel />,
        upscaling: { support: "yes" },
        frameGen:  { support: "soon" },
        trueHDR:   { support: "yes" },
        relay:     { support: "yes" },
      },
      {
        gpu: "AMD",    gpuIcon: <AmdLabel />,
        upscaling: { support: "yes" },
        frameGen:  { support: "soon" },
        trueHDR:   { support: "yes" },
        relay:     { support: "yes" },
      },
    ],
  },
  {
    platform: "Docker",
    platformIcon: <DockerIcon className="h-4 w-4" />,
    subtext: [
      { label: "nvidia", description: "NVIDIA Container Toolkit required" },
      { label: "amd",    description: "AMD Mesa or ROCm required" },
    ],
    gpus: [
      {
        gpu: "NVIDIA", gpuIcon: <NvidiaLabel />,
        upscaling: { support: "yes" },
        frameGen:  { support: "soon" },
        trueHDR:   { support: "yes" },
        relay:     { support: "soon" },
      },
      {
        gpu: "AMD",    gpuIcon: <AmdLabel />,
        upscaling: { support: "yes" },
        frameGen:  { support: "soon" },
        trueHDR:   { support: "yes" },
        relay:     { support: "soon" },
      },
    ],
  },
]

const windowsCompatGroups = compatGroups.filter((group) => group.platform === "Windows")
const earlyDevelopmentCompatGroups = compatGroups.filter((group) => group.platform !== "Windows")

function ImplBadge({ impl }: { impl: CompatImpl }) {
  if (impl === "ai") {
    return (
      <span className="inline-flex items-center gap-0.5 rounded-full border border-accent/30 bg-accent/10 px-1.5 py-px text-[10px] font-semibold text-accent">
        <Sparkles className="h-2.5 w-2.5" />
        AI
      </span>
    )
  }
  if (impl === "hardware") {
    return (
      <span className="inline-flex items-center rounded border border-border bg-muted/60 px-1.5 py-px text-[10px] font-medium text-muted-foreground">
        HW
      </span>
    )
  }
  return (
    <span className="inline-flex items-center rounded border border-border bg-muted/60 px-1.5 py-px text-[10px] font-medium text-muted-foreground">
      SW
    </span>
  )
}

function SupportCell({ feature }: { feature: CompatFeature }) {
  const { support, impl, tech } = feature
  const impls = impl ? (Array.isArray(impl) ? impl : [impl]) : []

  if (support === "yes" && tech) {
    const isAi = impls.includes("ai")
    return (
      <span className="inline-flex flex-col items-center gap-1 py-0.5">
        <CheckCircle2 className="h-4 w-4 text-emerald-400" aria-label="Supported" />
        <span className="inline-flex items-center gap-1 text-[10px] text-muted-foreground/55">
          {isAi && <Sparkles className="h-2.5 w-2.5 text-accent/70" aria-label="AI-accelerated" />}
          <span>{tech}</span>
        </span>
      </span>
    )
  }

  if (support === "yes") {
    return (
      <span className="inline-flex flex-col items-center gap-1">
        <CheckCircle2 className="h-4 w-4 text-emerald-400" aria-label="Supported" />
        {impls.map((i) => <ImplBadge key={i} impl={i} />)}
      </span>
    )
  }
  if (support === "soon") {
    const isAi = impls.includes("ai")
    if (tech) {
      return (
        <span className="inline-flex flex-col items-center gap-1 py-0.5">
          <Clock className="h-4 w-4 text-amber-400" aria-label="Coming soon" />
          <span className="inline-flex items-center gap-1 text-[10px] text-muted-foreground/55">
            {isAi && <Sparkles className="h-2.5 w-2.5 text-accent/70" aria-label="AI-accelerated" />}
            <span>{tech}</span>
          </span>
        </span>
      )
    }
    return (
      <span className="inline-flex flex-col items-center gap-1">
        <Clock className="h-4 w-4 text-amber-400" aria-label="Coming soon" />
        {impls.map((i) => <ImplBadge key={i} impl={i} />)}
      </span>
    )
  }
  return (
    <span className="inline-flex items-center justify-center">
      <Minus className="h-4 w-4 text-muted-foreground/40" aria-label="Not currently supported" />
    </span>
  )
}

function PlatformCompatibilityTable({ groups }: { groups: CompatPlatformGroup[] }) {
  return (
    <div className="overflow-x-auto rounded-lg border border-border">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border bg-muted/40">
            <th className="px-4 py-3 text-left font-medium text-muted-foreground whitespace-nowrap" scope="col">
              Platform
              <div className="text-[11px] font-normal text-muted-foreground/60 mt-0.5">OS / Environment</div>
            </th>
            <th className="px-4 py-3 text-left font-medium text-muted-foreground whitespace-nowrap" scope="col">
              GPU
              <div className="text-[11px] font-normal text-muted-foreground/60 mt-0.5">GPU hardware vendor</div>
            </th>
            <th className="px-4 py-3 text-center font-medium text-muted-foreground whitespace-nowrap" scope="col">
              AI Upscaling
              <div className="text-[11px] font-normal text-muted-foreground/60 mt-0.5">Resolution enhancement</div>
            </th>
            <th className="px-4 py-3 text-center font-medium text-muted-foreground whitespace-nowrap" scope="col">
              Motion Boost
              <div className="text-[11px] font-normal text-muted-foreground/60 mt-0.5">Frame rate interpolation</div>
            </th>
            <th className="px-4 py-3 text-center font-medium text-muted-foreground whitespace-nowrap" scope="col">
              HDR Mapping
              <div className="text-[11px] font-normal text-muted-foreground/60 mt-0.5">SDR → HDR tone mapping</div>
            </th>
            <th className="px-4 py-3 text-center font-medium text-muted-foreground whitespace-nowrap" scope="col">
              REFEREE Relay
              <div className="text-[11px] font-normal text-muted-foreground/60 mt-0.5">Relay to a peer device</div>
            </th>
          </tr>
        </thead>
        <tbody>
          {groups.map((group, gi) =>
            group.gpus.map((gpu, ri) => (
              <tr
                key={`${group.platform}-${gpu.gpu}`}
                className={[
                  "border-b border-border last:border-0",
                  ri === group.gpus.length - 1 && gi < groups.length - 1
                    ? "border-b-2"
                    : "",
                  ri === 1 ? "bg-muted/20" : "",
                ].join(" ")}
              >
                {ri === 0 && (
                  <td
                    rowSpan={group.gpus.length}
                    className="px-4 py-3 font-medium align-middle border-r border-border"
                  >
                    <span className="inline-flex items-center gap-2 mb-1">
                      <span className="text-muted-foreground">{group.platformIcon}</span>
                      {group.platform}
                    </span>
                    {group.subtext && (
                      <div className="mt-1.5 space-y-0.5">
                        {group.subtext.map((s) => (
                          <div key={s.label} className="flex items-baseline gap-1.5 flex-wrap">
                            <span className="font-mono text-[10px] text-accent">{s.label}</span>
                            <span className="text-[10px] text-muted-foreground font-normal">{s.description}</span>
                          </div>
                        ))}
                      </div>
                    )}
                  </td>
                )}
                <td className="px-4 py-3 text-muted-foreground">{gpu.gpuIcon}</td>
                <td className="px-4 py-3 text-center"><SupportCell feature={gpu.upscaling} /></td>
                <td className="px-4 py-3 text-center"><SupportCell feature={gpu.frameGen} /></td>
                <td className="px-4 py-3 text-center"><SupportCell feature={gpu.trueHDR} /></td>
                <td className="px-4 py-3 text-center"><SupportCell feature={gpu.relay} /></td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  )
}

function EarlyDevelopmentPlatformSection({ group }: { group: CompatPlatformGroup }) {
  return (
    <details className="group rounded-lg border border-border bg-secondary/25">
      <summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-4 py-3 [&::-webkit-details-marker]:hidden">
        <span className="inline-flex items-center gap-2 font-medium">
          <span className="text-muted-foreground">{group.platformIcon}</span>
          {group.platform}
          <Badge variant="outline" className="border-amber-500/25 bg-amber-500/10 text-amber-500">
            Early development
          </Badge>
        </span>
        <ChevronDown className="h-4 w-4 text-muted-foreground transition-transform group-open:rotate-180" />
      </summary>
      <div className="space-y-4 border-t border-border px-4 py-4">
        <Alert className="border-amber-500/25 bg-amber-500/10">
          <AlertCircle className="h-4 w-4 text-amber-500" />
          <AlertTitle>{group.platform} support is still early</AlertTitle>
          <AlertDescription>
            This platform may run into more issues than its Windows counterpart. These builds are currently best suited for
            REFEREE Relay workflows where this device connects to, or is paired with, a Windows-based REFEREE peer for the
            most complete processing support.
          </AlertDescription>
        </Alert>
        <PlatformCompatibilityTable groups={[group]} />
      </div>
    </details>
  )
}

function CompatibilityTable({ version }: { version: string | null }) {
  return (
    <section id="compatibility" className="mb-14">
      <h2 className="text-xl font-semibold mb-2">Platform Compatibility</h2>
      <p className="text-sm text-muted-foreground mb-5">
        Here&apos;s a summary of what&apos;s currently supported as of REFEREE version{" "}
        <span className="font-medium text-foreground">{version ?? "latest"}</span>.
        Features not currently supported are currently in development or planned for a future release as this is the highest priority.
      </p>
      <div className="rounded-xl border border-accent/25 bg-accent/5 p-3 sm:p-4">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3 className="font-semibold">Windows</h3>
            <p className="mt-1 text-xs text-muted-foreground">
              Primary desktop target with the most complete REFEREE feature support.
            </p>
          </div>
          <Badge className="border-accent/25 bg-accent/15 text-accent" variant="outline">
            Recommended
          </Badge>
        </div>
        <PlatformCompatibilityTable groups={windowsCompatGroups} />
      </div>

      <div className="mt-4 space-y-3">
        {earlyDevelopmentCompatGroups.map((group) => (
          <EarlyDevelopmentPlatformSection key={group.platform} group={group} />
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-5 mt-3 px-1 text-xs text-muted-foreground">
        <span className="inline-flex items-center gap-1.5">
          <CheckCircle2 className="h-3.5 w-3.5 text-emerald-400" /> Supported
        </span>
        <span className="inline-flex items-center gap-1.5">
          <Clock className="h-3.5 w-3.5 text-amber-400" /> Coming soon
        </span>
        <span className="inline-flex items-center gap-1.5">
          <Minus className="h-3.5 w-3.5 text-muted-foreground/40" /> Not supported
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="inline-flex items-center gap-0.5 rounded-full border border-accent/30 bg-accent/10 px-1.5 py-px text-[10px] font-semibold text-accent"><Sparkles className="h-2.5 w-2.5" />AI</span> AI-accelerated
        </span>
      </div>
    </section>
  )
}

// ─── Roadmap cards ────────────────────────────────────────────────────────────

function ItemCard({ item }: { item: RoadmapItem }) {
  const cfg = statusConfig[item.status]
  return (
    <div className="relative overflow-hidden rounded-lg border border-border bg-card p-5 flex gap-4">
      <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-accent/10 text-accent">
        {item.icon}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex flex-wrap items-center gap-2 mb-1.5">
          <h3 className="font-semibold text-sm leading-snug">{item.title}</h3>
          <span
            className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium ${cfg.badgeClass}`}
          >
            {cfg.icon}
            {cfg.label}
          </span>
        </div>
        <p className="text-sm text-muted-foreground leading-relaxed">
          {item.description}
        </p>
      </div>
    </div>
  )
}

function Section({
  title,
  description,
  items,
  accentLine,
}: {
  title: string
  description: string
  items: RoadmapItem[]
  accentLine?: boolean
}) {
  return (
    <section className="mb-12">
      <div className="flex items-center gap-3 mb-2">
        {accentLine && (
          <span className="inline-block h-4 w-1 rounded-full bg-accent flex-shrink-0" />
        )}
        <h2 className="text-xl font-semibold">{title}</h2>
      </div>
      <p className="text-sm text-muted-foreground mb-5 ml-[calc(0.25rem+12px)]" style={accentLine ? {} : { marginLeft: 0 }}>
        {description}
      </p>
      <div className="grid gap-3">
        {items.map((item) => (
          <ItemCard key={item.title} item={item} />
        ))}
      </div>
    </section>
  )
}

export default async function RoadmapPage() {
  const version = await fetchLatestVersion()
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Navigation />
      <PageBreadcrumb page="Roadmap" />
      <main className="pt-8 pb-20">
        <div className="max-w-6xl mx-auto px-6">
          <div className="mb-12">
            <h1 className="text-3xl sm:text-4xl font-bold tracking-tight mb-4">
              Roadmap
            </h1>
            <p className="text-muted-foreground text-pretty">
              What&apos;s being actively built, what&apos;s coming next, and what has already shipped.
              Timelines are estimates and may shift as development progresses.
            </p>
          </div>

          <CompatibilityTable version={version} />

          <Section
            title="In Progress"
            description="Features currently under active development."
            items={inProgress}
            accentLine
          />

          <Section
            title="Planned for the Future"
            description="Features on the roadmap that are not yet in active development."
            items={planned}
          />

          <div className="p-6 bg-secondary/50 border border-border rounded-lg">
            <h3 className="font-semibold mb-3">Have a feature request?</h3>
            <p className="text-sm text-muted-foreground leading-relaxed">
              Open an issue or start a discussion on GitHub. Community feedback
              directly shapes the priority of items on this roadmap.
            </p>
            <a
              href="https://github.com/cmacrowther/referee/issues"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-2 mt-4 text-sm font-medium text-accent hover:underline"
            >
              Open an issue on GitHub
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
                <path
                  d="M2.5 2.5h7m0 0v7m0-7L2.5 9.5"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </a>
          </div>

          <section className="mt-14">
            <h2 className="text-xl font-semibold mb-2">Changelog</h2>
            <p className="text-sm text-muted-foreground mb-8">
              A summary of what shipped in each release.
            </p>

            <div className="relative">
              {/* vertical timeline rule */}
              <div className="absolute left-[7px] top-2 bottom-2 w-px bg-border" aria-hidden="true" />

              <ol className="space-y-10">

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-accent bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.6.0</h3>
                    <span className="text-xs text-muted-foreground">April 27, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Server module split — auth, relay, routing, status, stream handling, state, and response helpers are now organized into focused modules with expanded coverage</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>API token initialization now uses the resolved app data directory more consistently for desktop and headless server startup</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Relay readiness checks now verify the linked peer before routing streams and report clearer offline or not-ready states</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Remote Relay settings lock automatically while the linked peer is offline, with inline status and error messages in the settings UI</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Motion Boost can now be toggled when controlling a NVIDIA-backed relay peer</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Status and settings refresh paths were tuned to reduce redundant UI work while keeping relay state current</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Developer docs now cover live stream settings updates, HLS proxy segment extension guidance, and improved local demo setup notes</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Project licensing clarified as GPL-3.0-or-later with updated README guidance</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.5.0</h3>
                    <span className="text-xs text-muted-foreground">April 25, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Relay system — link REFEREE instances on the local network and route streams through a linked peer with persistent credential storage</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Remote-backed sessions — the local player proxies HLS output from the linked relay peer with seamless handoff</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Real-time relay link status — connection availability and peer reachability are monitored and surfaced live in the UI</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Relay stream control — stream start/stop commands and heartbeat are forwarded to the relay peer; stream settings are locked when controlled remotely</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Auto-relay mode — relay routing is automatically activated when the local GPU does not meet processing requirements</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Remote stream settings — stream settings can be configured on the relay peer directly from the controlling instance</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Relay UI — trusted peer panel, per-peer status badges, and inline relay status indicators in the settings card</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span><code className="text-[11px]">/v1/ping</code> API endpoint for liveness checks and LAN peer discovery</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Audio timing stability improvements merged across all playback paths</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Stream settings update refactored for cleaner error reporting</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.4.3</h3>
                    <span className="text-xs text-muted-foreground">April 24, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Native C++/Rust RIFE worker — frame interpolation rewritten as a native worker, replacing the Python-based implementation for lower latency and no runtime dependency</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>FidelityFX CAS sharpening — Contrast Adaptive Sharpening integrated across all executor paths with a custom GLSL shader variant</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>AMD AMF hardware-accelerated decoding — universal executor now uses AMF for hardware decode on AMD GPUs</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Hardware-accelerated denoising — hqdn3d denoising added to native and universal source-pull paths; switched to GPL FFmpeg builds to enable filter</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>NVIDIA FRUC improvements — constant frame rate enforcement, dynamic target FPS, and corrected transcode pacing</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Built-in HLS stream player — embedded player window launches automatically when a stream starts</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>@cmacrowther/referee-client package — new npm package providing a typed API client and React integration hooks</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Rate limiting and SSRF protection added to the server</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>LAN peer discovery — initial foundation for discovering REFEREE instances on the local network</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>HLS manifest rewriting now selects the highest bandwidth video variant</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed audio/video sync and stabilized NVIDIA encoding path under live HLS conditions</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed Docker volume ownership for correct container runtime permissions</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.4.1</h3>
                    <span className="text-xs text-muted-foreground">April 19, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>AMD executor — full AMD pipeline on Windows with VCEEncC command building, hardware upscale support, and capability detection</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>RIFE frame interpolation — streaming RIFE support via embedded Python runtime and dedicated RIFE wrapper</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Portable interpolation fallback — unsupported interpolation cases are routed to a software fallback with gap handling</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Non-HLS source support — pipeline staged launch and preprocess steps now handle non-HLS inputs</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Binary manager overhaul — unified binary resolution for RIFE, FFmpeg, and shader management</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Intermediate execution plan — new intermediate plan structure for cleaner pipeline orchestration</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>libplacebo preprocess filter plan — dedicated preprocess stage for libplacebo-accelerated processing</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>HDR preprocess ownership flag — prevents double-processing of HDR operations in the Universal executor</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Engine settings toggle in debug panel — new section with per-engine controls and enhanced debug UI</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>AI badges in settings — upscaling feature cards now show AI and hardware acceleration indicators</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed VCEEncC AMF encoder retry logic on Linux</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed RIFE Vulkan library detection in FFmpeg preprocessor</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.3.1</h3>
                    <span className="text-xs text-muted-foreground">April 18, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Source content classification — automatic detection of animated vs. live-action content to select the optimal upscaler</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>ArtCNN shader support — ArtCNN C4F16 GLSL shader added as an additional upscaling option</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Universal shader manager — shader installation refactored to support multiple shader types beyond Anime4K</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Stream info badges now display active content kind and selected upscaler</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.3.0</h3>
                    <span className="text-xs text-muted-foreground">April 12, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Pipeline rearchitecture — staged execution model with discrete preprocess, normalize, encode, and HLS package stages for improved reliability and extensibility</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Universal executor — new cross-platform backend replaces the previous platform-specific executors, with a dedicated NVIDIA-specialized executor for Windows-exclusive features</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Anime4K GLSL shader upscaling — 2x upscale path via Anime4K shaders for anime content</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>NVIDIA AI processing option — explicit GPU utilization preference exposed in settings</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>FFmpeg binary management — bundled FFmpeg support with ffprobe integration for improved cross-platform compatibility</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>HLS source relay — new source module proxies HLS inputs directly into the pipeline without re-muxing</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>HLS packager — dedicated FfmpegHlsPackager stage; segment list size increased to 8; HEVC codec tag added for Apple device compatibility</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Source/upscaled video playback synchronisation — source and upscaled streams are kept in sync during live viewing</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Settings UI redesigned — tabbed navigation layout with CapabilityChip indicators and a ProcessingPathSummary for at-a-glance pipeline status</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Improved stderr handling — StderrMode propagated through pipeline stages for structured error capture and logging</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Pipeline session cleanup — completion signalling ensures sessions are fully torn down before a new pipeline starts</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed Vulkan ICD library path in NVIDIA Docker image</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.2.1</h3>
                    <span className="text-xs text-muted-foreground">April 11, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Improved playlist session management — existing sessions are now cleanly stopped before a new stream is started</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed Cargo default-run entry to correctly target the server binary</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.2.0</h3>
                    <span className="text-xs text-muted-foreground">April 5, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Docker support — separate NVIDIA and AMD profiles with dedicated Dockerfiles</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Headless server mode — run REFEREE without a desktop UI for self-hosted and container deployments</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>NVIDIA GPU detection for headless mode; encoder binary handling refactored</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.1.8</h3>
                    <span className="text-xs text-muted-foreground">April 4, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Fixed Windows and Linux artifact targets in the release workflow</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Tauri build targets now explicitly specified for correct package output</li>
                  </ul>
                </li>

                <li className="relative pl-8">
                  <span className="absolute left-0 top-1.5 flex h-[15px] w-[15px] items-center justify-center rounded-full border-2 border-border bg-background" aria-hidden="true" />
                  <div className="flex flex-wrap items-baseline gap-3 mb-3">
                    <h3 className="font-semibold">v0.1.7</h3>
                    <span className="text-xs text-muted-foreground">April 4, 2026</span>
                  </div>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>External player detection and launch — configure and open streams directly in a local media player from the UI</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Stream source name now displayed in the stream info badge</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Lookahead argument added for both NVENC and VCE encoder backends</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Removed Windows-specific pipeline configuration that could cause cross-platform issues</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>Frame generation and HDR toggles fixed; stream card now shows Open button once a stream is active</li>
                    <li className="flex gap-2"><span className="text-accent flex-shrink-0">+</span>License updated to GNU GPL v3</li>
                  </ul>
                </li>

              </ol>
            </div>
          </section>
        </div>
      </main>
      <Footer />
    </div>
  )
}
