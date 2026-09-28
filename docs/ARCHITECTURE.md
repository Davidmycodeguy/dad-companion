# Architecture

A fast, private Windows companion for Dark and Darker: market research, in-game item values on
hover, an auto lister with merchant selling, stash sorting and quests. Everything runs on the
player's PC; the only network traffic the app makes is the update check against this repository's
GitHub releases. The game's own traffic is only read, never sent.

## Stack

| Layer | Technology | Why |
|---|---|---|
| Desktop shell | Tauri 2 (Rust) | small installers, signed auto-updates from GitHub releases, tray, a transparent click-through overlay window |
| UI | React 19 + TypeScript, Vite, Tailwind v4, shadcn/ui | modern components and type checking; runs in the system WebView2 |
| Engine | Rust crates | native speed and direct Windows APIs (WinRT OCR, GDI capture, SendInput) without helper processes |
| Models | Rust, hand-written | ridge regression for item values, logistic regression for sort learning; no ML runtime to ship |
| Storage | SQLite (`rusqlite`), JSON | market history, settings, trained models, quest progress |

## Layout

```
app/
  src/               React UI: pages/ (overview, market, lister, overlay, sorter, stash, quests,
                     settings), components/, lib/ (typed command wrappers, formatting)
  src-tauri/         Rust: windows, tray, updater, the overlay, and the commands the UI calls
    auto_lister/     the Auto lister page's backend: rules, plans, pricing, runs
    hover/           hover values: the read loop, the card window, market facts
    network.rs       live game data: capture -> decode -> market history, characters, quests
crates/
  appdata/           the data folder, settings, first-run import from DnDTools, starter data
  game-data/         item catalog (assets/items.json), icon pack (assets/icons.pak), stat labels
  protocol/          the game's message framing and types generated from protos/
  capture/           packet capture through Npcap (loaded at run time; no admin rights needed)
  state/             characters, stashes and the stash rules (the locked seasonal stash is off limits)
  market/            market history (SQLite), the value model (training and prediction), pricing
                     rules, learned roll patterns, the hover card
  tooltip/           finding and reading the game's item tooltip on screen
  screen/            screen capture, Windows OCR, cursor and focus
  input/             mouse and keyboard input, hotkeys, game window, screen layouts and calibration
  lister/            the auto lister: plans, Marketplace and merchant state, runs, the in-game runners
  sorter/            stash sort planning, running the moves, and learning how the player sorts
  quests/            quest catalog, progress, and the quest message handler
protos/              the game's message definitions (.proto)
assets/              item data, icons, quests, and the starter market data shipped with the app
docs/                this file and the roadmap
```

## Principles

- **Private by default.** No telemetry, no uploads. Updates come from this repository's releases.
  Starter data that ships with the app has no seller names and none of the author's listings.
- **Pure core, thin edges.** Parsing, pricing, planning and run orchestration are plain Rust with
  unit tests; screen, input and network access sit behind small traits so the logic is tested
  without the game running.
- **Hard rules at every layer.** The locked Seasonal Shared Stash is never listed, sold, sorted or
  clicked. Listing costs fees the game never refunds, so a real listing run always asks first.
- **The Python app is the reference.** DnDTools (the app this grew from) and its tests are the
  specification for each port; behaviour differences are deliberate and documented where they are.
- **A data folder of our own:** `%LOCALAPPDATA%\DaD Companion\`, with a one-time import from a
  DnDTools install, or the starter data on a fresh install.
