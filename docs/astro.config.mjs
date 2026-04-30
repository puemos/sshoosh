import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://docs.puemos.com',
  base: '/sshoosh-docs/',
  build: {
    format: 'directory',
    assets: 'assets'
  }
});
