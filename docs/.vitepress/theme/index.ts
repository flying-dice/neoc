// https://vitepress.dev/guide/custom-theme
import { h, nextTick, watch } from 'vue'
import DefaultTheme from 'vitepress/theme'
import { useData, Theme } from 'vitepress'
import { createMermaidRenderer } from 'vitepress-mermaid-renderer'
import HomeHeroExtra from './components/HomeHeroExtra.vue'
import './style.css'


export default {
  extends: DefaultTheme,
  Layout: () => {
    const { isDark } = useData()

    const renderMermaid = () => {
      createMermaidRenderer({
        theme: isDark.value ? 'dark' : 'forest',
      })
    }

    nextTick(renderMermaid)
    watch(() => isDark.value, renderMermaid)

    return h(DefaultTheme.Layout, null, {
      'home-hero-after': () => h(HomeHeroExtra),
    })
  },
  enhanceApp({ app, router, siteData }) {
    // ...
  }
} satisfies Theme
