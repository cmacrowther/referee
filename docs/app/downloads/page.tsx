import { Navigation } from "@/components/navigation"
import { PageBreadcrumb } from "@/components/page-breadcrumb"
import { Footer } from "@/components/footer"
import { HeadlessServerSetup } from "@/components/headless-server"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  REFEREE_RELEASES_URL,
  fetchRefereeReleases,
  findNewestReleaseAsset,
  hasExtension,
  type ReleaseAsset,
} from "@/lib/github-releases"
import {
  ChevronDown,
  Download,
  ExternalLink,
  Monitor,
  Terminal,
  CheckCircle,
  AlertCircle,
} from "lucide-react"
import { siDebian, siFedora, siLinux } from "simple-icons"
import Link from "next/link"

export const revalidate = 3600

export const metadata = {
  title: "Downloads — REFEREE",
  description: "Download REFEREE for Windows or Linux and get up and running in minutes.",
}

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
  })
}

function isPresent<T>(value: T | null | undefined): value is T {
  return value != null
}

function WindowsIcon({ className }: { className?: string }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className={className}>
      <path
        fill="currentColor"
        d="M3 4.64 10.5 3.5v8.06H3V4.64ZM12 3.29 21 2v9.56h-9V3.29ZM3 12.94h7.5V21.06L3 20v-7.06ZM12 12.94h9V22l-9-1.29v-7.77Z"
      />
    </svg>
  )
}

type InstallStep = { title: string; description: string }
type PackageDistros = { label: string; distros: string[] }

type PlatformCardProps = {
  name: string
  icon: React.ReactNode
  primaryAsset: ReleaseAsset | undefined
  secondaryAsset?: ReleaseAsset | undefined
  primaryLabel: string
  secondaryLabel?: string
  primaryIcon?: React.ReactNode
  secondaryIcon?: React.ReactNode
  releasesUrl: string
  version?: string
  steps: InstallStep[]
  primaryStepsLabel?: string
  secondarySteps?: InstallStep[]
  secondaryStepsLabel?: string
  packageDistros?: PackageDistros[]
}

function StepList({ steps }: { steps: InstallStep[] }) {
  return (
    <ol className="space-y-4">
      {steps.map((step, i) => (
        <li key={i} className="flex gap-3">
          <span className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent text-xs font-bold">
            {i + 1}
          </span>
          <div>
            <p className="text-sm font-medium">{step.title}</p>
            <p className="text-sm text-muted-foreground mt-0.5">{step.description}</p>
          </div>
        </li>
      ))}
    </ol>
  )
}

