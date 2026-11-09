// Authority: [panel-whatwg-webstorage], same-origin localStorage is
// shared by the Panel windows and is the old preference's storage contract.
const LEGACY_THEME_KEY = 'sunshine-theme'
const APPEARANCE_KEY = 'foundation-ui.appearance'
const MODES = new Set(['system', 'light', 'dark'])

/** Read the theme choice written by the pre-desktop Panel. */
export function legacyAppearanceMode(storage = globalThis.localStorage) {
  try {
    const value = storage?.getItem(LEGACY_THEME_KEY)
    return MODES.has(value) ? value : 'system'
  } catch {
    return 'system'
  }
}

/** Apply the old choice to a window that has not created the new appearance record yet. */
export function applyLegacyAppearance(root, storage = globalThis.localStorage, prefersDark = false) {
  let mode = legacyAppearanceMode(storage)
  try {
    const current = JSON.parse(storage?.getItem(APPEARANCE_KEY) || 'null')
    if (MODES.has(current?.mode)) {
      mode = current.mode
    } else if (mode !== 'system') {
      storage.setItem(APPEARANCE_KEY, JSON.stringify({ mode }))
    }
  } catch {
    // A blocked storage area still gets the correct first paint below.
  }
  const dark = mode === 'system' ? prefersDark : mode === 'dark'
  root?.setAttribute('data-bs-theme', dark ? 'dark' : 'light')
  if (root) root.style.colorScheme = dark ? 'dark' : 'light'
  return mode
}
