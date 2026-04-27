"use client"

import { useState } from "react"
import { usePathname } from "next/navigation"
import { Button } from "@/components/ui/button"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { Menu, X, Github, Play } from "lucide-react"
import Link from "next/link"

export function Navigation() {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const pathname = usePathname()

  return (
    <nav className="fixed top-0 left-0 right-0 z-50 bg-background/80 backdrop-blur-md border-b border-border">
      <div className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between">
        <div className="flex items-center gap-8">
          <Link href="/" className="flex items-center text-foreground" aria-label="REFEREE">
            <RefereeWordmark variant="nav" />
          </Link>

          <div className="hidden md:flex items-center">
          <Link
            href="/"
            className={`text-sm transition-colors px-4 ${pathname === "/" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
          >
            Home
          </Link>
          <span className="w-px h-4 bg-border" aria-hidden="true" />
          <Link
            href="/usage"
            className={`text-sm transition-colors px-4 ${pathname === "/usage" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
          >
            User Guide
          </Link>
          <span className="w-px h-4 bg-border" aria-hidden="true" />
          <Link
            href="/developers"
            className={`text-sm transition-colors px-4 ${pathname === "/developers" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
          >
            For Developers
          </Link>
          <span className="w-px h-4 bg-border" aria-hidden="true" />
          <Link
            href="/downloads"
            className={`text-sm transition-colors px-4 ${pathname === "/downloads" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
          >
            Downloads
          </Link>
          <span className="w-px h-4 bg-border" aria-hidden="true" />
          <Link
            href="/demo"
            className={`text-sm transition-colors px-4 ${pathname === "/demo" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
          >
            Demo
          </Link>
          <span className="w-px h-4 bg-border" aria-hidden="true" />
          <Link
            href="/roadmap"
            className={`text-sm transition-colors px-4 ${pathname === "/roadmap" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
          >
            Roadmap
          </Link>
          </div>
        </div>

        <div className="hidden md:flex items-center gap-3">
          <Button variant="ghost" size="sm" asChild>
            <a
              href="https://github.com/cmacrowther/referee"
              target="_blank"
              rel="noopener noreferrer"
            >
              <Github className="h-4 w-4 mr-2" />
              GitHub
            </a>
          </Button>
          <Button size="sm" variant="referee" asChild>
            <Link href="/demo">
              <Play className="h-4 w-4 mr-2 fill-current" />
              Try Demo
            </Link>
          </Button>
        </div>

        <button
          className="md:hidden p-2"
          onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
        >
          {mobileMenuOpen ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
        </button>
      </div>

      {mobileMenuOpen && (
        <div className="md:hidden border-t border-border bg-background">
          <div className="px-6 py-4 space-y-3">
            <Link
              href="/"
              className={`block text-sm transition-colors ${pathname === "/" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setMobileMenuOpen(false)}
            >
              Home
            </Link>
            <Link
              href="/usage"
              className={`block text-sm transition-colors ${pathname === "/usage" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setMobileMenuOpen(false)}
            >
              User Guide
            </Link>
            <Link
              href="/developers"
              className={`block text-sm transition-colors ${pathname === "/developers" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setMobileMenuOpen(false)}
            >
              For Developers
            </Link>
            <Link
              href="/downloads"
              className={`block text-sm transition-colors ${pathname === "/downloads" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setMobileMenuOpen(false)}
            >
              Downloads
            </Link>
            <Link
              href="/demo"
              className={`block text-sm transition-colors ${pathname === "/demo" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setMobileMenuOpen(false)}
            >
              Demo
            </Link>
            <Link
              href="/roadmap"
              className={`block text-sm transition-colors ${pathname === "/roadmap" ? "text-accent" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setMobileMenuOpen(false)}
            >
              Roadmap
            </Link>
            <div className="pt-3 border-t border-border space-y-2">
              <Button variant="ghost" size="sm" className="w-full justify-start" asChild>
                <a
                  href="https://github.com/cmacrowther/referee"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <Github className="h-4 w-4 mr-2" />
                  GitHub
                </a>
              </Button>
              <Button size="sm" variant="referee" className="w-full" asChild>
                <Link href="/demo">
                  <Play className="h-4 w-4 mr-2 fill-current" />
                  Try Demo
                </Link>
              </Button>
            </div>
          </div>
        </div>
      )}
    </nav>
  )
}
