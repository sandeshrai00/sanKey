# IOHook Keycodes Reference

Complete mapping of IOHook numeric keycodes → W3C Key Names.

This table is the authoritative reference used by `v1-to-v2-converter.py` and `sora-pack-import.py`, sourced from `mechvibes-dx-new`'s `old_pack_fixer.rs`.

---

## Basic Keys

| Code | Key Name | Notes |
|------|----------|-------|
| 1 | `Escape` | ESC key |
| 2 | `Digit1` | `1` on top row |
| 3 | `Digit2` | `2` on top row |
| 4 | `Digit3` | `3` on top row |
| 5 | `Digit4` | `4` on top row |
| 6 | `Digit5` | `5` on top row |
| 7 | `Digit6` | `6` on top row |
| 8 | `Digit7` | `7` on top row |
| 9 | `Digit8` | `8` on top row |
| 10 | `Digit9` | `9` on top row |
| 11 | `Digit0` | `0` on top row |
| 12 | `Minus` | `-` |
| 13 | `Equal` | `=` |
| 14 | `Backspace` | Backspace / Delete |
| 15 | `Tab` | Tab |
| 28 | `Enter` | Enter / Return |
| 41 | `Backquote` | `` ` `` / Tilde |
| 39 | `Semicolon` | `;` |
| 40 | `Quote` | `'` |
| 43 | `Backslash` | `\` |
| 51 | `Comma` | `,` |
| 52 | `Period` | `.` |
| 53 | `Slash` | `/` |
| 57 | `Space` | Space bar |

---

## Letters

| Code | Key Name |
|------|----------|
| 16 | `KeyQ` |
| 17 | `KeyW` |
| 18 | `KeyE` |
| 19 | `KeyR` |
| 20 | `KeyT` |
| 21 | `KeyY` |
| 22 | `KeyU` |
| 23 | `KeyI` |
| 24 | `KeyO` |
| 25 | `KeyP` |
| 30 | `KeyA` |
| 31 | `KeyS` |
| 32 | `KeyD` |
| 33 | `KeyF` |
| 34 | `KeyG` |
| 35 | `KeyH` |
| 36 | `KeyJ` |
| 37 | `KeyK` |
| 38 | `KeyL` |
| 44 | `KeyZ` |
| 45 | `KeyX` |
| 46 | `KeyC` |
| 47 | `KeyV` |
| 48 | `KeyB` |
| 49 | `KeyN` |
| 50 | `KeyM` |

---

## Modifiers

| Code | Key Name |
|------|----------|
| 29 | `ControlLeft` | Left Control |
| 42 | `ShiftLeft` | Left Shift |
| 56 | `AltLeft` | Left Alt / Alt |
| 54 | `ShiftRight` | Right Shift |
| 58 | `CapsLock` | Caps Lock |

Right-side modifier codes vary by implementation:
| Code | Maps To |
|------|---------|
| 3597 | `ControlRight` (standard iohook) |
| 57400 | `AltRight` |
| 57435 | `MetaLeft` |
| 57436 | `MetaRight` |

---

## Function Keys F1–F12

| Code | Key Name |
|------|----------|
| 59 | `F1` |
| 60 | `F2` |
| 61 | `F3` |
| 62 | `F4` |
| 63 | `F5` |
| 64 | `F6` |
| 65 | `F7` |
| 66 | `F8` |
| 67 | `F9` |
| 68 | `F10` |
| 87 | `F11` |
| 88 | `F12` |

---

## Navigation / Arrow Keys

| Code | Key Name |
|------|----------|
| 57415 | `Home` |
| 57416 | `ArrowUp` |
| 57417 | `PageUp` |
| 57419 | `ArrowLeft` |
| 57421 | `ArrowRight` |
| 57423 | `End` |
| 57424 | `ArrowDown` |
| 57425 | `PageDown` |
| 57426 | `Insert` |
| 57427 | `Delete` |

---

## Numpad

| Code | Key Name |
|------|----------|
| 3637 | `NumpadDivide` | `/` (numpad) |
| 3612 | `NumpadEnter` | Enter (numpad) |
| 3597 | `ControlRight` | Right Control |
| 71 | `Numpad7` |
| 72 | `Numpad8` |
| 73 | `Numpad9` |
| 74 | `NumpadSubtract` | `-` (numpad) |
| 75 | `Numpad4` |
| 76 | `Numpad5` |
| 77 | `Numpad6` |
| 78 | `NumpadAdd` | `+` (numpad) |
| 79 | `Numpad1` |
| 80 | `Numpad2` |
| 81 | `Numpad3` |
| 82 | `Numpad0` |
| 83 | `NumpadDecimal` | `.` (numpad) |
| 55 | `NumpadMultiply` | `*` (numpad) |

---

## Other Keys

| Code | Key Name |
|------|----------|
| 26 | `BracketLeft` | `[` |
| 27 | `BracketRight` | `]` |
| 69 | `NumLock` |
| 70 | `ScrollLock` |
| 91 | `F13` |
| 92 | `F14` |
| 93 | `F15` |
| 99 | `F16` |
| 100 | `F17` |
| 101 | `F18` |
| 102 | `F19` |
| 103 | `F20` |
| 104 | `F21` |
| 105 | `F22` |
| 106 | `F23` |
| 107 | `F24` |
| 112 | `Convert` |
| 115 | `Lang1` |
| 119 | `Lang2` |
| 121 | `KanaMode` |
| 123 | `HiraganaKatakana` |
| 125 | `IntlYen` |
| 126 | `NumpadComma` |
| 57399 | `PrintScreen` |
| 57437 | `ContextMenu` |
| 57438 | `Power` |
| 57439 | `Sleep` |
| 57443 | `WakeUp` |

---

## Media Keys

| Code | Key Name |
|------|----------|
| 57360 | `MediaTrackPrevious` |
| 57369 | `MediaTrackNext` |
| 57376 | `AudioVolumeMute` |
| 57377 | `LaunchApp2` |
| 57378 | `MediaPlayPause` |
| 57380 | `MediaStop` |
| 57390 | `AudioVolumeDown` |
| 57392 | `AudioVolumeUp` |
| 57394 | `BrowserHome` |
| 57404 | `LaunchApp1` |
| 57444 | `LaunchApp3` |
| 57445 | `BrowserSearch` |
| 57446 | `BrowserFavorites` |
| 57447 | `BrowserRefresh` |
| 57448 | `BrowserStop` |
| 57449 | `BrowserForward` |
| 57450 | `BrowserBack` |
| 57452 | `LaunchMail` |
| 57453 | `MediaSelect` |
