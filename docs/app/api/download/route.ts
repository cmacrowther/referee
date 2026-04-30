import { NextRequest, NextResponse } from "next/server"
import {
  REFEREE_RELEASES_URL,
  fetchRefereeReleases,
  findNewestReleaseAsset,
  hasExtension,
} from "@/lib/github-releases"

export const dynamic = "force-dynamic"

function resolveRequestedPlatform(request: NextRequest): "windows" | "linux" {
  const platform = request.nextUrl.searchParams.get("platform")?.toLowerCase()
  if (platform === "linux") {
    return "linux"
  }
  if (platform === "windows") {
    return "windows"
  }

  const userAgent = request.headers.get("user-agent")?.toLowerCase() ?? ""
  return userAgent.includes("linux") ? "linux" : "windows"
}

function pickReleaseDownload(
  releases: Awaited<ReturnType<typeof fetchRefereeReleases>>,
  platform: "windows" | "linux"
) {
  if (platform === "linux") {
    return findNewestReleaseAsset(releases, (asset) => hasExtension(asset, ".deb"))
      ?? findNewestReleaseAsset(releases, (asset) => hasExtension(asset, ".rpm"))
  }

  return findNewestReleaseAsset(releases, (asset) => hasExtension(asset, ".exe"))
}

export async function GET(request: NextRequest) {
  const releases = await fetchRefereeReleases(0)
  const requestedPlatform = resolveRequestedPlatform(request)
  const download = pickReleaseDownload(releases, requestedPlatform)

  if (!download) {
    return NextResponse.redirect(REFEREE_RELEASES_URL)
  }

  return NextResponse.redirect(download.asset.browser_download_url)
}
