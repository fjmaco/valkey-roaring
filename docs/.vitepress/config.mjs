import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { defineConfig } from "vitepress";

// Project pages URL: https://fjmaco.github.io/valkey-roaring/
const BASE_URL = "/valkey-roaring/";

const commandsDir = path.join(
  path.dirname(fileURLToPath(import.meta.url)), "..", "commands");

function commandSidebar() {
  return readdirSync(commandsDir)
    .filter((f) => f.endsWith(".md") && f !== "index.md")
    .sort()
    .map((f) => {
      const name = f.replace(/\.md$/, "");
      return { text: name.toUpperCase(), link: `/commands/${name}` };
    });
}

export default defineConfig({
  title: "Valkey Roaring",
  lang: "en-US",
  description: "Roaring Bitmaps for Valkey and Redis",
  base: BASE_URL,
  cleanUrls: true,
  themeConfig: {
    search: { provider: "local" },
    nav: [
      { text: "Guide", link: "/guide/what-is-roaring-bitmap" },
      { text: "Commands", link: "/commands/" },
      { text: "Docker Hub", link: "https://hub.docker.com/r/fjmaco/valkey-roaring" },
    ],
    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "What Is a Roaring Bitmap", link: "/guide/what-is-roaring-bitmap" },
          { text: "Getting Started", link: "/guide/getting-started" },
          { text: "Export / Import", link: "/guide/export-import" },
          { text: "Persistence & Replication", link: "/guide/persistence-and-replication" },
          { text: "Performance", link: "/guide/performance" },
        ],
      },
      {
        text: "Commands",
        collapsed: false,
        items: [{ text: "Overview", link: "/commands/" }, ...commandSidebar()],
      },
    ],
    socialLinks: [
      { icon: "github", link: "https://github.com/fjmaco/valkey-roaring" },
    ],
    footer: {
      message: "Based on redis-roaring by Antonio Viggiano and contributors.",
    },
    outline: "deep",
  },
})
