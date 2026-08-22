//! # rsb3 — arquivos públicos da B3 em Arrow/Parquet
//!
//! Parser do **COTAHIST** (Cotações Históricas do Pregão da B3): formato
//! posicional de 245 bytes, latin-1, preços com 2 decimais implícitos →
//! **inteiros em centavos** no Arrow. A conversão para ponto flutuante é
//! decisão do consumidor, nunca do parser.
//!
//! - Núcleo estável (sem features): [`cotahist`] — parse → `RecordBatch`,
//!   escrita Parquet com esquema versionado.
//! - Feature `fetch` (default): download oficial com cache local, SHA-256 e
//!   manifesto de ingestão. Desligue-a para usar só o parser, sem
//!   dependências HTTP.
//! - Feature `web` (planejada): índices e taxas referenciais — endpoints
//!   voláteis, fora do núcleo estável.
//!
//! ## Filosofia de validação
//!
//! O esquema de saída é o contrato (`schema/cotahist.v1.json`, verificado por
//! teste), e o parser é testado **diferencialmente contra o pacote R
//! [`{rb3}`](https://github.com/ropensci/rb3)** (rOpenSci): mesmo arquivo,
//! comparação linha a linha sem tolerância. O nome `rsb3` diverge de `rb3` de
//! propósito, para não se confundir com o pacote R que serve de oráculo.
//!
//! Linhas rejeitadas nunca são silenciosas e mudança de layout é detectada
//! como erro ([`Error::UnexpectedLayout`]) antes de contaminar datasets.
//!
//! ## Exemplo
//!
//! ```no_run
//! # #[cfg(feature = "fetch")]
//! # fn demo() -> Result<(), rsb3::Error> {
//! use rsb3::cotahist::{self, Cache, Period, SchemaVersion};
//!
//! let cache = Cache { dir: "data".into() };
//! let txt = cotahist::fetch(Period::Year(2025), &cache)?;
//! let batch = cotahist::parse(&txt)?;
//! cotahist::to_parquet(&[batch], "cotahist_2025.parquet".as_ref(), SchemaVersion::V1)?;
//! # Ok(())
//! # }
//! ```

pub mod cotahist;

#[cfg(feature = "web")]
pub mod indexes {
    //! Composição e carteiras teóricas de índices — endpoints web voláteis.
    //! Planejado; ainda não implementado.
}

#[cfg(feature = "web")]
pub mod rates {
    //! Taxas referenciais (CDI etc.) — endpoints web voláteis.
    //! Planejado; ainda não implementado.
}

/// Erros do crate. Linha rejeitada nunca é silenciosa; mudança de layout do
/// arquivo oficial é erro explícito, para quarentena — nunca descarte.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("erro de I/O em {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("registro inválido na linha {line}: {reason}")]
    InvalidRecord { line: usize, reason: String },
    #[error(
        "possível mudança de layout na linha {line}: {reason} — \
         arquivo deve ir para quarentena, não para o dataset"
    )]
    UnexpectedLayout { line: usize, reason: String },
    #[error("erro Arrow: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("erro Parquet: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("falha no download de {url}: {reason}")]
    Download { url: String, reason: String },
    #[error("arquivo ZIP oficial inválido: {0}")]
    Archive(String),
    #[error("ainda não implementado: {0}")]
    Unimplemented(&'static str),
}
