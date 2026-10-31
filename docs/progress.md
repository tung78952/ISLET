# Islet v2 progress (resume point)

Plan: C:\Users\iamtp\.claude\plans\nh-c-v-l-i-v-reactive-possum.md

## Done
- Phase 1: Ghidra decompile of the reference app in D:\islet-re\decomp; numbers in docs/spec.md
  (fonts, spring constants 230/19, 110/15.5, 200/29, 420/41, window 640×190 u).
- Phase 2: captures in D:\islet-re\frames (01_hover … 11_rest), element boxes in D:\islet-re\elements.txt.
  Reference data restored afterwards (backup in D:\islet-re\backup).
- Phase 3/4 (in progress): src/main.rs rewritten (springs, peek→expand on hover, carousel pages
  Music/Pomodoro/Water/Shelf/Todo, lyrics mode + compact lyric pill, dip after collapse, demo mode
  ISLET_DEMO); src/ui.rs rewritten to measured layout.
- Pixel diff loop: tools/diff_all.ps1 (all states), tools/align.py (best per-element shift),
  tools/hotspots.py (worst blocks), tools/zoom.py (side-by-side crops).
  Scores (2026-10-29): music 97.6, pomo 97.4, water 97.4, shelf 97.6, todo 97.2, settings 97.8,
  caps-on toast 98.3, caps-off 97.9, volume toast 97.2, compact 96.3 % (equalizer bars are animated).
  Notch silhouette refit from sub-pixel edges: corner radius 12.4→31.6 u, concave ear arcs 10→22.2 u.
  Small chip/button text in Nusa is Segoe UI 10 (not 11.5). tools/subpix.py gives centroid offsets.
  NEVER edit sources with PowerShell Get/Set-Content (PS 5.1 mangles UTF-8 · and ×).
  Pomo ring: diagonal gradient 0xFD685D→0xF33548 + 3 glow strokes (gfx.stroke_circle_diag).
  Fonts confirmed exact (title/time rows reach 0 error after alignment).

- Smoothness: animation now paced by src/pump.rs (DwmFlush vsync thread → WM_FRAME) instead of an
  8 ms WM_TIMER (which fired at ~15.6 ms with jitter). Measured: 144 fps on the 143 Hz panel,
  median frame 6.9 ms, render 2.7 ms avg; 4 of 437 frames > 10 ms.

- Phase 5 (2026-10-29), all in the real build:
  - Power: RegisterPowerSettingNotification → WM_POWERBROADCAST (no 5 s poll).
  - Fullscreen hide: SetWinEventHook foreground + location change → WM_FOREGROUND (no 1 s poll).
    Verified: a borderless maximized window hides the notch, closing it shows it again.
  - Bluetooth: src/bt.rs AssociationEndpoint watcher (paired classic + LE, IsConnected); toasts on
    connect/disconnect after initial enumeration. Verified enumeration of 9 paired devices; a live
    connect event was not exercised.
  - App notifications: src/notify.rs polls UserNotificationListener every 1.5 s (access = Allowed).
    Not verified end to end: this PC has toasts disabled (PushNotifications\ToastEnabled = 0),
    so the Action Center stays empty.
  - Shelf: real shell icons/thumbnails (IShellItemImageFactory), cached per path; demo key
    shelf=path|path. Virtual-file drops (FileGroupDescriptorW/FileContents, HGLOBAL or IStream).
  - Sounds: src/sound.rs synthesizes Islet's own chimes (soft two-note, alert triple blip).

