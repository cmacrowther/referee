import { Navigation } from "@/components/navigation"
import { PageBreadcrumb } from "@/components/page-breadcrumb"
import { CodeExamples } from "@/components/code-examples"
import { ApiReference } from "@/components/api-reference"
import { IntegrateFAQ } from "@/components/integrate-faq"
import { CorsProxyGuide } from "@/components/headless-server"
import { IntegrateSidebar } from "@/components/integrate-sidebar"
import { Footer } from "@/components/footer"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Tag } from "lucide-react"

export const metadata = {
  title: "For Developers — REFEREE",
  description: "Code examples and full API reference for integrating REFEREE into your application.",
}

export default function DevelopersPage() {
  return (
    <main className="min-h-screen">
      <Navigation />
      <div className="pt-16">
        <div className="mx-auto flex max-w-6xl gap-8 px-6 xl:gap-12">
          <aside className="hidden w-60 shrink-0 self-stretch border-r border-border pr-6 xl:block">
            <div className="sticky top-20 max-h-[calc(100vh-5rem)] overflow-y-auto">
              <IntegrateSidebar />
            </div>
          </aside>
          <div className="min-w-0 flex-1">
            <PageBreadcrumb page="For Developers" className="pt-8 pb-0" />
            <div className="pt-10 pb-0">
              <Alert className="border-accent/25 bg-accent/8">
                <Tag className="h-4 w-4 text-accent" />
                <AlertTitle className="text-foreground">Versioned API — <code className="font-mono">/v1/</code></AlertTitle>
                <AlertDescription>
                  <p>
                    All endpoints are under the <code>/v1/</code> prefix. Mutating endpoints require an{" "}
                    <code>X-Referee-Token</code> header. Browser clients obtain this via{" "}
                    <code>POST /v1/auth/request</code> (desktop shows a consent dialog; headless requires
                    the origin to be pre-approved). Set <code>REFEREE_API_TOKEN</code> and{" "}
                    <code>REFEREE_ALLOWED_ORIGINS</code> env vars for headless deployments.
                  </p>
                </AlertDescription>
              </Alert>
            </div>
            <CodeExamples />
            <div className="pt-10 pb-0">
              <CorsProxyGuide />
            </div>
            <ApiReference />
            <IntegrateFAQ />
          </div>
        </div>
      </div>
      <Footer />
    </main>
  )
}
