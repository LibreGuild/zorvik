import { describe, expect, it } from "vitest";
import type { CatalogEntry } from "../../bindings/CatalogEntry";
import type { CatalogPreview } from "../../bindings/CatalogPreview";
import {
  CATEGORIES,
  CATEGORY_LABELS,
  categoryCounts,
  downloadUrl,
  entryLabels,
  filterGroups,
  foldersTo,
  groupLabel,
  hostOf,
  initialTicked,
  licenseOf,
  needsPicking,
  progressText,
  requestCount,
  searchCatalog,
  sourceText,
  tickAll,
  tickedTags,
  tickedTotal,
  webUrl,
} from "./model";

function entry(id: string, name: string, patch: Partial<CatalogEntry> = {}): CatalogEntry {
  return {
    id,
    name,
    category: "devTools",
    description: `${name} API`,
    website: `https://docs.example.com/${id}`,
    auth: "Bearer token",
    format: "OpenAPI 3.0",
    source: { type: "download", url: `https://raw.githubusercontent.com/${id}/openapi.json` },
    ...patch,
  };
}

const openai = entry("openai", "OpenAI", {
  category: "ai",
  description: "GPT models, embeddings and images.",
  format: "OpenAPI 3.1",
  license: { name: "MIT", url: "https://example.com/LICENSE" },
});
const groq = entry("groq", "Groq", {
  category: "ai",
  description: "Fast inference.",
  format: "OpenAI-compatible",
  source: { type: "compatible", of: "openai", baseUrl: "https://api.groq.com/openai/v1", paths: ["/models"] },
});
const anthropic = entry("anthropic", "Anthropic", {
  category: "ai",
  description: "Claude models through the Messages API.",
  format: "Written for Zorvik",
  source: { type: "starter", file: "anthropic.yaml" },
});
const github = entry("github", "GitHub", { description: "Repositories, issues and pull requests." });
const githubGraphql = entry("github-graphql", "GitHub GraphQL", {
  format: "GraphQL",
  source: { type: "graphql", url: "https://api.github.com/graphql", query: "{ viewer { login } }", auth: { type: "none" } },
});
const hubspot = entry("hubspot", "HubSpot", {
  category: "marketing",
  description: "CRM objects, marketing, sales and service.",
  source: { type: "link", reason: "Not licensed for use outside HubSpot." },
});
const stripe = entry("stripe", "Stripe", { category: "payments", description: "Payments, customers and subscriptions." });
const ALL = [stripe, openai, hubspot, githubGraphql, groq, github, anthropic];

describe("categories", () => {
  it("has a readable label for every category", () => {
    expect(CATEGORIES.map((c) => c.id)).toEqual([
      "ai",
      "cloud",
      "devTools",
      "monitoring",
      "data",
      "messaging",
      "productivity",
      "payments",
      "commerce",
      "marketing",
      "identity",
      "media",
    ]);
    expect(CATEGORY_LABELS.devTools).toBe("Developer tools");
    expect(CATEGORY_LABELS.marketing).toBe("CRM & marketing");
    expect(CATEGORY_LABELS.data).toBe("Databases & search");
  });
});

describe("searchCatalog", () => {
  const names = (list: CatalogEntry[]) => list.map((e) => e.name);

  it("lists everything by name when nothing is searched", () => {
    expect(names(searchCatalog(ALL, "", "all"))).toEqual(["Anthropic", "GitHub", "GitHub GraphQL", "Groq", "HubSpot", "OpenAI", "Stripe"]);
  });

  it("keeps one category", () => {
    expect(names(searchCatalog(ALL, "", "ai"))).toEqual(["Anthropic", "Groq", "OpenAI"]);
    expect(searchCatalog(ALL, "", "cloud")).toEqual([]);
  });

  it("matches the name, id, description and category, every word, any case", () => {
    expect(names(searchCatalog(ALL, "ANTHROPIC", "all"))).toEqual(["Anthropic"]);
    expect(names(searchCatalog(ALL, "github-graphql", "all"))).toEqual(["GitHub GraphQL"]);
    expect(names(searchCatalog(ALL, "subscriptions", "all"))).toEqual(["Stripe"]);
    expect(names(searchCatalog(ALL, "crm", "all"))).toEqual(["HubSpot"]);
    expect(names(searchCatalog(ALL, "developer tools", "all"))).toEqual(["GitHub", "GitHub GraphQL"]);
    expect(names(searchCatalog(ALL, "models claude", "all"))).toEqual(["Anthropic"]);
    expect(searchCatalog(ALL, "nothing like this", "all")).toEqual([]);
  });

  it("puts names that start with the query first", () => {
    expect(names(searchCatalog(ALL, "hub", "all"))).toEqual(["HubSpot", "GitHub", "GitHub GraphQL"]);
  });

  it("searches within the category", () => {
    expect(names(searchCatalog(ALL, "models", "ai"))).toEqual(["Anthropic", "OpenAI"]);
    expect(searchCatalog(ALL, "models", "payments")).toEqual([]);
  });

  it("doesn't reorder the list it was given", () => {
    const list = [...ALL];
    searchCatalog(list, "", "all");
    expect(list).toEqual(ALL);
  });
});

describe("categoryCounts", () => {
  it("counts every category, and all of them", () => {
    const counts = categoryCounts(ALL, "");
    expect(counts.all).toBe(7);
    expect(counts.ai).toBe(3);
    expect(counts.devTools).toBe(2);
    expect(counts.payments).toBe(1);
    expect(counts.cloud).toBe(0);
  });

  it("counts what the search matches", () => {
    const counts = categoryCounts(ALL, "models");
    expect(counts.all).toBe(2);
    expect(counts.ai).toBe(2);
    expect(counts.devTools).toBe(0);
  });
});

