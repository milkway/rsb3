# rsb3

> Arquivos públicos da B3 em Arrow/Parquet — parser do COTAHIST com preços em
> centavos inteiros. *B3 (Brazilian stock exchange) public files as
> Arrow/Parquet: a strict COTAHIST parser with integer-cent prices.*

[![CI](https://github.com/milkway/rsb3/actions/workflows/ci.yml/badge.svg)](https://github.com/milkway/rsb3/actions)

O **COTAHIST** é o arquivo oficial de cotações históricas do pregão da B3
(formato posicional de 245 bytes, latin-1, estável há décadas, com preços de
2 decimais implícitos). O `rsb3` baixa, valida e converte esses arquivos para
**Arrow/Parquet**, com três decisões de projeto:

1. **O esquema é o contrato.** A saída é um `RecordBatch` no esquema
   versionado [`schema/cotahist.v1.json`](schema/cotahist.v1.json) — sem
   structs de domínio próprias. Mudança de esquema gera versão nova.
2. **Preços são inteiros em centavos** (`open_cents`, `close_cents`, …).
   Converter para ponto flutuante é decisão sua, nunca do parser.
3. **Nenhuma linha é descartada em silêncio.** Largura errada, tipo de
   registro desconhecido ou campo inválido são erros explícitos com número de
   linha — mudança de layout é detectada antes de contaminar datasets.

## Uso

```rust
use rsb3::cotahist::{self, Cache, Period, SchemaVersion};

let cache = Cache { dir: "data".into() };
let txt = cotahist::fetch(Period::Year(2025), &cache)?;   // download + cache + SHA-256 + manifesto
let batch = cotahist::parse(&txt)?;                        // posicional -> Arrow
cotahist::to_parquet(&[batch], "cotahist_2025.parquet".as_ref(), SchemaVersion::V1)?;
```

Ou, direto pelo exemplo:

```bash
cargo run --release --example backfill_cotahist -- 2025
```

### Features

| Feature | Default | O que traz |
|---|---|---|
| `fetch` | ✅ | download oficial com cache local imutável, SHA-256 e manifesto de ingestão (reqwest/zip/sha2) |
| `web` | — | índices e taxas referenciais (planejado; endpoints voláteis ficam fora do núcleo estável) |

Só quer o parser? `default-features = false` e nenhuma dependência HTTP entra.

## Validação: teste diferencial contra o `{rb3}`

O parser é validado **contra o pacote R [`{rb3}`](https://github.com/ropensci/rb3)**
(rOpenSci), que serve de oráculo: o mesmo arquivo é lido pelo leitor fwf do
próprio pacote e comparado linha a linha, sem tolerância
(`tests/differential_rb3_oracle.rs`). Na v0.1, o parser processou o ano de
2025 completo (3.174.698 linhas) com zero rejeições e diferença zero contra o
oráculo na contagem total e numa amostra integral de símbolos.

Os testes diferenciais pulam com mensagem quando `Rscript`/`{rb3}` não estão
instalados — o CI não depende de R. Para rodá-los localmente:

```bash
Rscript -e 'install.packages("rb3")'
cargo test
# opcional, sobre um arquivo oficial baixado:
RSB3_REAL_COTAHIST=data/COTAHIST_A2025.TXT cargo test --release
```

## Relação com o `{rb3}`

O nome `rsb3` diverge de `rb3` **de propósito**: o pacote R da rOpenSci
(de Wilson Freitas e colaboradores) é a referência da área e é usado aqui como
oráculo diferencial, não como concorrente. Este crate cobre o caso "quero os
arquivos da B3 dentro de um pipeline Rust/Arrow" — se você trabalha em R, use
o `{rb3}`.

## Licença

MIT.
