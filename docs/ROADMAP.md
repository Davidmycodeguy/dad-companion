# Roadmap

## Version 1

- [x] App shell: main window, tray, single instance, settings, our own data folder with a one-time
      import from DnDTools, starter market data for fresh installs
- [x] Signed updates from GitHub releases; CI; tag-triggered installer builds
- [x] Item catalog, icon pack, Ctrl+K item search
- [x] Live game data: Npcap capture, the game's message framing and types, characters, stashes,
      marketplace pages, merchants and quests
- [x] Market research: the market at a glance, each item's listings, price spread, daily prices and
      probable sales; market history in SQLite (compatible with DnDTools' database)
- [x] Value model: training and prediction in Rust (matches DnDTools' model), learned roll patterns
- [x] Hover values: tooltip finder and reader (Windows OCR), the value card beside the tooltip
- [x] Auto lister: plans priced from the live market, saved data or the value model; review; listing
      with a last-second re-check; collecting; crawls; selling to merchants; calibration hover test
- [x] Stash sorter: planning, running the moves with on-screen checks, learning from corrections
- [x] Stash viewer with gold and item values; quests with progress and an item checklist
- [x] Overview dashboard
- [x] Website, video and the lantern logo

## Next

- [ ] Faster hover values with DXGI capture
- [ ] Calibration offsets editor (per resolution) for the lister's screen positions
- [ ] Stash-to-stash transfers and clearing the bag into stash tabs
- [ ] Written guides (calibration, pricing rules)
