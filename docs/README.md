# esp32sim documentation

| Document | What it covers |
| --- | --- |
| [architecture.md](architecture.md) | How the emulator is built: crates, CPU core, SoC bus, scheduling, boards, UI |
| [peripherals.md](peripherals.md) | Every modelled block, what part of it is modelled, what is missing |
| [boards.md](boards.md) | The `BoardModel` trait, the three boards, pin maps, how to add one |
| [cli.md](cli.md) | Command-line flags, environment variables, action scripts, output files |
| [web-ui.md](web-ui.md) | The browser UI and its WebSocket protocol |
| [decisions.md](decisions.md) | Design decisions and hard-won gotchas (the "why" behind the code) |
| [roadmap.md](roadmap.md) | What is planned, in priority order |
| [networking-plan.md](networking-plan.md) | Plan: WiFi/network support (virtual Ethernet + user-mode NAT + `esp_wifi` shim) |
| [testing-plan.md](testing-plan.md) | Plan: test layers, CPU/SoC/board/firmware suites, CI tiers, milestones |

Board-specific material lives next to the board: `../boards/atech14/README.md`,
`../examples/waveshare-cam/README.md`. The top-level `../README.md` is the quick start.
