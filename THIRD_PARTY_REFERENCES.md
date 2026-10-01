# Third-party references

Ниже — архитектурные и продуктовые референсы. Код из них в этот репозиторий не копировался.

| Проект | Что берем как ориентир | Что не переносим |
|---|---|---|
| https://github.com/casoon/origin | domain не знает Tauri; adapters; typed errors; least privilege | крупную платформенную инфраструктуру, OAuth/MCP/jobs |
| https://github.com/frostybee/tauri-svelte-template | Tauri 2 + Svelte 5, generated IPC, desktop hygiene | лишние для утилиты updater/crash stack до реальной необходимости |
| https://github.com/Shiaoming123/meow-starter | local-first/SQLite/design-system подход | Vue и web/mobile слой |
| https://github.com/specta-rs/tauri-specta | typed commands, Rust как source of truth | rspc |
| https://github.com/interlark/parser-2gis | поля и пользовательский сценарий | Chrome/CDP и антибот-обход |
| https://github.com/VllSunday/2gis-parser | SQLite/export/config UX | Playwright/Chrome CDP |

## Лицензирование

Собственный код — MIT. Если в будущем будет переноситься код из стороннего проекта, сначала проверить его лицензию и зафиксировать происхождение в этом файле.
