// The Zorvik website (GitHub Pages): a landing page (src/pages/index.astro) and the developer
// docs under /docs (Starlight, src/content/docs/docs). The same sources build the main site
// (/zorvik/, from the latest release) and the preview of the next version (/zorvik/next/, from
// main, with SITE_BASE and SITE_CHANNEL=next); see src/lib/site.mjs.
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import { BASE, NEXT, RELEASE_BASE } from "./src/lib/site.mjs";

const GOATCOUNTER = "https://zorvik.goatcounter.com/count";

// Pictures and links in the docs are written with the main site's path (/zorvik/shots/...). A
// build with another base moves them there, so the preview shows its own pictures, new ones too.
const rebase = () => (tree) => {
  if (BASE === RELEASE_BASE) return;
  const visit = (node) => {
    if (typeof node.url === "string" && node.url.startsWith(`${RELEASE_BASE}/`)) node.url = BASE + node.url.slice(RELEASE_BASE.length);
    node.children?.forEach(visit);
  };
  visit(tree);
};

const group = (label, directory) => ({ label, items: [{ autogenerate: { directory: `docs/${directory}` } }] });

export default defineConfig({
  // The preview has no `site`, so it gets no sitemap and no canonical links (Starlight's sitemap
  // integration logs that it skips); with noindex below, search engines only list the main site.
  site: NEXT ? undefined : "https://libreguild.github.io",
  base: BASE,
  trailingSlash: "always",
  markdown: { remarkPlugins: [rebase] },
  integrations: [
    starlight({
      title: "Zorvik Docs",
      description: "Developer documentation for Zorvik, the open-source workbench to build, test, mock and load test APIs.",
      logo: { src: "./src/assets/icon.png", alt: "Zorvik" },
      favicon: "/favicon.png",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/LibreGuild/zorvik" }],
      editLink: { baseUrl: "https://github.com/LibreGuild/zorvik/edit/main/docs/pages/" },
      lastUpdated: true,
      customCss: ["./src/styles/docs.css"],
      // The preview says so at the top of every page (above a page's own banner).
      components: NEXT ? { Banner: "./src/components/Banner.astro" } : {},
      head: [
        // Page views only: no cookies, nothing personal (GoatCounter).
        { tag: "script", attrs: { "data-goatcounter": GOATCOUNTER, async: true, src: "https://gc.zgo.at/count.js" } },
        ...(NEXT ? [{ tag: "meta", attrs: { name: "robots", content: "noindex" } }] : []),
      ],
      sidebar: [
        { label: "Overview", link: "/docs/" },
        group("Getting started", "getting-started"),
        group("Requests", "requests"),
        group("Protocols", "protocols"),
        group("Variables & environments", "variables"),
        group("Scripts & tests", "scripting"),
        group("Runner & contract tests", "testing"),
        group("Mock servers & servers", "servers"),
        group("Load testing", "load-testing"),
        group("Network tools", "tools"),
        group("Command line", "cli"),
        group("AI agents", "agents"),
        group("Training Bootcamp", "bootcamp"),
        group("Reference", "reference"),
        group("Help", "help"),
      ],
    }),
  ],
});
