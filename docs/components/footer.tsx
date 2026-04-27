import { Github, FileText, Download } from "lucide-react"
import { RefereeWordmark } from "@/components/referee-wordmark"
import Link from "next/link"

export function Footer() {
  const links = [
    {
      title: "Resources",
      items: [
        {
          label: "GitHub Repository",
          href: "https://github.com/cmacrowther/referee",
          icon: Github,
          external: true,
        },
        {
          label: "Downloads",
          href: "/downloads",
          icon: Download,
          external: false,
        },
        {
          label: "Integration Guide",
          href: "/integrate",
          icon: FileText,
          external: true,
        },
      ],
    },
  ]

  return (
    <footer className="py-16 border-t border-border">
      <div className="max-w-6xl mx-auto px-6 sm:px-6 md:px-6">
        <div className="flex flex-col md:flex-row justify-between gap-12">
          <div className="max-w-sm">
            <h3 className="text-xl font-semibold tracking-tight mb-4">
              <RefereeWordmark variant="display" />
            </h3>
            <p className="text-sm text-muted-foreground leading-relaxed">
              Hardware-accelerated video upscaling for web-based media players. 
              Built to leverage supported local GPU hardware for real-time AI enhancement.
            </p>
          </div>

          {links.map((section) => (
            <div key={section.title}>
              <h4 className="font-semibold mb-4">{section.title}</h4>
              <ul className="space-y-3">
                {section.items.map((item) => (
                  <li key={item.label}>
                    {item.external ? (
                      <a
                        href={item.href}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-sm text-muted-foreground hover:text-foreground transition-colors flex items-center gap-2"
                      >
                        <item.icon className="h-4 w-4" />
                        {item.label}
                      </a>
                    ) : (
                      <Link
                        href={item.href}
                        className="text-sm text-muted-foreground hover:text-foreground transition-colors flex items-center gap-2"
                      >
                        <item.icon className="h-4 w-4" />
                        {item.label}
                      </Link>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>

        <div className="mt-12 pt-8 border-t border-border flex flex-col sm:flex-row justify-between items-center gap-4">
          <p className="text-sm text-muted-foreground">
            Built by{" "}
            <a
              href="https://craw.ca/"
              target="_blank"
              rel="noopener noreferrer"
              className="text-foreground hover:text-accent transition-colors"
            >
              Colin Crowther
            </a>
          </p>
          <p className="text-sm text-muted-foreground">
            Open source under the{" "}
             <a
              href="https://www.gnu.org/licenses/gpl-3.0.en.html"
              target="_blank"
              rel="noopener noreferrer"
              className="text-foreground hover:text-accent transition-colors"
            >
              GPLv3 license.
            </a>
          </p>
        </div>

        <div className="mt-6 space-y-4 text-justify">
          <p className="text-xs text-muted-foreground leading-relaxed">
            NVIDIA, the NVIDIA logo, GeForce RTX, RTX, DLSS, and related marks are trademarks and/or registered trademarks of NVIDIA
            Corporation in the U.S. and other countries. This project is an independent, unofficial demo and is not affiliated with,
            endorsed by, or sponsored by NVIDIA Corporation. All rights in NVIDIA branding and technologies remain the property of NVIDIA
            Corporation, and no ownership or intellectual property rights are claimed by this project.
          </p>
          <p className="text-xs text-muted-foreground leading-relaxed">
            AMD, the AMD Arrow logo, Radeon, FidelityFX, and related marks are trademarks and/or registered trademarks of Advanced Micro
            Devices, Inc. in the U.S. and other countries. This project is an independent, unofficial demo and is not affiliated with,
            endorsed by, or sponsored by AMD. All rights in AMD branding and technologies remain the property of Advanced Micro Devices,
            Inc., and no ownership or intellectual property rights are claimed by this project.
          </p>
          <p className="text-xs text-muted-foreground leading-relaxed">
            REFEREE is provided &quot;as is&quot;, without warranty
            of any kind, express or implied.
          </p>
          <p className="text-xs text-muted-foreground leading-relaxed">
            This project is a neutral, local network utility designed strictly for personal use. It acts solely as a conduit to process
            data provided by the user. The creator and contributors of REFEREE do not host, provide, or
            distribute media content; are not affiliated with, endorsed by, or connected to NVIDIA Corporation, Advanced Micro Devices,
            Inc., or streaming service providers; and accept no liability for end-user use of this software.
          </p>
          <p className="text-xs text-muted-foreground leading-relaxed">
            <span className="font-semibold text-foreground">User Responsibility:</span> Users are solely responsible for ensuring they have
            the legal right to access, process, and manipulate media streams used with this application, including compliance with
            applicable Terms of Service and DRM policies. The creator and contributors assume no responsibility for copyright
            infringement, account bans, or legal/technical consequences resulting from software use.
          </p>
        </div>
      </div>
    </footer>
  )
}