function PlatformCard({
  name,
  icon,
  primaryAsset,
  secondaryAsset,
  primaryLabel,
  secondaryLabel,
  primaryIcon,
  secondaryIcon,
  releasesUrl,
  version,
  steps,
  primaryStepsLabel,
  secondarySteps,
  secondaryStepsLabel,
  packageDistros,
}: PlatformCardProps) {
  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      <div className="p-6 border-b border-border">
        <div className="flex items-center gap-3 mb-4">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-accent/10 text-accent">
            {icon}
          </div>
          <h2 className="text-xl font-semibold">{name}</h2>
        </div>

        <div className="flex flex-col sm:flex-row gap-3">
          {primaryAsset ? (
            <Button
              size="default"
              variant="referee"
              asChild
            >
              <a href={primaryAsset.browser_download_url}>
                {primaryIcon ?? <Download className="h-4 w-4 mr-2" />}
                {primaryLabel}
                <span className="ml-2 text-xs font-normal opacity-75">
                  {formatBytes(primaryAsset.size)}
                </span>
              </a>
            </Button>
          ) : (
            <Button size="default" variant="outline" asChild>
              <a href={releasesUrl} target="_blank" rel="noopener noreferrer">
                {primaryIcon ?? <Download className="h-4 w-4 mr-2" />}
                {primaryLabel}
              </a>
            </Button>
          )}

          {secondaryLabel && (
            secondaryAsset ? (
              <Button size="default" variant="outline" asChild>
                <a href={secondaryAsset.browser_download_url}>
                  {secondaryIcon ?? <Download className="h-4 w-4 mr-2" />}
                  {secondaryLabel}
                  <span className="ml-2 text-xs font-normal opacity-75">
                    {formatBytes(secondaryAsset.size)}
                  </span>
                </a>
              </Button>
            ) : (
              <Button size="default" variant="outline" asChild>
                <a href={releasesUrl} target="_blank" rel="noopener noreferrer">
                  {secondaryIcon ?? <Download className="h-4 w-4 mr-2" />}
                  {secondaryLabel}
                </a>
              </Button>
            )
          )}
        </div>

        {packageDistros && packageDistros.length > 0 && (
          <div className="mt-4 space-y-1.5">
            {packageDistros.map((pkg) => (
              <div key={pkg.label} className="flex items-baseline gap-2 flex-wrap">
                <span className="text-xs font-mono text-accent">{pkg.label}</span>
                <span className="text-xs text-muted-foreground">{pkg.distros.join(" · ")}</span>
              </div>
            ))}
          </div>
        )}
        {primaryAsset && (
          <p className="mt-3 text-xs text-muted-foreground">
            {primaryAsset.name} &middot; updated {formatDate(primaryAsset.updated_at)}
          </p>
        )}
      </div>

      <div className="p-6">
        {secondarySteps ? (
          <div className="space-y-8">
            <div>
              <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground mb-4">
                {primaryStepsLabel ?? "Installation"}
              </h3>
              <StepList steps={steps} />
            </div>
            <div className="border-t border-border pt-8">
              <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground mb-4">
                {secondaryStepsLabel ?? "Installation"}
              </h3>
              <StepList steps={secondarySteps} />
            </div>
          </div>
        ) : (
          <>
            <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground mb-4">
              Installation
            </h3>
            <StepList steps={steps} />
          </>
        )}
      </div>
    </div>
  )
}

const windowsSteps: InstallStep[] = [
  {
    title: "Run the installer",
    description:
      'Double-click the downloaded .exe file. Windows may show a SmartScreen prompt \u2014 click \u201cMore info\u201d then \u201cRun anyway\u201d to proceed.',
  },
  {
    title: "Follow the setup wizard",
    description:
      "Choose your install location and let the installer complete. REFEREE will be added to your Start Menu and system tray.",
  },
  {
    title: "Launch REFEREE",
    description:
      "Start REFEREE from the Start Menu or system tray. It will launch a local HTTP server on port 41300 that any compatible web player can connect to.",
  },
  {
    title: "Connect your player",
    description:
      "Open a REFEREE-integrated web player. It will automatically detect the local server — no configuration required.",
  },
]

const sharedLinuxTail: InstallStep[] = [
  {
    title: "Launch REFEREE",
    description:
      "Search for REFEREE in your app launcher, or run: referee from a terminal.",
  },
  {
    title: "Connect your player",
    description:
      "REFEREE will launch a local HTTP server on port 14002. Open a compatible web player and it will connect automatically.",
  },
]

const debSteps: InstallStep[] = [
  {
    title: "Install the .deb package",
    description: "Run: sudo dpkg -i referee_*.deb",
  },
  {
    title: "Resolve any dependencies",
    description: "If needed, run: sudo apt-get install -f",
  },
  ...sharedLinuxTail,
]

const rpmSteps: InstallStep[] = [
  {
    title: "Install the .rpm package",
    description: "Run: sudo rpm -i referee_*.rpm",
  },
  ...sharedLinuxTail,
]