describe("what an entry shows", () => {
  it("labels the format, OpenAI-compatible and link-only entries, once each", () => {
    expect(entryLabels(openai)).toEqual(["OpenAPI 3.1"]);
    expect(entryLabels(groq)).toEqual(["OpenAI-compatible"]);
    expect(entryLabels({ ...groq, format: "OpenAPI 3.1" })).toEqual(["OpenAPI 3.1", "OpenAI-compatible"]);
    expect(entryLabels(hubspot)).toEqual(["OpenAPI 3.0", "Link only"]);
  });

  it("says where the requests come from", () => {
    expect(sourceText(openai, ALL)).toBe("Downloads from raw.githubusercontent.com");
    expect(sourceText(groq, ALL)).toBe("Uses the OpenAI API description");
    expect(sourceText(anthropic, ALL)).toBe("Written for Zorvik");
    expect(sourceText(githubGraphql, ALL)).toBe("One GraphQL request; the editor completes from the schema");
    expect(sourceText(hubspot, ALL)).toBeNull();
    // An unknown entry to be compatible with: its id.
    expect(sourceText(groq, [groq])).toBe("Uses the openai API description");
  });

  it("names the host it downloads from while importing", () => {
    expect(progressText(stripe, ALL)).toBe("Downloading from raw.githubusercontent.com…");
    expect(progressText(groq, ALL)).toBe("Downloading from raw.githubusercontent.com…");
    expect(progressText(anthropic, ALL)).toBe("Importing Anthropic…");
    expect(downloadUrl(groq, ALL)).toBe("https://raw.githubusercontent.com/openai/openapi.json");
    expect(downloadUrl(githubGraphql, ALL)).toBeNull();
  });

  it("takes a host from a URL", () => {
    expect(hostOf("https://raw.githubusercontent.com/a/b.json")).toBe("raw.githubusercontent.com");
    expect(hostOf("https://api.example.com:8443/x")).toBe("api.example.com:8443");
    expect(hostOf("not a url")).toBe("not a url");
  });

  it("opens web addresses only", () => {
    expect(webUrl("https://docs.stripe.com/api")).toBe("https://docs.stripe.com/api");
    expect(webUrl("http://www.apache.org/licenses/LICENSE-2.0.html")).toBe("http://www.apache.org/licenses/LICENSE-2.0.html");
    expect(webUrl("javascript:alert(1)")).toBeNull();
    expect(webUrl("file:///etc/passwd")).toBeNull();
    expect(webUrl("docs.example.com")).toBeNull();
    expect(webUrl("")).toBeNull();
    expect(webUrl(undefined)).toBeNull();
  });

  it("gives an OpenAI-compatible API the license of OpenAI's description", () => {
    expect(licenseOf(openai, ALL)?.name).toBe("MIT");
    expect(licenseOf(groq, ALL)?.name).toBe("MIT");
    expect(licenseOf(anthropic, ALL)).toBeUndefined();
  });
});

describe("groups of a large API", () => {
  const preview = (operations: number, selected: string[] = []): CatalogPreview => ({
    operations,
    groups: [
      { tag: "repos", operations: 200 },
      { tag: "issues", operations: 80 },
      { tag: "users", operations: 30 },
      { tag: "", operations: 5 },
    ],
    selected,
  });

  it("asks only when the API is large and has several groups", () => {
    expect(needsPicking(preview(315))).toBe(true);
    expect(needsPicking(preview(300))).toBe(false);
    expect(needsPicking({ operations: 900, groups: [{ tag: "all", operations: 900 }], selected: [] })).toBe(false);
    expect(needsPicking({ operations: 900, groups: [], selected: [] })).toBe(false);
  });

  it("ticks the suggested groups, or all of them", () => {
    expect([...initialTicked(preview(315, ["issues", "users"]))]).toEqual(["issues", "users"]);
    expect([...initialTicked(preview(315))]).toEqual(["repos", "issues", "users", ""]);
    // Suggestions the API no longer has don't count.
    expect([...initialTicked(preview(315, ["gone"]))]).toEqual(["repos", "issues", "users", ""]);
  });

  it("totals the ticked groups and keeps the API's order", () => {
    const groups = preview(315).groups;
    expect(tickedTotal(groups, new Set(["repos", "users"]))).toBe(230);
    expect(tickedTotal(groups, new Set())).toBe(0);
    expect(tickedTags(groups, new Set(["users", "", "repos"]))).toEqual(["repos", "users", ""]);
  });

  it("finds groups by name, the untagged ones as Other", () => {
    const groups = preview(315).groups;
    expect(groupLabel("")).toBe("Other");
    expect(filterGroups(groups, "SER").map((g) => g.tag)).toEqual(["users"]);
    expect(filterGroups(groups, "other").map((g) => g.tag)).toEqual([""]);
    expect(filterGroups(groups, "")).toHaveLength(4);
  });

  it("selects all or none of the groups shown, leaving the others", () => {
    const groups = preview(315).groups;
    const shown = filterGroups(groups, "s"); // repos, issues, users
    expect([...tickAll(new Set([""]), shown, true)].sort()).toEqual(["", "issues", "repos", "users"]);
    expect([...tickAll(new Set(["", "repos"]), shown, false)]).toEqual([""]);
  });

  it("counts requests", () => {
    expect(requestCount(1)).toBe("1 request");
    expect(requestCount(1234)).toBe("1,234 requests");
  });
});

describe("foldersTo", () => {
  it("lists the folders down to a path", () => {
    expect(foldersTo("Anthropic")).toEqual(["Anthropic"]);
    expect(foldersTo("APIs/AI/Anthropic")).toEqual(["APIs", "APIs/AI", "APIs/AI/Anthropic"]);
    expect(foldersTo("")).toEqual([]);
  });
});
