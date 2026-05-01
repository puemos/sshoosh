import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://puemos.github.io',
  base: '/sshoosh/',
  markdown: {
    shikiConfig: {
      themes: {
        light: 'github-light',
        dark: 'github-dark',
      },
      defaultColor: false,
    },
  },
  build: {
    format: 'directory',
    assets: 'assets'
  }
});
