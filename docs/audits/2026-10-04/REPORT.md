# Lead Aggregator v0.4.5: дубли macOS и live HTTP аудит

Дата: 2026-10-04. Baseline: main / v0.4.5, `a093d71`. Проверены реальные ответы через CatalogHttpClient с User-Agent приложения, без браузера и обхода защиты.

## Before / after

Числа каталогов — уникальные ссылки-кандидаты, не экспортированные лиды.

| Проверка | Before | After | Причина / граница |
|---|---|---|---|
| Физические `.app` | 2 | 1, live | Удалена build-копия; сохранён `/Applications/Lead Aggregator.app` v0.4.5 |
| Launch Services | 16 записей, из них 14 stale DMG | 1 запись, live | Unregister, GC + rescan; `-kill` на этом macOS удалён |
| Spotlight | 2 | 1, live | Проверено `mdfind` по bundle ID |
| 2GIS, pages=20 | 20 запросов, 59 кандидатов, `max_pages` | 6 запросов, 60 кандидатов, `duplicate_pages` / `no_new_candidates`, live | Начиная с page 6 редирект на первую страницу; stale `hasPagesToLoad=true` больше не продолжает повтор |
| Yell, pages=20 | 20 страниц × 20 = 400, HTTP 200, `max_pages` | 400, replay тех же HTML | Рабочая пагинация, исправление URL не требуется |
| Zoon, pages=20 | 8 страниц × 30 + 4 посторонних ссылки = 244; page 9 HTTP 429 | 240, replay тех же HTML; остановка на 429 | Исключены network/offers/medical footer; после 429 сеть не повторялась |
| Rusprofile, pages=20 | HTTP 200 + CAPTCHA; CLI ошибочно посчитал 1000 скрытых ссылок | 0 принимаемых кандидатов при CAPTCHA | CLI проверяет защиту до discovery; после CAPTCHA сеть не повторялась |

Разница 59/60 в двух live прогонах 2GIS — изменение выдачи, не доказанный прирост от фикса. HTML сообщает `total=7636`, `pages=637`; фактически public HTTP после пятой страницы возвращает начало выдачи. Парсер не теряет тысячи карточек: они не пришли в доступной выдаче. При лимите 5 страниц около 60 результатов ожидаемо; повышение лимита не снимает редирект сайта.

## URL и пагинация

- 2GIS: `https://2gis.ru/moscow/search/автосервис`, далее `/page/N`. В каждом успешном HTML 12 уникальных firm-ссылок. В JSON сохранён percent-encoded URL и конечный URL каждого ответа.
- Yell: `https://www.yell.ru/moscow/top/?text=автосервис&page=N`. Все 20 ответов HTTP 200, 20 кандидатов на страницу.
- Zoon: probe `https://zoon.ru/search/?city=msk&query=автосервис&page=1`; категория `https://zoon.ru/msk/autoservice/`, далее `/page-N/`. `moscow → msk` уже реализовано в baseline. HTTP 429 на странице 9 — конечная граница этого прогона.
- Rusprofile: `https://www.rusprofile.ru/codes/45200000`, HTML содержит `/codes/45200000/2`, но видимая CAPTCHA скрывает результаты через `display: none`. Это общероссийский список ОКВЭД 45.2, а не фильтр Москвы. `/search = 404` — вводная пользователя, в этом прогоне не перепроверена: первый `/codes` уже вернул защиту.

По умолчанию остаётся только 2GIS, gentle: 50 страниц, 2000 результатов, задержка 2000 ms, concurrency=1. Yell/Zoon/Rusprofile остаются выключенными. Они не добавляют результаты к запуску по умолчанию.

## Исправления

Install/uninstall самостоятельно обнаруживают приложения через bundle ID, Spotlight, Launch Services, `/Applications`, `~/Applications`, cwd/target, custom install directory и `/Volumes`. Проверяют идентификатор перед удалением, сохраняют выбранный install destination, снимают stale DMG регистрации. Installer отсоединяет образ до обновления реестра. Относительный install path нормализуется. Busy DMG — явная ошибка с сохранением mounted temp directory. Произвольные неиндексированные и незарегистрированные каталоги вне этих корней глобально не сканируются.

2GIS и public catalogs прекращают повтор страниц независимо от stale next-page flags. 2GIS передаёт предупреждение о повторе в итог запуска. Все providers немедленно прекращают запросы при 429/CAPTCHA/403/anti-bot; Retry-After сохраняется в ошибке, автоматический retry на 429 удалён. CLI сохраняет Zoon probe как page 0, прекращает работу после защищённого probe и проверяет все ответы до парсинга. Неверные CLI limits отклоняются до сети.

## Проверки

- `cargo fmt --all`: выполнено.
- `cargo test --workspace`: 38 passed, 20 suites.
- `cargo clippy --workspace`: exit 0, 34 warnings; без ошибок.
- `pnpm --dir apps/desktop check`: 0 errors, 8 CSS warnings.
- `pnpm --dir apps/desktop build`: успешно, те же CSS warnings.
- Три shell-suite: `test_macos_duplicates.sh`, `test_install_pipe.sh`, `test_macos_paths.sh`: успешно.
- 2GIS профильный smoke: 1 страница, лимит 3; 3 профиля загружены, 3 уникальных записи, 0 parse errors. См. `2gis-profile-smoke.json`.

Discovery CLI before и after запускался с `--pages 20 --max-results 1000`. Yell прошёл все 20 страниц, 2GIS before — все 20. After 2GIS корректно закончил на повторе страницы 6. Для Zoon/Rusprofile достижение 20 страниц невозможно без нарушения обязательной остановки на защите. Enrichment/экспорт сотен Yell/Zoon профилей не проверялся. Полный install/uninstall цикл с переустановкой не выполнялся; live cleanup и реальные post-state проверены, scripts проверены mock-тестами. Установленный desktop бинарник остаётся исходным v0.4.5; исправления представлены исходниками и patch.

Пример выполненного audit:

```bash
cargo run -q -p lead-aggregator-cli -- audit --source 2gis --region moscow --query автосервис --pages 20 --max-results 1000 --json --save-html /tmp/lead-audit-after-2gis
```

Raw HTML сохранён локально в `/tmp/lead-audit-before-2gis`, `/tmp/lead-audit-after-2gis`, `/tmp/lead-audit-before-catalogs`. В patch включены только безопасные JSON/CSV отчёты и синтетические fixtures. Каталожный replay описан в `../2026-10-04-catalogs-live-notes.md`; сведения macOS — в `macos.md`. Без PR, commit, push, tag или новой release сборки. Исходный пользовательский diff `Cargo.lock` в patch не включён.
