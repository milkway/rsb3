# Changelog

Formato: [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versões
seguem [SemVer](https://semver.org/lang/pt-BR/). O esquema Arrow (`v1`) tem
versionamento próprio e **não mudou**: Parquets gerados pela 0.1.0 continuam
válidos.

## [0.2.0] — 2026-10-29

### Corrigido

- **Arquivo truncado passava em silêncio.** O parser aceitava um COTAHIST
  sem trailer (99) e sem header (00) — exatamente o que um download
  interrompido deixa. Agora header na primeira linha e trailer na última são
  obrigatórios, e o TOTAL DE REGISTROS do trailer é conferido com o número de
  cotações lidas (`Error::UnexpectedLayout`). As duas convenções históricas
  da B3 são aceitas: só cotações (arquivos atuais) ou cotações + header +
  trailer (ex.: `COTAHIST_A2010`).
- **MSRV declarada era falsa.** `rust-version` dizia 1.85, mas o `zip` 8.x
  já exigia 1.88 com a feature default `fetch`. Agora `rust-version = "1.88"`
  e o CI compila nessa toolchain.
- Fixture sintética: o trailer declarava 7 registros para 6 cotações.

### Alterado

- **`arrow`/`parquet` 59 → 60.** Como os tipos do Arrow fazem parte da API
  pública (`RecordBatch`), quem consome o crate precisa acompanhar — daí a
  versão 0.2.
- `parquet` sem features default: só `arrow` + `snap` (o único codec usado).
  Saem `zstd`, `brotli`, `lz4` e `flate2` da árvore de dependências — menos
  compilação e menos superfície de advisories (o `zstd` 0.13 acumulava 13).
- `zip` 8 → 9 (advisories RUSTSEC-2026-0367/0368/0369; o `rsb3` nunca usou
  `extract`, mas a versão antiga continuava na árvore de quem dependesse dele).

### Adicionado

- `Period::Month { year, month }` → `COTAHIST_M<MM><AAAA>`, o arquivo mensal
  oficial.
- Oráculo diferencial `{rb3}` compara agora **todas as 25 colunas** do esquema
  v1 (antes: 13), inclusive `days_to_settlement`, `maturity_date`,
  `strike_price_*` e `bdi_code`; a amostra do arquivo real inclui opções e
  termo, para exercitar PRAZOT/DATVEN/PREEXE/PTOEXE não vazios.
- CI: job de MSRV, `cargo audit` (também semanal) e `cargo doc` com
  `-D warnings`.

## [0.1.0] — 2026-08-22

Primeira versão: parser COTAHIST posicional → Arrow (esquema `v1` congelado),
escrita Parquet, `fetch` oficial com cache/SHA-256/manifesto e teste
diferencial contra o `{rb3}`.
