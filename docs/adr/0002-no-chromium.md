# ADR-0002: No bundled Chromium

Status: accepted

Первая реализация провайдера читает публичный HTML 2ГИС через `reqwest`. Chrome/CDP/Playwright не используются.

Если HTML перестанет содержать нужные данные, допустим отдельный macOS WebKit provider через системный WebKit. Chromium остается вне проекта.
