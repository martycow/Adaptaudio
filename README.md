---
category: product
owner: marty
created_at: 2026-11-09
last_verified: 2026-11-09
source_of_truth: how to run Adaptaudio and where to start reading
guard: none
---

# Adaptaudio

Мастерская адаптивной музыки для **Cedar Station**: сочиняешь музыку из треков и паттернов,
озвучиваешь встроенным синтом или своим железом, проверяешь, как она реагирует на игру, и
экспортируешь в FMOD для Unity.

## Запуск

```bash
cargo run --release -p adaptaudio
```

Первая сборка тянет Rune и занимает пару минут. Проект открывается с демо «Cedar Station —
Night Shift»: восемь режимов игры, 14 треков, сценарии и три головоломки.

Что попробовать за пять минут:

1. **Пробел** — играть. Клавиши **1–8** — режимы игры (Explore … PlayDead).
2. Справа — **Simulator**: тяни XY-пад (Intensity × Tension), жми события (⚡ Detected).
3. **Simulator → Scenarios → Full night → Run**: вся ночь опоссума за две минуты.
4. **Pattern** — кнопки ✦ Melody, Chords, Arp, Bass, Vary пишут ноты за тебя (всегда в ладу).
5. **Sound Lab** — крути ручки; ✦ Mutate даёт вариации звука. Наведи на любое слово — подсказка.
6. **⇥** в верхней панели — экспорт в FMOD (стемы + manifest + головоломки).

Послушать без интерфейса:

```bash
cargo run --release -p aa-engine --example render_scenario -- "Full night" night.wav
```

## Документы

- [AGENTS.md](AGENTS.md) — правила для агентов, команды, структура.
- [docs/VISION.md](docs/VISION.md) — что это и зачем; решения и открытые вопросы.
- [docs/ADAPTIVE_AUDIO.md](docs/ADAPTIVE_AUDIO.md) — модель адаптивной музыки и её перенос в FMOD.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — крейты, потоки, формат проекта, что не проверено.
- [docs/GLOSSARY.md](docs/GLOSSARY.md) — словарь (его же показывает экран Learn).
- [docs/ROADMAP.md](docs/ROADMAP.md) — фазы и их статус.
- [tools/fmod-import/README.md](tools/fmod-import/README.md) — импорт экспорта в FMOD Studio.
