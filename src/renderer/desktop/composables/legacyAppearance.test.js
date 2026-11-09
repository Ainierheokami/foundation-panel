import test from 'node:test'
import assert from 'node:assert/strict'
import { applyLegacyAppearance, legacyAppearanceMode } from './legacyAppearance.js'

function storage(values = {}) {
  return {
    getItem: (key) => values[key] ?? null,
    setItem: (key, value) => { values[key] = value },
  }
}

function root() {
  const attrs = {}
  return { attrs, style: {}, setAttribute: (key, value) => { attrs[key] = value } }
}

test('reads the old light, dark, and system choices', () => {
  assert.equal(legacyAppearanceMode(storage({ 'sunshine-theme': 'light' })), 'light')
  assert.equal(legacyAppearanceMode(storage({ 'sunshine-theme': 'dark' })), 'dark')
  assert.equal(legacyAppearanceMode(storage({ 'sunshine-theme': 'system' })), 'system')
  assert.equal(legacyAppearanceMode(storage({ 'sunshine-theme': 'invalid' })), 'system')
})

test('applies the old choice to a new desktop window', () => {
  const dark = root()
  const darkStorage = storage({ 'sunshine-theme': 'dark' })
  applyLegacyAppearance(dark, darkStorage, false)
  assert.equal(dark.attrs['data-bs-theme'], 'dark')
  assert.equal(dark.style.colorScheme, 'dark')
  assert.deepEqual(JSON.parse(darkStorage.getItem('foundation-ui.appearance')), { mode: 'dark' })

  const light = root()
  const lightStorage = storage({ 'sunshine-theme': 'light', 'foundation-ui.appearance': '{"mode":"dark"}' })
  applyLegacyAppearance(light, lightStorage, true)
  assert.equal(light.attrs['data-bs-theme'], 'dark')
  assert.equal(light.style.colorScheme, 'dark')
  assert.equal(lightStorage.getItem('foundation-ui.appearance'), '{"mode":"dark"}')

  const system = root()
  applyLegacyAppearance(system, storage({ 'sunshine-theme': 'system' }), true)
  assert.equal(system.attrs['data-bs-theme'], 'dark')
})