- Feedback round 2 (2026-10-29):
  - Removed the post-collapse "dip" (it was a misreading; Nusa simply undershoots ~7.5% below
    compact via the 230/19 spring, measured with tools/silhouette.py on 11_rest).
  - Minimal notch is a 44.8×4 u sliver with an invisible 140×18 u hover pad.
  - Compact content, lyric pill and toast crossfade (toast_mix/pill_mix springs) like Nusa.
  - Caps Lock state tracked from the hook's key stream (GetKeyState lagged on fast presses).
  - Equalizer animates continuously (capped ~60 fps); dirty-rect rendering, software D2D target,
    working set trimmed after startup. Measured idle: CPU 0.3 %, WS 9 MB (Nusa: 13 %, 12 MB);
    equalizer running: ~8 % of one core.
  - Lyrics: click a line to seek (Cmd::Seek), lines glide when advancing (lyr_shift), lyric pill
    gets the art-tinted glow; lookups retry "Artist - Title" splits, title-only and free text
    (free text only accepted when the duration matches within 4 s).
  - Shelf: Nusa-style tiles (icon, name, size, hover ×, Clear chip), drag glow trailing the
    cursor, ring burst on drop. Demo keys: shelf=, drag=x,y, minimal=, pet=.

- Feedback round 3 (2026-10-29):
  - ui::btn(): every button eases a hover/press spring (App.hover) and zooms about its center;
    gfx.zoom/unzoom compose with the page-slide offset.
  - Water switch knob/track and the page dots glide (toggle_x, dot_pos springs).
  - Timeline scrubbing (Act::Scrub + App.scrub): bar thickens with a knob on hover, drag to seek.
  - Seek hold in the media thread (2.5 s) stops the position bouncing back after a seek.
  - Lyric pill widens to fit its line (220–560 u) and shows "Loading lyrics...".
  - Drop burst now spreads across the whole panel.
  - Bluetooth/app-notice toasts are two-line 320×56 (badge, bold name, grey status, pulsing
    headphones halo for devices). Toast icons pop and values slide in on entrance.
  - Verified by automated hover captures (play button, chevron, timeline) and pixel diffs
    (all ≥ 97 % except compact 96.2 %).

- Round 4 from the user's phone videos (D:\islet\effectvid, decoded with OpenCV; sheets via
  scratch sheet.py):
  - Toasts replace each other immediately with a ~130 ms crossfade (no queue delay); a changed
    value rolls (Caps On enters from below, Off from above), matching capslockeffect.mp4.
  - Volume comes from IAudioEndpointVolumeCallback (no debounce timer, which froze the bar while
    a key was held); the bar eases toward the level (vol_level spring).
  - Bluetooth: battery from DEVPKEY_Bluetooth_Battery on the device container; "Connected · N%"
    counts up over 0.9 s with a filling ring and ripples, then the ring dims.
  - Water reminder: 372×56 toast with pulsing droplet ring, Snooze (10 min) and Drank buttons that
    work without opening the notch.
  - Music ↔ lyrics view morphs (lyr_mix): art/title/progress/transport glide, header and dots fade,
    lyric column rises in; lyric lines brighten/dim with the scroll.
  - Diff captures must keep the cursor off the notch (hover effects otherwise change pixels).

- Round 5 (effectvid/new + screenshots): lyrics view no longer strands the UI when the player
  closes (lyr_mix needs media; lyric_line None without media); timeline fill eases (tl_frac);
  volume rubber-band at either end (WM_VOLKEY + vol_stretch, sys::volume_state); hovered lyric
  lines brighten, wheel scrolls lyrics (lyr_user, returns after 3 s); faster cascade; fixed-width
  lyric pill; Caps icon crossfades filled/outline and its bar brightens (caps_mix); time-based
  effects render at display rate; hardware Direct2D target again.

- Round 6: wheel scroll bounded with rubber-band overscroll (lyr_bounds); Ctrl+wheel lyric size
  15–26 saved as settings lyric_size; startup drops in and grows from 0.8 (init must kick());
  minimal notch arms only after the first hover/click; snappier open (dwell 170 ms, 190/21) and
  close (300/23); calmer layered equalizer. rec.ps1 gained wheel:n / cwheel:n steps.

- Round 7 (effectvid/new2/moi.mp4, 30 fps camera): Nusa opens in ~4 frames after a ~100 ms dwell,
  closes in ~4 frames, and drops in on start in ~2 frames. Tuned: dwell 90 ms, open 420/32,
  close 520/34, content fade 900/60, intro 900/60 with grow 0.85. Islet recorded at 30 fps now
  matches (open ≈ 4 frames, close ≈ 3–4 frames).

