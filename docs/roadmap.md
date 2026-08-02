# Build queue

The GitHub issue queue mirrors the product specification's dependency graph.
Issue numbers reflect concurrent creation order; the stable IDs are the title
prefixes.

| ID | Issue | Delivers | Depends on |
| --- | --- | --- | --- |
| BOOT-001 | [#1](https://github.com/bredebjorhovd/hotwire/issues/1) | Monorepo and vertical-slice boundaries | — |
| INP-001 | [#2](https://github.com/bredebjorhovd/hotwire/issues/2) | Quartz event-tap proof | BOOT-001 |
| UX-001 | [#3](https://github.com/bredebjorhovd/hotwire/issues/3) | Onboarding and live-board prototype | BOOT-001 |
| ADP-001 | [#4](https://github.com/bredebjorhovd/hotwire/issues/4) | Herdr and Papegøye adapters | CORE-001, APP-001 |
| APP-001 | [#5](https://github.com/bredebjorhovd/hotwire/issues/5) | Tauri shell, IPC, and menu bar | BOOT-001 |
| SAFE-001 | [#6](https://github.com/bredebjorhovd/hotwire/issues/6) | Runner, review, diagnostics, recovery | CORE-001, APP-001 |
| CORE-001 | [#7](https://github.com/bredebjorhovd/hotwire/issues/7) | Profiles, triggers, routing, receipts | BOOT-001 |

The first safe parallel wave after bootstrap is `INP-001`, `UX-001`,
`APP-001`, and `CORE-001`. Keep native capture, UI, shell lifecycle, and the
platform-neutral runtime in their declared ownership boundaries. `ADP-001` and
`SAFE-001` begin after the core and app contracts land.

