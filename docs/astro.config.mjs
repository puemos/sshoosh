import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://docs.puemos.com',
  base: '/sshoosh-docs/',
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
