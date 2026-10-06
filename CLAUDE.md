# dumbfish-core

UCI-движок на Rust: классическая оценка, три фазы, настраиваемые сила и стиль.
GUI живёт в другом репозитории и общается с движком только по UCI — никаких других каналов.

## Структура

- `core/` (`dumbfish-core`) — битборды, таблицы атак, Board, FEN, Zobrist, генерация ходов, perft.
- `engine/` (`dumbfish`) — UCI-бинарник: поиск, оценка, bench.
- `tuner/` (`dumbfish-tuner`) — подбор весов оценки.
- `tools/` (`dumbfish-tools`) — вспомогательные утилиты: `magics` (генерация magic-чисел для `core/src/attacks.rs`).

## Правила

- Свои битборды и генератор ходов. Шахматные крейты (`chess`, `shakmaty`, `cozy-chess` и т.п.) не подключаем.
- Любое изменение генератора ходов проходит perft на стандартном наборе (CPW, https://www.chessprogramming.org/Perft_Results):
  - startpos: `rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1`
  - Kiwipete: `r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1`
  - Position 3: `8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1`
  - Position 4: `r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1`
  - Position 5: `rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8`
  - Position 6: `r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P3/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10`
- Изменения поиска и оценки сопровождаются счётчиком узлов `bench` (в сообщении коммита) и результатом SPRT.
  Эвристики не добавляются на интуиции — только с подтверждением SPRT.
- Оценка — линейная функция вектора признаков и весов по фазам. Веса хранятся в данных, не в коде.
  Команда `eval` печатает вклад каждого признака.
- Тесты лежат в `<крейт>/tests/` и проверяют публичный API; в исходниках `#[cfg(test)]` нет.
  Эталоны (медленные атаки и т.п.) в тестах пишутся независимо от кода движка.
- Перед коммитом:
  ```sh
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
- Коммиты строго по Conventional Commits (`feat(core): …`, `test(core): …`, `chore: …`). Без трейлеров авторства (`Co-Authored-By` и т.п.).