- Round 8 (effectvid/new3, drag & drop): drag fills the drop area with a deep-blue gradient and
  a bloom trailing the cursor; a drop spreads blue light over the whole panel from the drop point
  (drawn behind content) fading over 1.6 s while new tiles fade/rise in. Demo key dropped=x,y.

- Round 8b (user screenshots of Nusa's drop): colour mesh (indigo/blue/teal/violet lights on navy)
  for drag and drop; six rings ripple from the drop point across the panel; empty-state art fades
  under new items; the notch gets a velocity kick on drop (stretch and spring back). The drop
  light is drawn in ui::draw behind the header.

- Round 8c (re-read of new3 frames 164–198 + screenshot): light is a panel "aurora" rising from
  the bottom edge (indigo centre, teal left, violet right, brightest under the cursor), clipped to
  the notch outline via gfx.push_notch_clip (D2D layer with the notch geometry); the drop box is a
  dark see-through frame with existing items faint behind it; a drop sends one bright wave and a
  faint follower across the panel; the aurora fades over ~3 s. Drop kick 460/170 → measured
  +18 px (3.4 %) stretch, then back.

- Round 8d (colour-sampled from the user's Nusa screenshots 26/27): bottom-edge light peaks at
  #304B85 centre, #0A2739 left, #181336 right (gfx.glow_ellipse); drag light starts dim (0.3) and
  builds with cursor movement (drag_energy); drop ripple is a packet of six flat 10 u bands with
  alphas [.04 .11 .23 .15 .07 .03] of #C8CCEB, measured from the screenshot's banding.

- Round 9 (effectvid/new4 are 800×238 screen recordings = pixel-exact references):
  - Pomodoro: compact timer is a sloshing red liquid bubble (icon::liquid, gfx.clip_circle);
    expanded ring shows time left (gfx.arc_diag) opening clockwise from 12; a finished phase no
    longer auto-starts — the 310×61 "Focus Time! / Back to work, 25 min" toast offers ✕ and
    Focus/Break (Act::ToastStartPhase / ToastDismiss), lasting 60 s. Demo keys phase_end=1,
    pomo_run=1, pomo_left=secs.
  - Shelf drag light: AURORA palette sampled from the recording's bottom row (13 stops, seamless
    gfx.fill_rect_hstops + black top fade) — matches within ±3 per channel; drop frame is a grey
    translucent vgrad; Clear chip stays during drag; drop ripple: six soft bands 13 u apart,
    0.3 s, items appear in 0.12 s and the empty frame fades in 0.16 s (frames 263–269).

- Round 10: app notifications fixed (unpackaged senders have no AppInfo — describe() now falls
  back; verified with a real Windows toast) and shown as a ringing-bell two-line toast; tall
  toasts shake (ring_angle), pulse and show a countdown bar; liquid timer redrawn (glass, rim,
  meniscus, highlight) and animated continuously while running; window-wide drop zone while a
  button is held over the 800×238 window (T_DRAGZONE poll + 1.5/255 layer) so files dragged near
  the notch open the shelf; bottom light level follows how close the dragged file is to the
  notch's bottom edge (aurora_lvl) and survives the collapse after a drop, ripples 0.75 s.

- Round 11: notch steps aside to a sliver (App.yielded, poll_yield on the 50 ms timer) on a click
  next to it, pointer work beside it > 0.6 s, typing into a field under it (GetGUIThreadInfo caret,
  else last click position) and window move/size into it (EVENT_SYSTEM_MOVESIZE*); returns 1 s
  after; resting on the sliver reopens. Hover expands only once the cursor rests (< 6 u in 90 ms).
  Drop zone arms only for drags that started outside the window (plain clicks near the notch no
  longer open it). Same-hue bottom-light highlight near the edge (aurora_highlight, level up to
  1.6). Facebook (browser) notifications → iPhone-style ToastKind::Message with drawn Messenger
  icon; message toasts excluded from screen capture (WDA_EXCLUDEFROMCAPTURE); Windows banners
  turned off per sender AUMID (ShowBanner=0, backup %APPDATA%\Isletanners.txt, tray item
  "Hide Windows banners" restores). Test: scratch yieldtest.ps1 (own WinForms window).

- Round 12 (effectvid/new5 + Nusa string table): Facebook messages also detected from Chromium
  tab titles via UI Automation (tab strip only, sys::spawn_tab_watch, WM_TABMSG) because Facebook
  sends no desktop notification while its tab is open; status toasts 312×60 for charging /
  unplugged / low / full (count-up ring + big %), Wi-Fi with SSID, pulsing arcs and signal bars,
  brightness bar via GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS; Nusa's exact strings ("Plug in
  soon", "You can unplug", "5 minute short break", "15 min, you earned it", "Your bottle is
  probably empty"); "Task Time!" reminder at a to-do's start time with Later (10 min) / Start
  (focus session); "Goal reached" water toast. Demo keys chg_toast=, unplugged=, wifi_on=,
  wifi_off=, bright=, task=.

- Round 13 (smoothness + unified cards): software D2D render target (hardware readback stalled
  up to 31 ms; software p99 6.4 ms, max 8.8 ms); shell icons load on a worker thread (WM_ICON;
  removed a 135 ms hitch); no EmptyWorkingSet; cached radial glow brushes and text widths;
  UIA focused-field lookup moved to a background thread (spawn_focus_watch); partial redraw of
  the right indicator (loop_rect). Brightness bar slides from the previous level. Tab watcher is
  event driven (MSAA EVENT_OBJECT_NAMECHANGE on Chrome_WidgetWin_1, no polling). Tray item "Show
  message content". All status toasts share ui::card (Bluetooth layout: badge, title/sub, ring
  with ripples; Wi-Fi bars appear one by one, centred glyph). To-do rows: title and time on
  separate lines, "-00:00" end hidden. Demo keys goal=, todos=title@HH:MM-HH:MM~title.

