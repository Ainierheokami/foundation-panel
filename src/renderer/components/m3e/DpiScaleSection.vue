<template>
  <MSection id="m-desktop-scale" :title="d.title">
    <MBanner v-if="problem" tone="error" icon="error">{{ problem }}</MBanner>
    <MGroup>
      <MSliderItem v-model="pos" icon="format_size" :title="d.scale" :min="0" :max="Math.max(options.length - 1, 0)" :step="1" :decimals="0" size="m" icon-start="text_decrease" icon-end="text_increase" :readout-text="loading ? d.loading : `${dpi}%`" :disabled="applying || loading || options.length < 2" />
      <MItem stack :title="d.presets"><template #control><MChipSet v-model="dpi" :label="d.presets" :options="options.map((v) => ({ value: v, label: `${v}%` }))" :disabled="applying || loading" /></template></MItem>
    </MGroup>
    <MFooter v-if="note">{{ note }}</MFooter>
    <div class="m-dpi__actions"><MButton icon="check" :loading="applying" :disabled="applying || loading || dpi === systemDpi" @click="apply">{{ applying ? d.applying : d.apply }}</MButton></div>
  </MSection>
</template>

<script setup>
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { MSection, MGroup, MItem, MSliderItem, MChipSet, MButton, MFooter, MBanner, useNotify } from 'foundation-ui/vue'
import { useI18n } from '../../desktop/i18n/index.js'

// 桌面缩放: the old DPI 调节 tool on the virtual display page; it scales the display the stream uses. The steps are what Windows lets that display take now without signing out (a phone-sized display stops
// at 200 %): `get_dpi_scale_info` answers them, and they are read again whenever the display or its resolution may have changed (the page gets the focus, a value was applied).
const { t } = useI18n()
const d = computed(() => t.value.m3e.dpi)
const options = ref([]), dpi = ref(100), systemDpi = ref(null), loading = ref(true), applying = ref(false), note = ref(''), problem = ref('')
const pos = computed({
  get: () => Math.max(0, options.value.indexOf(dpi.value)),
  set: (i) => { dpi.value = options.value[i] ?? dpi.value },
})
// a failure stays on the page with the host's own words (could not read the display, not Windows), until the next call succeeds
async function read() {
  try {
    const info = await invoke('get_dpi_scale_info')
    options.value = info.options
    systemDpi.value = info.current
    problem.value = ''
    // a value the display no longer allows (its resolution changed) is not kept in the controls
    if (!info.options.includes(dpi.value)) dpi.value = info.current
  } catch (e) { systemDpi.value = null; problem.value = d.value.readFailed.replace('{error}', String(e)) }
  return systemDpi.value
}
const reread = async () => { if (!applying.value) await read() }
onMounted(async () => {
  const v = await read()
  if (v) dpi.value = v
  loading.value = false
  window.addEventListener('focus', reread)
})
onBeforeUnmount(() => window.removeEventListener('focus', reread))
async function apply() {
  applying.value = true
  note.value = ''
  problem.value = ''
  const wanted = dpi.value
  try {
    const result = await invoke('set_desktop_dpi', { dpi: wanted })
    // never silent: a step the display does not allow without signing out is set to the highest it allows, and the page says so once
    if (result?.clamped) { dpi.value = result.applied; note.value = d.value.limit.replace('{value}', result.max) }
    else useNotify().snack(d.value.applied.replace('{value}', result?.applied ?? wanted))
    // Windows can keep another value; say what it shows after a second, as the tool did
    setTimeout(async () => {
      const now = await read()
      if (now !== null && now !== (result?.applied ?? wanted)) note.value = d.value.partial.replace('{value}', now)
    }, 1000)
  } catch (e) {
    problem.value = d.value.failed.replace('{error}', String(e))
  } finally { applying.value = false }
}
</script>

<style>
.m-dpi__actions { display: flex; gap: 8px; padding: 8px 0 4px; }
</style>
