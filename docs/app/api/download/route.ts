import { NextRequest, NextResponse } from "next/server"

export const dynamic = "force-dynamic"

type ReleaseAsset = { name: string; browser_download_url: string }

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

function pickReleaseAsset(assets: ReleaseAsset[], platform: "windows" | "linux") {
  if (platform === "linux") {
    return assets.find((a) => a.name.endsWith(".AppImage"))
      ?? assets.find((a) => a.name.endsWith(".deb"))
  }

  return assets.find((a) => a.name.endsWith(".exe"))
}

export async function GET(request: NextRequest) {
  const res = await fetch(
    "https://api.github.com/repos/cmacrowther/referee/releases/latest",
    { headers: { Accept: "application/vnd.github+json" }, next: { revalidate: 0 } }
  )

  if (!res.ok) {
    return NextResponse.redirect(
      "https://github.com/cmacrowther/referee/releases"
    )
  }

  const release = await res.json()
  const assets = (release.assets as ReleaseAsset[]) ?? []
  const requestedPlatform = resolveRequestedPlatform(request)
  const asset = pickReleaseAsset(assets, requestedPlatform)

  if (!asset) {
    return NextResponse.redirect(
      `https://github.com/cmacrowther/referee/releases/tag/${release.tag_name}`
    )
  }

  return NextResponse.redirect(asset.browser_download_url)
}