- Round 14 (level icons + notification detail): sun rays lengthen/brighten with the eased level
  and turn ~1.15° per percent (sun_rot); speaker waves appear per third of the range, cross at
  0/mute. Brightness rubber band plays once when the bar arrives at 0/100 (bright_bounce): the
  laptop reports no further Fn presses at the ends (checked with power broadcasts, the keyboard
  hook and WmiMonitorBrightnessEvent). Notifications keep a NoticeInfo (kind Chat/Browser/
  Screenshot/App, site, Windows id); resting the cursor on (or clicking) a Notice/Message toast
  grows it to NOTICE_OPEN 420x160 with source row, title, wrapped body or a screenshot thumbnail
  (newest file in FOLDERID_Screenshots, worker thread, WM_SHOT) and Open / Close (Close also
  removes it from the Action Center). shell_image returns the thumbnail's real size; shelf draws
  with image_fit. Demo keys notice=App|Title|Body|site, notice_open=1, shot=<image>, sun=.

- Round 15 (idle cost, measured per named thread over 60 s): idle CPU 1.0-1.8 % -> 0.36-0.39 % of
  one core (Nusa 0.26-1.67 % in the same conditions); private memory 12 MB (Nusa ~66 MB).
  Changes: notification list read only when %LOCALAPPDATA%\Microsoft\Windows\Notifications
  changes (FindFirstChangeNotification, 15 s safety) instead of every 500 ms; media via SMTC
  change events (5 s safety read); network via NetworkStatusChanged; tab watcher remembers each
  element's role so page renames skip the cross-process calls; no EVENT_OBJECT_LOCATIONCHANGE hook
  (fullscreen checked in the tick); idle tick redraws only when the clock minute changes;
  pointer/yield poll 50 ms near the notch or while typing, else 250 ms; focus watch queries UIA
  only without a Win32 caret in a window reaching the top. Notifications arriving while the
  widgets are open are held and shown on close; while yielded or hidden they wait. Threads are
  named (islet-*) for profiling.

