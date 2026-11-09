---
category: tech
owner: marty
created_at: 2026-11-09
last_verified: 2026-11-09
source_of_truth: how to build Adaptaudio.app and the release archive for GitHub Releases
guard: none
---

# Сборка Adaptaudio.app для GitHub Releases

`bundle.sh` собирает приложение и архив, который потом скачивает самообновление
(`apps/adaptaudio/src/update.rs`).

```
tools/bundle-macos/bundle.sh
```

Нужен Mac на Apple Silicon. Скрипт ничего не публикует.

## Что делает скрипт

1. `cargo build --release -p adaptaudio`.
2. Собирает `dist/Adaptaudio.app`: бинарник в `Contents/MacOS/adaptaudio` и `Info.plist`.
   Версия берётся из `[workspace.package]` в корневом `Cargo.toml`.
3. Подписывает ad-hoc: `codesign --force --deep -s -`.
4. Упаковывает через `ditto -c -k --keepParent` в `dist/Adaptaudio-<версия>-macos-arm64.zip`
   и кладёт рядом `.sha256`.
5. Печатает команду `gh release create`. Запускаешь её сам.

Идентификатор приложения по умолчанию `com.martycow.adaptaudio`. Поменять:
`BUNDLE_ID=com.example.adaptaudio tools/bundle-macos/bundle.sh`. Меняй его один раз и до первого
релиза: macOS привязывает разрешения (микрофон) к идентификатору.

## Как выпустить версию

1. Подними `version` в `[workspace.package]` корневого `Cargo.toml`.
2. Запусти `tools/bundle-macos/bundle.sh`.
3. Выполни напечатанную команду `gh release create v<версия> …`, подставив свой репозиторий.

Тег релиза должен быть `v<версия>`. Имена файлов не меняй: обновление ищет файл, который
заканчивается на `macos-arm64.zip`, и файл с тем же именем плюс `.sha256`.

## Что важно знать

- **Подпись ad-hoc, а не Developer ID.** Она защищает от повреждённого архива, но не
  подтверждает автора. Обновление доверяет тому, что файл скачан по HTTPS с `github.com` и что
  SHA-256 сошёлся.
- **Первый запуск после ручной загрузки из браузера.** macOS пометит архив как «из интернета» и
  Gatekeeper откажется открывать приложение без нотаризации. Открой через правый клик →
  «Открыть» или сними метку: `xattr -dr com.apple.quarantine Adaptaudio.app`. При
  самообновлении метки нет: `curl` её не ставит.
- **Микрофон.** У ad-hoc подписи нет постоянной «личности», поэтому после обновления macOS может
  заново спросить доступ к микрофону.
- **Старая версия остаётся рядом** как `Adaptaudio.app.previous`. Чтобы откатиться, удали новую
  и переименуй старую обратно.
- `dist/` — результат сборки, в git его не добавляй.
