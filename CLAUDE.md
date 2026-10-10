# dumbfish-core

UCI-движок на Rust: классическая оценка, три фазы, настраиваемые сила и стиль.
GUI живёт в другом репозитории и общается с движком только по UCI — никаких других каналов.

## Структура

- `core/` (`dumbfish-core`) — битборды, таблицы атак, Board, FEN, Zobrist, генерация ходов, perft.
- `engine/` (`dumbfish`) — библиотека (поиск, оценка, UCI, bench) и бинарник. UCI-команды, опции и формат вывода
  описаны в `README.md`; при их изменении README обновляется в том же коммите. `dumbfish bench [depth]` печатает
  `N nodes M nps` (`N` — сигнатура, одинаковая на любой машине); bench и SPRT однопоточные.
  Веса оценки — `engine/weights.txt`, пресеты стиля — `engine/styles.txt`, модель WDL — `engine/wdl.txt`.
- `tuner/` (`dumbfish-tuner`) — подбор весов оценки методом Texel:
  `dumbfish-tuner <dataset.epd> <weights.txt> [epochs] [--freeze=<группа>,...]` переписывает файл весов. Не трогает
  веса групп из `--freeze` (имена как у групп стиля) и веса признаков, активных менее чем в 0.01 % позиций.
  Датасет — `quiet-labeled.v7.epd` (Zurichess, 1.4M позиций с результатами партий):
  ```sh
  mkdir -p target/tuner && curl -fsSL https://bitbucket.org/zurichess/tuner/downloads/quiet-labeled.v7.epd.gz \
    | gzip -d > target/tuner/quiet-labeled.v7.epd
  ```
  Новые веса принимаются только после SPRT, как любое изменение оценки.
- `tools/` (`dumbfish-tools`) — вспомогательные утилиты:
  - `magics` — генерация magic-чисел для `core/src/attacks.rs`;
  - `perft <depth> [fen]` — perft с разбивкой по ходам из корня (для поиска расхождений с эталонным движком);
  - `wdl <dataset.epd>` — подбор модели выигрыш/ничья/проигрыш для `engine/wdl.txt` по результатам партий.
- `justfile`: `just uci [ref]` собирает движок из ветки или коммита (по умолчанию — последней изменённой ветки,
  `.` — рабочее дерево) и запускает UCI в терминале; `just build [ref]` кладёт бинарник в `target/play/dumbfish` для GUI
  и ссылку `target/play/syzygy` на таблицы: движок сам берёт таблицы из `SYZYGY_PATH` или из каталога `syzygy`
  рядом с исполняемым файлом.
- `scripts/sprt.sh <base-ref> [<dev-ref>]` — SPRT `<dev-ref>` (по умолчанию рабочего дерева) против `<base-ref>`
  через fastchess (см. ниже).
- `scripts/calibrate.sh [<ref>]` — Elo каждого `Skill Level` для `UCI_Elo` (`engine/skill-elo.txt`): каждый уровень
  играет со Stockfish на близком `UCI_Elo` порциями партий; прерванный прогон при повторном запуске продолжается.
  Калибровка нужна заново после любого изменения силы игры.
- Эндшпильные таблицы Syzygy читает свой код (`engine/src/syzygy/`). Таблицы до 5 фигур скачиваются в
  `target/syzygy` (команда в `README.md`); тесты с ними (сверка с сервером таблиц lichess, согласованность
  результата позиции с результатами ходов и техника матования голого короля без таблиц против защиты по
  таблицам) по умолчанию пропускаются:
  ```sh
  cargo test --release -p dumbfish --test syzygy --test uci --test technique -- --include-ignored
  ```

## Правила

- Свои битборды и генератор ходов. Шахматные крейты (`chess`, `shakmaty`, `cozy-chess` и т.п.) не подключаем.
- Любое изменение генератора ходов проходит perft на стандартном наборе (CPW, https://www.chessprogramming.org/Perft_Results):
  - startpos: `rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1`
  - Kiwipete: `r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1`
  - Position 3: `8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1`
  - Position 4: `r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1`
  - Position 5: `rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8`
  - Position 6: `r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10`

  Полный набор (с ожидаемыми числами) — `core/tests/perft.rs`, запуск:
  ```sh
  cargo test --release -p dumbfish-core --test perft -- --include-ignored
  ```
- Изменения поиска и оценки сопровождаются счётчиком узлов `bench` и результатом SPRT. Сигнатура пишется
  отдельной строкой `Bench: N` в теле коммита; получить её: `cargo run --release -p dumbfish -- bench`.
  Эвристики не добавляются на интуиции — только с подтверждением SPRT. Группу эвристик можно проверить одним
  SPRT (вершина цепочки веток против её базы); если группа не проходит, её делят пополам и проверяют половины,
  пока не найдётся виновник. Итог такого теста дописывается в последний коммит группы.
- SPRT: `scripts/sprt.sh <base-ref> [<dev-ref>]` собирает обе версии и играет матч до принятия H0 или H1
  (по умолчанию `tc=8+0.08`, границы `[0, 5]`, книга `8moves_v3` скачивается в `target/sprt/`). Для упрощений —
  тест на отсутствие регресса: `ELO0=-5 ELO1=0`. UCI-опции одной стороны — `DEV_OPTIONS`/`BASE_OPTIONS`
  (например, проверка многопоточности: `DEV_OPTIONS=Threads=2 CONCURRENCY=1`). Итог fastchess (строки `Elo:`, `LLR:`, `Games:`) копируется в
  тело коммита под строкой `Bench: N`. Нужен `fastchess` в `PATH`:
  ```sh
  git clone --depth 1 --branch v1.8.2-alpha https://github.com/Disservin/fastchess ~/.local/src/fastchess
  make -C ~/.local/src/fastchess LDFLAGS=-pthread   # на ALT нет libstdc++fs
  cp ~/.local/src/fastchess/fastchess ~/.local/bin/
  ```
- Оценка — линейная функция вектора признаков и весов по фазам. Веса хранятся в данных, не в коде
  (`engine/weights.txt`: `признак дебют миттельшпиль эндшпиль`). Фаза считается по материалу (конь и слон 1,
  ладья 2, ферзь 4, максимум 24): 24 — чистый дебют, 12 — чистый миттельшпиль, 0 — чистый эндшпиль, между ними
  линейное смешивание соседних фаз. Новый признак — эвристика, добавляется только с SPRT. Исключение — признаки
  `mop_up_*`, активные только против голого короля: в обычных партиях они почти не встречаются, их веса заданы
  вручную и проверяются тестом `technique` (мат в пределах 50 ходов).
  UCI-команда `eval` печатает вклад каждого признака.
- Комментарии в коде — только документация (`///`, `//!`); обычных `//` комментариев нет.
  Смысл проверки в тестах передаётся сообщением ассерта. Данные (позиции bench, веса, пресеты) лежат в файлах.
- Тесты лежат в `<крейт>/tests/` и проверяют публичный API; в исходниках `#[cfg(test)]` нет.
  Эталоны (медленные атаки и т.п.) в тестах пишутся независимо от кода движка.
- Перед коммитом:
  ```sh
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
- Коммиты строго по Conventional Commits (`feat(core): …`, `test(core): …`, `chore: …`). Без трейлеров авторства (`Co-Authored-By` и т.п.).
- Коммиты атомарные: одна фича — один или несколько коммитов, каждый оставляет проект в рабочем состоянии
  (проходят fmt, clippy и тесты). Несвязанные изменения не смешиваются в одном коммите.