- Round 16 (devices, privacy, storm test): capture exclusion only for Message cards, held 300 ms
  after the last one (it had covered every notification and flickered the window on each
  switch); notification list re-read 300 ms after a store change. Message card centred (its
  slide-in ended 10 u high). Bluetooth cards show the device kind from
  DEVPKEY_DeviceContainer_Category of the container's device nodes (headphones, headset,
  speaker, mouse, keyboard, gamepad, phone) and "Low battery" in red at <= 20 %. New
  devices.rs: WM_DEVICECHANGE for keyboard/mouse/WPD interfaces (receiver interfaces merged per
  container, built-in devices of the PC's own container ignored, Bluetooth skipped) and volumes
  (drive card with Open); IMMNotificationClient for headphone jacks/USB audio (Bluetooth
  endpoints skipped) and to follow the default output's volume. ToastKind::Device. Demo keys
  bt_kind=, bt_batt=, device=kind|Name|1|detail|E:. storm.rs: storm=<s>;seed=;stormlog= drives
  real handlers with random events and checks invariants; 3x120 s runs pass (render p99 ~2.8
  ms). One earlier run froze mid-animation once (not reproduced); the tick now restarts the
  frame pump after 400 ms without frames and the storm report counts restarts (0 since).

- Round 17 (device queue, battery colours): Bluetooth/Device cards go through present_device;
  one arriving while another is under 1.5 s old (or while the widgets are open) waits in
  device_queue (latest state per name, at most 4) and plays 1.5 s each from the tick. A
  different device crossfades as a new card instead of updating the old one. battery_tone:
  > 50 % green, 21-50 % amber, <= 20 % red, applied to Bluetooth cards with a battery and to
  "Charger unplugged" (follows the counting-up level). Storm gains a 3-device burst and a check
  that no device card is left unshown; 2x120 s pass.

- Round 18 (Luma merge, step 1: the brain): src/luma/ carries Luma's config, scheduler, smart
  and focus logic unchanged (26 of its tests pass inside Islet), plus Brain, which runs Luma's
  ticker every 2 s from Islet's tick. Data in %APPDATA%\Islet\luma (copied once from
  %APPDATA%\com.pause.desktop, which is never changed). Water page = Luma cups x cup size
  (config cupMl/waterGoal added), next from Luma's schedule, chart from today's answers; water
  settings: cup, goal, learned day start to bedtime (settable). Pomodoro page = Luma focus /
  Pomodoro (rounds, long rest, phase notices with Skip/Pause, time's-up card with +5 min). To-do
  = today's Luma reminders (timed, "HH:MM-HH:MM" starts focus) + untimed to-dos; Islet's timed
  to-dos moved over (todo.txt.bak), water.txt retired (water.txt.bak). New cards: stretch, eyes,
  breathe, bedtime (ToastKind::Remind), heads-up countdown, Luma-app takeover. Cards wait for an
  answer (45 s timeout = Luma's snooze; strict hides Later for 10 s) and return after other
  toasts. While the Luma app is running or its task is enabled, Islet's reminders stay off until
  "Turn off" (closes pause.exe, disables the "Luma" task). ISLET_FAST=1: 1 minute = 2 s, muted.
  Fixed: long-lived toasts redrew at 60 fps (15 % CPU) - arrival animations now stop after 3-5 s
  (waiting card 0.05 %, idle 0.13 %). Storm covers Luma reminders/answers/focus; 4 seeds pass.

- Round 19 (bug pass): foreground_covers ignores shell overlays (capture layer, Win+Tab, Start,
  panel) and topmost windows; Gfx::invalidate forces a whole frame after show/display change and
  once a minute (screen check over 7 captures: notch 88 % black throughout). Sliver: brushing does
  nothing, back on leaving 1 s or a click, 2 s no-hover after. Notification lag was the 15 s safety
  read (folder change events don't fire for wpndatabase): now polls the db-shm change counter every
  200 ms, 20 test toasts avg 197 ms, max 288 ms (was 6-19 s). media.rs follows all sessions via
  choose(); a same-session clip is held 15 s. First notification no longer removed. Phone Link
  parsed as app/sender/content with Phone Link's icons, brand or icon colour, exact-message link
  from a private copy of wpndatabase (rusqlite). Toasts stay 6-12 s by length; memory-only history
  of 50 with a bell. Audio endpoints: flags masked, only real active changes count, cards held 2 s
  and dropped if Bluetooth reported. Call card (ToastKind::Call, wording-based detection, unverified).
  ISLET_TIMELOG is temporary, remove after the wired-headphone test.

- Round 20 (phone first): Chrome tab watcher removed. bt::phone_connected() (a connected Bluetooth
  device of kind Phone, 60 s grace); while true, browsers and PC apps learned from Phone Link
  (phone_apps.txt) stay quiet (notify::phone_covers). Clicking brings up the app's or the site's
  open window (sys::open_app/open_site), else launches it. Branch islet-classic and tag
  islet-classic-v1 at 4ae491f; master fast-forwarded to the Luma merge.
- Round 21: a notification's card and detail view crossfade by share of the growth (card out in
  the first half, detail in the second), so a taller phone card no longer shows both. The "Show
  message content" setting is gone: hovering showed the text anyway.

- Round 22: eyes in the real notch (src/eyes.rs): lens outlines traced from design sheets, every
  expression the same point count so they morph on springs. Mind wakes on a pointer arriving
  (170 u from the notch), yields to step-aside, clicks, typing, cards and the open panel, tires
  after 3 s still or 12 s moving. Eyes replace the lyric pill for a moment. Typing now steps the
  notch aside only when the field touches the notch itself (no 24 u margin). Water card uses the
  animated glass (src/anim.rs). Old cat mascot removed. Waiting: peeks on events, expression
  wiring, bell on real cards, eight more icons (need images), character choice, eye CPU on its
  own, moving the Spider-Man lenses out of the code before any release, removing ISLET_TIMELOG.

- Round 23: the eyes come out on their own (Mind Peek): random every 15-40 min, a look before a
  reminder card, nodding to music (loudness meter, read only while nodding, with notes in a wider
  notch), a battery scene at 10 % (the eyes butt the battery), love, sad and cry on events.
  An event waits 10 s for a free moment. Task and heads-up cards ring the new bell. Waiting: eight
  icons (need images), character choice, Spider-Man lenses out of the code before any release.

- Round 24: nodding eyes and notes removed. A new song passes by as a card (cover, title, artist).
  Small eyes (eyes::Mini) head the lyric pill: they read, blink, nod to the beat and perk up at
  each line; the meter runs only while the pill shows. Banners are hidden per phone app.

- Round 25: props replace the drawn icons (assets/props, cut from effectvid/design/props.png):
  white pictures, one part tinted with CHARACTER, moved by code beside the small eyes. The lyric
  bar grows 90 u to one side every 1-2 min for 6 s, with the small eyes dancing to the beat and
  notes; the lyrics stay put. The equalizer rides through pauses under 0.5 s.

- Round 26: the main reminders are acts (eyes::act): the eyes wind up and hit a prop, which pushes
  the words out; then each does its own business (water sloshes, the moon rises, a breath swells
  notch and eyes, the eyes pull the notch apart). The cards wait quietly and nudge every 15 s.
  Other cards are plain with a bell, and their words slide in. Only looks, positions and timing
  are used, so another character can perform the same acts.

## Next
1. Remaining diff cases: water sub2 (chart), todo add form/edit, lyrics view (needs same lyric text).
2. Bluetooth/notice toast icons (currently a letter badge), mascot reactions.
3. Phase 6: final report with side-by-side gallery.

## Diff command example
powershell -File D:\islet\tools\islet_shot.ps1 -Spec "page=music;expanded=1;charging=1;clock=20:06;title=siopaolo - better for you [visualizer];artist=siopaolo;art=D:\islet-re\art.png;pos=100;dur=290;playing=1" -Out D:\islet-re\cmp\music.png
python -I D:\islet\tools\diff.py D:\islet-re\frames\01_hover\0233.png D:\islet-re\frames\03_pages\p0.png D:\islet-re\cmp\music.png D:\islet-re\cmp\music "138,0,662,175,35"
