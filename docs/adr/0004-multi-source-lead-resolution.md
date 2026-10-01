# ADR 0004 — Multi-source lead resolution

## Decision

Вынести контракт провайдеров в `provider-core`, хранить raw records отдельно и выполнять deterministic dedupe после сбора всех выбранных источников.

## Why

- новый каталог добавляется без изменения UI/application flow;
- provenance не теряется;
- повторный dedupe возможен без повторных сетевых запросов;
- один заблокированный источник не ломает весь run.

## Non-goals

- обход CAPTCHA/403/429;
- скрытая ротация прокси;
- fuzzy auto-merge только по названию;
- получение/использование чужих API-ключей.
