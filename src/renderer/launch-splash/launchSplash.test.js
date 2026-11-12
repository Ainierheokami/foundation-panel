// The launch splash window (src-tauri/src/launch_splash): what the page does with the frames Rust sends, and what the window promises the game it covers.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8')
const page = read('./main.js'), html = read('./index.html'), vite = read('../../../vite.config.js')
const rust = read('../../../src-tauri/src/launch_splash/mod.rs'), main = read('../../../src-tauri/src/main.rs'), events = read('../../../src-tauri/src/tray/events.rs')

test('the page draws MLaunchSplash from the frames of Rust, keyed by launch, and asks for the current frame once it listens', () => {
  assert.match(page, /listen\('launch-splash:frame'/)
  assert.match(page, /listen\('launch-splash:leave'/)
  assert.match(page, /invoke\('launch_splash_frame'\)/)
  assert.match(page, /h\(MLaunchSplash, \{ key: frame\.value\.id,/, 'a new launch remounts (fades in); the same launch keeps its bar running')
  assert.match(page, /leaving: leaving\.value/)
  assert.match(page, /window\.__launchSplash = \{ set, leave, present \}/)
  const listenAt = page.indexOf("listen('launch-splash:frame'"), askAt = page.indexOf("invoke('launch_splash_frame')")
  assert.ok(listenAt > 0 && askAt > listenAt, 'listen first, then ask: a frame sent in between is not lost')
})

test('the page is its own entry with a transparent body, and the window is built from it', () => {
  assert.match(html, /background: transparent/)
  assert.match(vite, /launchSplash: resolve\(rendererSrcPath, 'launch-splash\/index\.html'\)/)
  assert.match(rust, /WebviewUrl::App\("launch-splash\/index\.html"\.into\(\)\)/)
})

test('the window is transparent, always on top, out of the taskbar, not focusable and click-through, and it is shown without taking focus', () => {
  const builder = rust.slice(rust.indexOf('WebviewWindowBuilder::new'), rust.indexOf('.build()'))
  for (const call of ['.decorations(false)', '.transparent(true)', '.always_on_top(true)', '.skip_taskbar(true)', '.focused(false)', '.focusable(false)', '.visible(false)']) assert.ok(builder.includes(call), call)
  assert.match(rust, /set_ignore_cursor_events\(true\)/)
  assert.match(rust, /SetWindowPos\(hwnd, Some\(HWND_TOPMOST\), x, y, width, height, flags\)/)
  assert.match(rust, /SWP_NOACTIVATE \| SWP_SHOWWINDOW/)
  assert.doesNotMatch(rust, /set_focus|SetForegroundWindow|SW_SHOW\b|\.show\(\)\s*;\s*\n\s*\}\s*\n\s*#\[cfg\(target_os = "windows"\)\]/, 'it never takes focus; the non-Windows build only shows')
})

test('the splash follows the host: the tray state feeds it, a lost Core takes it down, and its command is registered', () => {
  assert.match(events, /crate::launch_splash::on_tray_state\(app, &state\);/)
  assert.match(events, /crate::launch_splash::on_core_disconnected\(app\);/)
  assert.match(main, /^mod launch_splash;$/m)
  assert.match(main, /launch_splash::launch_splash_frame,/)
  assert.match(rust, /const CAPABILITY: &str = "launch-splash-v1";/, 'a host without the capability never starts it')
})

test('the game is never covered for long: the plan ends on the window, the host, the deadline and a lost Core', () => {
  const plan = read('../../../src-tauri/src/launch_splash/plan.rs')
  for (const rule of ['now_ms >= current.deadline_ms', 'window_found && current.stage != "failed"', 'pub fn on_disconnect', 'if !state.active']) assert.ok(plan.includes(rule), rule)
})

test('the splash comes before the game: the window is readied on the announcement, shown when the display is ready, and the host is told once a frame is painted', () => {
  const plan = read('../../../src-tauri/src/launch_splash/plan.rs'), sunshine = read('../../../src-tauri/src/sunshine.rs')
  // the plan readies (Prepare) while display_ready is false and shows when it turns true
  assert.match(plan, /Prepare\(Frame\)/)
  assert.match(plan, /if !state\.display_ready \{/)
  // an older host without the field has no such phase
  assert.match(sunshine, /#\[serde\(default = "launch_display_ready_default"\)\]/)
  // the page reports ready after its picture is decoded and painted after two animation frames of a present
  assert.match(page, /await img\.decode\(\)/)
  assert.match(page, /invoke\('launch_splash_prepared', \{ id: next\.id \}\)/)
  assert.match(page, /listen\('launch-splash:present'/)
  assert.match(page, /requestAnimationFrame\(\(\) => requestAnimationFrame\(\(\) => \{ invoke\('launch_splash_painted'/, 'two frames: the first one after the show is painted')
  // Rust emits present only after SetWindowPos succeeded, answers the host from the painted report, and the events ask the host for the answer channel
  assert.match(rust, /emit_to\(WINDOW_LABEL, PRESENT_EVENT, id\)/)
  assert.match(rust, /post_launch_splash_shown\(&id, panel_ms, prepare_ms\)/)
  assert.match(sunshine, /\?launch_splash_ack=1/)
  assert.match(main, /launch_splash::launch_splash_prepared,/)
  assert.match(main, /launch_splash::launch_splash_painted,/)
})

test('a stage change does not send the picture again: it is megabytes', () => {
  assert.match(rust, /event\.art = String::new\(\)/)
  assert.match(page, /if \(!next\.art && frame\.value && frame\.value\.id === next\.id\)/, 'the page keeps the picture it has')
})
