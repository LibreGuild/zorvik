// The Zorvik website (GitHub Pages): a landing page (src/pages/index.astro) and the developer
// docs under /docs (Starlight, src/content/docs/docs).
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

const GOATCOUNTER = "https://zorvik.goatcounter.com/count";

const group = (label, directory) => ({ label, items: [{ autogenerate: { directory: `docs/${directory}` } }] });

export default defineConfig({
  site: "https://libreguild.github.io",
  base: "/zorvik",
  trailingSlash: "always",
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
      head: [
        // Page views only: no cookies, nothing personal (GoatCounter).
        { tag: "script", attrs: { "data-goatcounter": GOATCOUNTER, async: true, src: "https://gc.zgo.at/count.js" } },
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
