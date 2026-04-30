export const REFEREE_RELEASES_URL = "https://github.com/cmacrowther/referee/releases"

export type ReleaseAsset = {
  name: string
  browser_download_url: string
  size: number
  updated_at: string
}

export type Release = {
  tag_name: string
  name: string
  published_at: string
  assets: ReleaseAsset[]
  html_url: string
  draft?: boolean
}

export type ReleaseDownload = {
  release: Release
  asset: ReleaseAsset
}

const RELEASES_API_URL =
  "https://api.github.com/repos/cmacrowther/referee/releases?per_page=30"

export async function fetchRefereeReleases(
  revalidate = 3600
): Promise<Release[]> {
  try {
    const res = await fetch(RELEASES_API_URL, {
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "referee-docs",
      },
      next: { revalidate },
    })

    if (!res.ok) return []

    const releases = await res.json()
    if (!Array.isArray(releases)) return []

    return releases.filter((release): release is Release => {
      return Boolean(release?.tag_name && !release.draft)
    })
  } catch {
    return []
  }
}

export function findNewestReleaseAsset(
  releases: Release[],
  predicate: (asset: ReleaseAsset) => boolean
): ReleaseDownload | null {
  for (const release of releases) {
    const asset = release.assets?.find(predicate)
    if (asset) return { release, asset }
  }

  return null
}

export function hasExtension(asset: ReleaseAsset, extension: string): boolean {
  return asset.name.toLowerCase().endsWith(extension.toLowerCase())
}
