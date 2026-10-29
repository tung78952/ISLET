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

## Next
1. Remaining diff cases: water sub2 (chart), todo add form/edit, lyrics view (needs same lyric text).
2. Bluetooth/notice toast icons (currently a letter badge), mascot reactions.
3. Phase 6: final report with side-by-side gallery.

## Diff command example
powershell -File D:\islet\tools\islet_shot.ps1 -Spec "page=music;expanded=1;charging=1;clock=20:06;title=siopaolo - better for you [visualizer];artist=siopaolo;art=D:\islet-re\art.png;pos=100;dur=290;playing=1" -Out D:\islet-re\cmp\music.png
python -I D:\islet\tools\diff.py D:\islet-re\frames\01_hover\0233.png D:\islet-re\frames\03_pages\p0.png D:\islet-re\cmp\music.png D:\islet-re\cmp\music "138,0,662,175,35"
