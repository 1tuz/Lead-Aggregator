# Agent rules

1. Сначала прочитай `ARCHITECTURE.md` и `DESIGN.md`.
2. Не добавляй Chromium, Electron, Playwright, Selenium или ChromeDriver.
3. Не добавляй чужие/найденные API-ключи.
4. Не реализуй обход CAPTCHA, 403, 429 или антибот-защиты.
5. Доменный код не должен импортировать Tauri.
6. Frontend не делает прямых внешних HTTP-запросов.
7. Новые IPC-команды: Rust command → `collect_commands![]` → generated bindings → UI wrapper.
8. Не расширяй Tauri capabilities без фактической необходимости.
9. Перед изменением парсера добавь/обнови синтетический HTML fixture/test.
10. После изменений: `cargo fmt --all`, `cargo test --workspace`, `cargo clippy --workspace`, `pnpm --dir apps/desktop check`, `pnpm --dir apps/desktop build`.
11. Для больших задач используй независимые подзадачи/субагентов, затем один интеграционный проход.
12. Не добавляй абстракцию «на будущее», пока нет реального второго потребителя.

## Multi-source rules (v0.2)

13. Каждый новый каталог реализует `DirectoryProvider`; не добавляй source-specific условия в application/UI.
14. Не стирай `source_records`: это provenance и основа повторной дедупликации без сети.
15. Автосклейка разрешена только по сильным ключам. Не объединяй лиды только по похожему названию.
16. Сохраняй `sources`, `branches` и source tags при merge.
17. Provider обязан иметь консервативный `ProviderPolicy` и прекращать работу на CAPTCHA/403/429.
18. Перед релизом проверь adapters на небольшом live smoke test (1 страница, 3-5 карточек) и обнови fixture-тесты; не пытайся обходить защиту, если источник блокирует автоматические запросы.
