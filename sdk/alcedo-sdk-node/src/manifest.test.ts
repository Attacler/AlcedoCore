import { describe, it, expect } from 'vitest'
import { PluginManifestSchema } from './zod-schemas.js'
import type { z } from 'zod'

const validManifest = {
  manifestVersion: 1,
  pluginSlug: 'hello-world',
  pages: [
    { path: '/index', label: 'Home', icon: 'pi pi-home', sidebar: true },
  ],
  views: [
    { path: '/index', component: './views/default.vue' },
  ],
  inputWidgets: [
    { type: 'star-rating', label: 'Star Rating', supportedFieldTypes: ['int'], component: './widgets/StarRating.vue' },
  ],
  displayComponents: [
    { type: 'color-swatch', label: 'Color Swatch', supportedFieldTypes: ['string'], component: './displays/ColorSwatch.vue' },
  ],
  viewTypes: [
    { type: 'calendar', label: 'Calendar View', icon: 'pi pi-calendar', component: './views/CalendarView.vue' },
  ],
}

describe('PluginManifestSchema', () => {
  describe('valid manifests', () => {
    it('accepts a valid manifest with all fields', () => {
      const result = PluginManifestSchema.safeParse(validManifest)
      expect(result.success).toBe(true)
    })

    it('accepts a minimal manifest (only required fields)', () => {
      const minimal = {
        manifestVersion: 1,
        pluginSlug: 'hello-world',
      }
      const result = PluginManifestSchema.safeParse(minimal)
      expect(result.success).toBe(true)
    })

    it('accepts manifestVersion as a positive integer', () => {
      const v2 = { ...validManifest, manifestVersion: 2 }
      const result = PluginManifestSchema.safeParse(v2)
      expect(result.success).toBe(true)
    })
  })

  describe('error messages on invalid manifests', () => {
    it('rejects missing manifestVersion with descriptive error', () => {
      const { manifestVersion: _, ...noVersion } = validManifest
      const result = PluginManifestSchema.safeParse(noVersion)
      expect(result.success).toBe(false)
      expect(result.error?.issues.some((i: { path: (string | number)[] }) => i.path.includes('manifestVersion'))).toBe(true)
    })

    it('rejects string manifestVersion with type error', () => {
      const result = PluginManifestSchema.safeParse({ ...validManifest, manifestVersion: 'one' })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some((i: { path: (string | number)[] }) => i.path.includes('manifestVersion'))).toBe(true)
    })

    it('rejects zero manifestVersion (not positive)', () => {
      const result = PluginManifestSchema.safeParse({ ...validManifest, manifestVersion: 0 })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some((i: { path: (string | number)[] }) => i.path.includes('manifestVersion'))).toBe(true)
    })

    it('rejects missing pluginSlug with descriptive error', () => {
      const { pluginSlug: _, ...noSlug } = validManifest
      const result = PluginManifestSchema.safeParse(noSlug)
      expect(result.success).toBe(false)
      expect(result.error?.issues.some((i: { path: (string | number)[] }) => i.path.includes('pluginSlug'))).toBe(true)
    })

    it('rejects empty pluginSlug with descriptive error', () => {
      const result = PluginManifestSchema.safeParse({ ...validManifest, pluginSlug: '' })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some((i: { path: (string | number)[] }) => i.path.includes('pluginSlug'))).toBe(true)
    })

    it('rejects input widget with empty type field', () => {
      const result = PluginManifestSchema.safeParse({
        ...validManifest,
        inputWidgets: [{ type: '', label: 'Bad', supportedFieldTypes: ['int'], component: './bad.vue' }],
      })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some(i =>
        i.path.includes('inputWidgets') && i.path.includes('type')
      )).toBe(true)
    })

    it('rejects input widget with empty component path', () => {
      const result = PluginManifestSchema.safeParse({
        ...validManifest,
        inputWidgets: [{ type: 'bad', label: 'Bad', supportedFieldTypes: ['int'], component: '' }],
      })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some(i =>
        i.path.includes('inputWidgets') && i.path.includes('component')
      )).toBe(true)
    })

    it('rejects view type with missing label', () => {
      const { label: _, ...noLabel } = validManifest.viewTypes[0]
      const result = PluginManifestSchema.safeParse({
        ...validManifest,
        viewTypes: [noLabel],
      })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some(i =>
        i.path.includes('viewTypes') && i.path.includes('label')
      )).toBe(true)
    })

    it('rejects input widget with empty supportedFieldTypes array', () => {
      const result = PluginManifestSchema.safeParse({
        ...validManifest,
        inputWidgets: [{ type: 'bad', label: 'Bad', supportedFieldTypes: [], component: './bad.vue' }],
      })
      expect(result.success).toBe(false)
      expect(result.error?.issues.some(i =>
        i.path.includes('inputWidgets') && i.path.includes('supportedFieldTypes')
      )).toBe(true)
    })

    it('strips unknown top-level fields silently', () => {
      const withUnknown = { ...validManifest, unknownField: 'should-be-stripped' }
      const result = PluginManifestSchema.safeParse(withUnknown)
      expect(result.success).toBe(true)
      expect(result.data).not.toHaveProperty('unknownField')
    })
  })
})
