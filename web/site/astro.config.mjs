import { defineConfig } from "astro/config";
import tailwindcss from "@tailwindcss/vite";
import mdx from "@astrojs/mdx";

// https://astro.build/config
//
// GitHub Pages publishes this repo at https://vivekpal1.github.io/kdo/.
// `site` + `base` together make Astro generate correct asset URLs.
// If we move to a custom domain later, set `site` to that and drop `base`.
export default defineConfig({
  site: "https://vivekpal1.github.io",
  base: "/kdo",
  integrations: [mdx()],
  markdown: {
    shikiConfig: {
      theme: "github-light",
      wrap: true,
    },
  },
  vite: {
    plugins: [tailwindcss()],
  },
});
