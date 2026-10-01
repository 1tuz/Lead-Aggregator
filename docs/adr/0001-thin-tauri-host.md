# ADR-0001: Thin Tauri host

Status: accepted

Tauri используется только как desktop host и IPC boundary. Домен, сеть, хранилище и экспорт вынесены в отдельные Rust crates.

Причина: тестируемость без GUI, возможность позже добавить CLI или другой host без переноса бизнес-логики.
