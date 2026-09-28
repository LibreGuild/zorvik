// What a request can be copied as: cURL for three shells, or code in a language and library.
// Listed by language; a language with several libraries (or shells) has variants.
import type { CurlFlavor } from "../../bindings/CurlFlavor";
import type { SnippetLanguage } from "../../bindings/SnippetLanguage";
import type { EditorLanguage } from "../CodeEditor";

export type ExportTarget = { kind: "curl"; flavor: CurlFlavor } | { kind: "code"; language: SnippetLanguage };

export interface ExportVariant {
  id: string;
  /** The library or shell, e.g. "axios". */
  label: string;
  target: ExportTarget;
}

export interface ExportLanguage {
  id: string;
  label: string;
  group: "Command line" | "Code";
  highlight: EditorLanguage;
  variants: ExportVariant[];
  /** Other words people search for (platforms, runtimes). */
  keywords: string;
}

const code = (language: SnippetLanguage, label: string): ExportVariant => ({ id: language, label, target: { kind: "code", language } });

export const EXPORT_LANGUAGES: ExportLanguage[] = [
  {
    id: "curl",
    label: "cURL",
    group: "Command line",
    highlight: "shell",
    keywords: "terminal shell bash zsh windows cmd powershell",
    variants: [
      { id: "curl-bash", label: "bash / zsh", target: { kind: "curl", flavor: "bash" } },
      { id: "curl-cmd", label: "Windows cmd", target: { kind: "curl", flavor: "cmd" } },
      { id: "curl-powershell", label: "PowerShell", target: { kind: "curl", flavor: "powerShell" } },
    ],
  },
  { id: "httpie", label: "HTTPie", group: "Command line", highlight: "shell", keywords: "terminal shell http", variants: [code("httpie", "HTTPie")] },
  { id: "wget", label: "Wget", group: "Command line", highlight: "shell", keywords: "terminal shell", variants: [code("wget", "Wget")] },
  {
    id: "powershell",
    label: "PowerShell",
    group: "Command line",
    highlight: "powershell",
    keywords: "windows invoke-webrequest",
    variants: [code("powerShell", "Invoke-WebRequest")],
  },
  {
    id: "javascript",
    label: "JavaScript",
    group: "Code",
    highlight: "javascript",
    keywords: "node nodejs browser typescript js",
    variants: [code("javascript", "fetch"), code("javascriptAxios", "axios")],
  },
  { id: "python", label: "Python", group: "Code", highlight: "python", keywords: "py", variants: [code("python", "requests"), code("pythonHttpx", "HTTPX")] },
  { id: "go", label: "Go", group: "Code", highlight: "go", keywords: "golang", variants: [code("go", "net/http")] },
  { id: "java", label: "Java", group: "Code", highlight: "java", keywords: "jvm", variants: [code("java", "HttpClient")] },
  { id: "kotlin", label: "Kotlin", group: "Code", highlight: "kotlin", keywords: "android jvm", variants: [code("kotlin", "OkHttp")] },
  { id: "swift", label: "Swift", group: "Code", highlight: "swift", keywords: "ios macos apple", variants: [code("swift", "URLSession")] },
  { id: "csharp", label: "C#", group: "Code", highlight: "csharp", keywords: "csharp dotnet .net", variants: [code("csharp", "HttpClient")] },
  { id: "php", label: "PHP", group: "Code", highlight: "php", keywords: "", variants: [code("php", "curl")] },
  { id: "ruby", label: "Ruby", group: "Code", highlight: "ruby", keywords: "rails", variants: [code("ruby", "Net::HTTP")] },
  { id: "rust", label: "Rust", group: "Code", highlight: "rust", keywords: "", variants: [code("rust", "reqwest")] },
  { id: "dart", label: "Dart", group: "Code", highlight: "dart", keywords: "flutter", variants: [code("dart", "http")] },
  { id: "c", label: "C", group: "Code", highlight: "c", keywords: "libcurl", variants: [code("c", "libcurl")] },
];

/** The language and variant of a variant id. */
export function findVariant(id: string | null | undefined): { language: ExportLanguage; variant: ExportVariant } | null {
  for (const language of EXPORT_LANGUAGES) {
    const variant = language.variants.find((v) => v.id === id);
    if (variant) return { language, variant };
  }
  return null;
}

/** Languages whose name, libraries or keywords contain every word of `query`. */
export function searchLanguages(query: string): ExportLanguage[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return EXPORT_LANGUAGES;
  return EXPORT_LANGUAGES.filter((l) => {
    const text = [l.label, l.keywords, ...l.variants.map((v) => v.label)].join(" ").toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

const REMEMBERED = "zorvik.export.variant";

/** The last variant used, else cURL for this computer's shell. */
export function initialVariant(windows: boolean): string {
  try {
    const saved = localStorage.getItem(REMEMBERED);
    if (findVariant(saved)) return saved as string;
  } catch {
    // Storage unavailable: the default.
  }
  return windows ? "curl-cmd" : "curl-bash";
}

export function rememberVariant(id: string) {
  try {
    localStorage.setItem(REMEMBERED, id);
  } catch {
    // Not remembered; nothing else depends on it.
  }
}
