// The latest release, read from GitHub when the site is built (the Pages workflow rebuilds it
// after every release and once a day). Download links always use `releases/latest/download/…`,
// which GitHub points at the newest release by itself, so they never go stale.

export const REPO = "LibreGuild/zorvik";
const API = `https://api.github.com/repos/${REPO}/releases?per_page=100`;

export type Os = "windows" | "macos" | "linux";

export interface Download {
  os: Os;
  file: string;
  label: string;
  note: string;
  /** The one to offer first on that system. */
  primary: boolean;
  url: string;
  /** Bytes, when the release is known. */
  size: number | null;
}

export interface ReleaseInfo {
  version: string | null;
  tag: string | null;
  date: string | null;
  notesUrl: string;
  /** Every download of every versioned release (nightlies are replaced, so they don't count). */
  downloads: number | null;
  files: Download[];
}

const FILES: Omit<Download, "url" | "size">[] = [
  { os: "windows", file: "Zorvik-Windows-Setup-x64.exe", label: "Installer", note: "Windows 10/11 · no admin rights needed", primary: true },
  { os: "windows", file: "Zorvik-Windows-Portable-x64.zip", label: "Portable", note: "Unzip and run, nothing installed", primary: false },
  { os: "macos", file: "Zorvik-macOS-universal.dmg", label: "Disk image", note: "macOS 11+ · Apple silicon and Intel", primary: true },
  { os: "linux", file: "Zorvik-Linux-amd64.deb", label: ".deb", note: "Ubuntu 22.04+, Debian 12+", primary: true },
  { os: "linux", file: "Zorvik-Linux-x86_64.rpm", label: ".rpm", note: "Fedora, RHEL, openSUSE", primary: false },
  { os: "linux", file: "Zorvik-Linux-x86_64.AppImage", label: "AppImage", note: "Any x86-64 Linux", primary: false },
];

interface GhAsset {
  name: string;
  size: number;
  download_count: number;
}
interface GhRelease {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
  html_url: string;
  assets: GhAsset[];
}

export async function latestRelease(): Promise<ReleaseInfo> {
  let releases: GhRelease[] = [];
  try {
    const headers: Record<string, string> = { accept: "application/vnd.github+json", "user-agent": "zorvik-website" };
    const token = process.env.GITHUB_TOKEN;
    if (token) headers.authorization = `Bearer ${token}`;
    const res = await fetch(API, { headers, signal: AbortSignal.timeout(15_000) });
    if (res.ok) releases = (await res.json()) as GhRelease[];
    else console.warn(`releases: GitHub answered ${res.status}; the page shows no version`);
  } catch (e) {
    console.warn(`releases: ${e}; the page shows no version`);
  }
  const versioned = releases.filter((r) => !r.draft && !r.prerelease && r.tag_name.startsWith("v"));
  const latest = versioned[0];
  const sizes = new Map(latest?.assets.map((a) => [a.name, a.size]) ?? []);
  const downloads = versioned.length ? versioned.reduce((n, r) => n + r.assets.reduce((m, a) => m + a.download_count, 0), 0) : null;
  return {
    version: latest ? latest.tag_name.replace(/^v/, "") : null,
    tag: latest?.tag_name ?? null,
    date: latest?.published_at ?? null,
    notesUrl: latest?.html_url ?? `https://github.com/${REPO}/releases/latest`,
    downloads,
    files: FILES.map((f) => ({ ...f, url: `https://github.com/${REPO}/releases/latest/download/${f.file}`, size: sizes.get(f.file) ?? null })),
  };
}

export const formatSize = (bytes: number | null) => (bytes == null ? "" : `${(bytes / 1_048_576).toFixed(bytes > 104_857_600 ? 0 : 1)} MB`);