export default async function DownloadsPage() {
  const releases = await fetchRefereeReleases()
  const release = releases[0] ?? null
  const windowsExe = findNewestReleaseAsset(releases, (asset) => hasExtension(asset, ".exe"))
  const linuxDeb = findNewestReleaseAsset(releases, (asset) => hasExtension(asset, ".deb"))
  const linuxRpm = findNewestReleaseAsset(releases, (asset) => hasExtension(asset, ".rpm"))

  const linuxPackages = [
    linuxDeb
      ? {
          download: linuxDeb,
          label: "Download .deb",
          icon: (
            <svg role="img" viewBox="0 0 24 24" className="h-4 w-4 mr-2 fill-current" aria-hidden="true">
              <path d={siDebian.path} />
            </svg>
          ),
          steps: debSteps,
          stepsLabel: ".deb Installation (Ubuntu · Debian)",
          distros: { label: ".deb", distros: ["Ubuntu 22.04+", "Debian 12+"] },
        }
      : null,
    linuxRpm
      ? {
          download: linuxRpm,
          label: "Download .rpm",
          icon: (
            <svg role="img" viewBox="0 0 24 24" className="h-4 w-4 mr-2 fill-current" aria-hidden="true">
              <path d={siFedora.path} />
            </svg>
          ),
          steps: rpmSteps,
          stepsLabel: ".rpm Installation (Fedora · RHEL)",
          distros: { label: ".rpm", distros: ["Fedora 38+", "RHEL / CentOS Stream 9+"] },
        }
      : null,
  ].filter(isPresent)

  const primaryLinuxPackage = linuxPackages[0]
  const secondaryLinuxPackage = linuxPackages[1]
  const releasesUrl = REFEREE_RELEASES_URL

  return (
    <div className="min-h-screen bg-background text-foreground">
      <Navigation />
      <PageBreadcrumb page="Downloads" />
      <main className="pt-8 pb-20">
        <div className="max-w-6xl mx-auto px-6">
          {/* Header */}
          <div className="mb-12">
            <h1 className="text-3xl sm:text-4xl font-bold tracking-tight mb-4">
              Download <RefereeWordmark variant="display" />
            </h1>
            <p className="text-muted-foreground text-pretty">
              Get started in minutes. Download the installer for your platform and connect
              any compatible web player to your GPU.
            </p>
            <div className="inline-flex items-center gap-2 mb-4">
              {release ? (
                <Badge variant="outline" className="text-accent border-accent/30 bg-accent/5">
                  {release.tag_name}
                </Badge>
              ) : null}
              {release ? (
                <span className="text-xs text-muted-foreground">
                  Released {formatDate(release.published_at)}
                </span>
              ) : null}
            </div>
          </div>

          <div className="mb-8 space-y-4">
            {/* No release warning */}
            {!release && (
              <div className="flex items-start gap-3 rounded-lg border border-amber-500/30 bg-amber-500/5 p-4">
                <AlertCircle className="h-5 w-5 text-amber-500 flex-shrink-0 mt-0.5" />
                <div>
                  <p className="text-sm font-medium">Could not fetch latest release</p>
                  <p className="text-sm text-muted-foreground mt-0.5">
                    Download links are unavailable right now.{" "}
                    <a
                      href={releasesUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-accent underline underline-offset-2 hover:no-underline"
                    >
                      Browse releases on GitHub
                    </a>{" "}
                    instead.
                  </p>
                </div>
              </div>
            )}

            <div className="flex items-start gap-3 rounded-lg border border-blue-500/30 bg-blue-500/5 p-4">
              <AlertCircle className="h-5 w-5 text-blue-500 flex-shrink-0 mt-0.5" />
              <div>
                <p className="text-sm font-medium">Pre-release compatibility varies</p>
                <p className="text-sm text-muted-foreground mt-0.5">
                  REFEREE is still pre-release, and feature parity differs across
                  platform and GPU vendor combinations. Review the{" "}
                  <Link
                    href="/roadmap#compatibility"
                    className="text-accent underline underline-offset-2 hover:no-underline"
                  >
                    compatibility chart on the Roadmap page
                  </Link>{" "}
                  before downloading.
                </p>
              </div>
            </div>
          </div>

          {/* Platform cards */}
          <div className="mb-12 space-y-8">
            {/* Windows — primary / recommended */}
            <PlatformCard
              name="Windows"
              icon={<WindowsIcon className="h-5 w-5" />}
              primaryAsset={windowsExe?.asset}
              primaryLabel="Download .exe"
              releasesUrl={releasesUrl}
              version={windowsExe?.release.tag_name ?? release?.tag_name}
              steps={windowsSteps}
              packageDistros={[
                { label: ".exe", distros: ["Windows 10/11"] },
              ]}
            />

            {/* Linux — early access */}
            {primaryLinuxPackage && (
              <section>
                <div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2">
                  <span className="inline-flex items-center gap-1.5 rounded-full border border-amber-500/30 bg-amber-500/10 px-3 py-1 text-xs font-semibold text-amber-400">
                    <AlertCircle className="h-3 w-3" />
                    Early Access
                  </span>
                  <p className="text-sm text-muted-foreground">
                    Linux support is under active development. Feature parity with Windows is limited and some GPU and distribution combinations may be unstable.
                  </p>
                </div>
                <PlatformCard
                  name="Linux"
                  icon={
                    <svg role="img" viewBox="0 0 24 24" className="h-5 w-5 fill-current" aria-hidden="true">
                      <path d={siLinux.path} />
                    </svg>
                  }
                  primaryAsset={primaryLinuxPackage.download.asset}
                  secondaryAsset={secondaryLinuxPackage?.download.asset}
                  primaryLabel={primaryLinuxPackage.label}
                  secondaryLabel={secondaryLinuxPackage?.label}
                  primaryIcon={primaryLinuxPackage.icon}
                  secondaryIcon={secondaryLinuxPackage?.icon}
                  releasesUrl={releasesUrl}
                  version={primaryLinuxPackage.download.release.tag_name}
                  steps={primaryLinuxPackage.steps}
                  primaryStepsLabel={primaryLinuxPackage.stepsLabel}
                  secondarySteps={secondaryLinuxPackage?.steps}
                  secondaryStepsLabel={secondaryLinuxPackage?.stepsLabel}
                  packageDistros={linuxPackages.map((pkg) => pkg.distros)}
                />
              </section>
            )}
          </div>

          {/* Requirements note */}
          <div className="rounded-lg border border-border bg-card p-6 mb-8">
            <div className="flex items-start gap-3">
              <Monitor className="h-5 w-5 text-accent flex-shrink-0 mt-0.5" />
              <div>
                <h3 className="font-semibold mb-2">System Requirements</h3>
                <ul className="space-y-1.5">
                  {[
                    "Windows 10/11 (64-bit) or modern Linux (x64)",
                    "NVIDIA GeForce RTX 20-series or newer, or AMD Radeon RX 6000-series or newer",
                    "Latest NVIDIA or AMD graphics drivers",
                    "8 GB+ system RAM",
                  ].map((req) => (
                    <li key={req} className="flex items-start gap-2 text-sm text-muted-foreground">
                      <CheckCircle className="h-4 w-4 text-accent flex-shrink-0 mt-0.5" />
                      {req}
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          </div>

          {/* Headless / self-hosted — collapsed by default */}
          <details className="group mb-12">
            <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-lg border border-border bg-card px-5 py-4 transition-colors hover:bg-muted/40">
              <div className="flex items-center gap-3">
                <Terminal className="h-5 w-5 flex-shrink-0 text-accent" />
                <div>
                  <p className="text-sm font-semibold">For the terminally inclined</p>
                  <p className="mt-0.5 text-xs text-muted-foreground">Headless server, Docker, and other install methods</p>
                </div>
              </div>
              <ChevronDown className="h-4 w-4 flex-shrink-0 text-muted-foreground transition-transform group-open:rotate-180" />
            </summary>
            <HeadlessServerSetup cardSections />
          </details>

          {/* Bottom links */}
          <div className="flex flex-col sm:flex-row items-center justify-center gap-4 text-sm text-muted-foreground">
            <a
              href={releasesUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1.5 hover:text-foreground transition-colors"
            >
              <ExternalLink className="h-4 w-4" />
              All releases on GitHub
            </a>
            <span className="hidden sm:block">&middot;</span>
            <Link
              href="/developers"
              className="flex items-center gap-1.5 hover:text-foreground transition-colors"
            >
              <Terminal className="h-4 w-4" />
              Integration guide
            </Link>
          </div>
        </div>
      </main>
      <Footer />
    </div>
  )
}
