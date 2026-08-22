//! Parser do COTAHIST (Cotações Históricas do Pregão): formato posicional de
//! 245 bytes, latin-1, preços com 2 decimais implícitos → inteiros em
//! centavos no Arrow.
//!
//! Nomes de colunas alinhados aos do template `b3-cotahist-daily` do oráculo
//! `{rb3}`, com sufixo `_cents` onde a unidade é centavo — o que torna a
//! comparação diferencial direta e a unidade impossível de ignorar.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Date32Builder, Int32Builder, Int64Builder, RecordBatch, StringBuilder,
};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::NaiveDate;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::Error;

/// Versões do esquema Arrow do COTAHIST. O esquema é o contrato: mudança
/// gera versão nova, nunca alteração in-place. Espelhado em
/// `schema/cotahist.v1.json` e verificado por teste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaVersion {
    V1,
}

impl SchemaVersion {
    pub fn schema(self) -> Arc<Schema> {
        match self {
            SchemaVersion::V1 => Arc::new(Schema::new(vec![
                Field::new("refdate", DataType::Date32, false),
                Field::new("bdi_code", DataType::Utf8, false),
                Field::new("symbol", DataType::Utf8, false),
                Field::new("instrument_market", DataType::Int32, false),
                Field::new("corporation_name", DataType::Utf8, false),
                Field::new("specification_code", DataType::Utf8, false),
                // Branco no mercado à vista.
                Field::new("days_to_settlement", DataType::Int32, true),
                Field::new("trading_currency", DataType::Utf8, false),
                Field::new("open_cents", DataType::Int64, false),
                Field::new("high_cents", DataType::Int64, false),
                Field::new("low_cents", DataType::Int64, false),
                Field::new("average_cents", DataType::Int64, false),
                Field::new("close_cents", DataType::Int64, false),
                Field::new("best_bid_cents", DataType::Int64, false),
                Field::new("best_ask_cents", DataType::Int64, false),
                Field::new("trade_quantity", DataType::Int64, false),
                Field::new("traded_contracts", DataType::Int64, false),
                Field::new("volume_cents", DataType::Int64, false),
                Field::new("strike_price_cents", DataType::Int64, false),
                Field::new("strike_price_adjustment_indicator", DataType::Utf8, false),
                // 99991231 no arquivo significa "sem vencimento" → null.
                Field::new("maturity_date", DataType::Date32, true),
                Field::new("allocation_lot_size", DataType::Int64, false),
                // 6 decimais implícitos no arquivo; guardado como milionésimos.
                Field::new("strike_price_in_points_millionths", DataType::Int64, false),
                Field::new("isin", DataType::Utf8, false),
                Field::new("distribution_id", DataType::Int32, false),
            ])),
        }
    }
}

/// Período de um arquivo COTAHIST oficial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Year(i32),
    Day(NaiveDate),
}

/// Diretório de cache local dos arquivos baixados (raw store dos arquivos
/// oficiais).
#[derive(Debug, Clone)]
pub struct Cache {
    pub dir: std::path::PathBuf,
}

impl Period {
    /// Nome oficial do arquivo (sem extensão), como publicado pela B3.
    pub fn file_stem(self) -> String {
        match self {
            Period::Year(y) => format!("COTAHIST_A{y}"),
            Period::Day(d) => format!("COTAHIST_D{}", d.format("%d%m%Y")),
        }
    }
}

/// Download + cache local do arquivo oficial da B3 (superfície da seção 8).
///
/// Comportamento de raw store (seção 9 do plano): o ZIP original é guardado
/// imutável no cache com hash SHA-256 e manifesto de ingestão
/// (`<nome>.manifest.json`); o TXT extraído fica ao lado. Idempotente: se o
/// TXT já existe no cache, nada é baixado de novo.
///
/// Retorna o caminho do TXT pronto para [`parse`].
#[cfg(feature = "fetch")]
pub fn fetch(period: Period, cache: &Cache) -> Result<std::path::PathBuf, Error> {
    use sha2::Digest;
    use std::io::Read;

    const BASE_URL: &str = "https://bvmf.bmfbovespa.com.br/InstDados/SerHist";

    let stem = period.file_stem();
    let txt_path = cache.dir.join(format!("{stem}.TXT"));
    if txt_path.exists() {
        return Ok(txt_path);
    }
    std::fs::create_dir_all(&cache.dir).map_err(|source| Error::Io {
        path: cache.dir.display().to_string(),
        source,
    })?;

    // 1. Download do ZIP oficial (se ainda não estiver no cache).
    let zip_path = cache.dir.join(format!("{stem}.ZIP"));
    let url = format!("{BASE_URL}/{stem}.ZIP");
    if !zip_path.exists() {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| Error::Download {
                url: url.clone(),
                reason: e.to_string(),
            })?;
        let response = client.get(&url).send().map_err(|e| Error::Download {
            url: url.clone(),
            reason: e.to_string(),
        })?;
        if !response.status().is_success() {
            return Err(Error::Download {
                url,
                reason: format!("status HTTP {}", response.status()),
            });
        }
        let bytes = response.bytes().map_err(|e| Error::Download {
            url: url.clone(),
            reason: e.to_string(),
        })?;
        // Escrita atômica: nunca deixar um ZIP truncado com o nome final.
        let tmp = cache.dir.join(format!("{stem}.ZIP.part"));
        std::fs::write(&tmp, &bytes).map_err(|source| Error::Io {
            path: tmp.display().to_string(),
            source,
        })?;
        std::fs::rename(&tmp, &zip_path).map_err(|source| Error::Io {
            path: zip_path.display().to_string(),
            source,
        })?;
    }

    // 2. Hash + manifesto de ingestão do artefato original.
    let zip_bytes = std::fs::read(&zip_path).map_err(|source| Error::Io {
        path: zip_path.display().to_string(),
        source,
    })?;
    let sha256: String = sha2::Sha256::digest(&zip_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let manifest = serde_json::json!({
        "provider": "b3_cotahist",
        "url": url,
        "file": format!("{stem}.ZIP"),
        "sha256": sha256,
        "size_bytes": zip_bytes.len(),
        "ingested_at": chrono::Utc::now().to_rfc3339(),
    });
    let manifest_path = cache.dir.join(format!("{stem}.manifest.json"));
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|source| Error::Io {
        path: manifest_path.display().to_string(),
        source,
    })?;

    // 3. Extração do único TXT do arquivo.
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| Error::Archive(e.to_string()))?;
    if archive.len() != 1 {
        return Err(Error::Archive(format!(
            "esperava 1 arquivo no ZIP, encontrei {}",
            archive.len()
        )));
    }
    let mut entry = archive
        .by_index(0)
        .map_err(|e| Error::Archive(e.to_string()))?;
    let mut content = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut content)
        .map_err(|e| Error::Archive(e.to_string()))?;
    let tmp_txt = cache.dir.join(format!("{stem}.TXT.part"));
    std::fs::write(&tmp_txt, &content).map_err(|source| Error::Io {
        path: tmp_txt.display().to_string(),
        source,
    })?;
    std::fs::rename(&tmp_txt, &txt_path).map_err(|source| Error::Io {
        path: txt_path.display().to_string(),
        source,
    })?;
    Ok(txt_path)
}

const RECORD_LEN: usize = 245;

/// Lê um arquivo COTAHIST (posicional, latin-1) e devolve um `RecordBatch`
/// no esquema [`SchemaVersion::V1`]. Registros de header (00) e trailer (99)
/// são descartados; qualquer outra anomalia é erro — nunca descarte
/// silencioso.
pub fn parse(path: &Path) -> Result<RecordBatch, Error> {
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_bytes(&bytes)
}

/// Corpo do parser, separado para testes e para uso futuro em streaming.
pub fn parse_bytes(bytes: &[u8]) -> Result<RecordBatch, Error> {
    let mut b = Builders::new();
    let mut saw_trailer = false;

    for (idx, raw_line) in bytes.split(|&c| c == b'\n').enumerate() {
        let line_no = idx + 1;
        let line = match raw_line.strip_suffix(b"\r") {
            Some(l) => l,
            None => raw_line,
        };
        if line.is_empty() {
            continue; // linha final vazia após o último \n
        }
        if saw_trailer {
            return Err(Error::UnexpectedLayout {
                line: line_no,
                reason: "registro após o trailer (99)".into(),
            });
        }
        if line.len() != RECORD_LEN {
            return Err(Error::UnexpectedLayout {
                line: line_no,
                reason: format!("largura {} ≠ {RECORD_LEN} bytes", line.len()),
            });
        }
        match &line[0..2] {
            b"00" => {} // header
            b"99" => saw_trailer = true,
            b"01" => parse_quote(line, line_no, &mut b)?,
            other => {
                return Err(Error::UnexpectedLayout {
                    line: line_no,
                    reason: format!("tipo de registro desconhecido: {:?}", latin1_str(other)),
                });
            }
        }
    }

    b.finish()
}

/// Grava batches em Parquet no esquema congelado da versão indicada.
pub fn to_parquet(
    batches: &[RecordBatch],
    out: &Path,
    version: SchemaVersion,
) -> Result<(), Error> {
    let schema = version.schema();
    for batch in batches {
        if batch.schema() != schema {
            return Err(Error::Arrow(arrow::error::ArrowError::SchemaError(
                "batch não está no esquema da versão pedida".into(),
            )));
        }
    }
    let file = File::create(out).map_err(|source| Error::Io {
        path: out.display().to_string(),
        source,
    })?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Extração de campos do registro tipo 01
// ---------------------------------------------------------------------------

/// Layout posicional do registro 01 (posições 1-based da especificação da
/// B3, aqui como offsets 0-based `[início, fim)`).
mod pos {
    pub const REFDATE: (usize, usize) = (2, 10); // DATA DO PREGÃO
    pub const BDI: (usize, usize) = (10, 12); // CODBDI
    pub const SYMBOL: (usize, usize) = (12, 24); // CODNEG
    pub const MARKET: (usize, usize) = (24, 27); // TPMERC
    pub const NAME: (usize, usize) = (27, 39); // NOMRES
    pub const SPEC: (usize, usize) = (39, 49); // ESPECI
    pub const SETTLEMENT: (usize, usize) = (49, 52); // PRAZOT
    pub const CURRENCY: (usize, usize) = (52, 56); // MODREF
    pub const OPEN: (usize, usize) = (56, 69); // PREABE
    pub const HIGH: (usize, usize) = (69, 82); // PREMAX
    pub const LOW: (usize, usize) = (82, 95); // PREMIN
    pub const AVERAGE: (usize, usize) = (95, 108); // PREMED
    pub const CLOSE: (usize, usize) = (108, 121); // PREULT
    pub const BEST_BID: (usize, usize) = (121, 134); // PREOFC
    pub const BEST_ASK: (usize, usize) = (134, 147); // PREOFV
    pub const TRADES: (usize, usize) = (147, 152); // TOTNEG
    pub const CONTRACTS: (usize, usize) = (152, 170); // QUATOT
    pub const VOLUME: (usize, usize) = (170, 188); // VOLTOT
    pub const STRIKE: (usize, usize) = (188, 201); // PREEXE
    pub const STRIKE_ADJ: (usize, usize) = (201, 202); // INDOPC
    pub const MATURITY: (usize, usize) = (202, 210); // DATVEN
    pub const LOT_SIZE: (usize, usize) = (210, 217); // FATCOT
    pub const STRIKE_PTS: (usize, usize) = (217, 230); // PTOEXE
    pub const ISIN: (usize, usize) = (230, 242); // CODISI
    pub const DIST: (usize, usize) = (242, 245); // DISMES
}

fn field(line: &[u8], range: (usize, usize)) -> &[u8] {
    &line[range.0..range.1]
}

/// Decodifica latin-1: cada byte vira o code point de mesmo valor.
fn latin1_str(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn text(line: &[u8], range: (usize, usize)) -> String {
    latin1_str(field(line, range)).trim().to_string()
}

fn int(line: &[u8], range: (usize, usize), line_no: usize, name: &str) -> Result<i64, Error> {
    let s = latin1_str(field(line, range));
    let t = s.trim();
    t.parse::<i64>().map_err(|_| Error::InvalidRecord {
        line: line_no,
        reason: format!("campo {name} não numérico: {s:?}"),
    })
}

fn int_opt(
    line: &[u8],
    range: (usize, usize),
    line_no: usize,
    name: &str,
) -> Result<Option<i64>, Error> {
    let s = latin1_str(field(line, range));
    if s.trim().is_empty() {
        return Ok(None);
    }
    int(line, range, line_no, name).map(Some)
}

fn date(
    line: &[u8],
    range: (usize, usize),
    line_no: usize,
    name: &str,
) -> Result<NaiveDate, Error> {
    let s = latin1_str(field(line, range));
    NaiveDate::parse_from_str(s.trim(), "%Y%m%d").map_err(|_| Error::InvalidRecord {
        line: line_no,
        reason: format!("campo {name} não é data AAAAMMDD: {s:?}"),
    })
}

fn date32(d: NaiveDate) -> i32 {
    const EPOCH: NaiveDate = match NaiveDate::from_ymd_opt(1970, 1, 1) {
        Some(d) => d,
        None => unreachable!(),
    };
    (d - EPOCH).num_days() as i32
}

struct Builders {
    refdate: Date32Builder,
    bdi_code: StringBuilder,
    symbol: StringBuilder,
    instrument_market: Int32Builder,
    corporation_name: StringBuilder,
    specification_code: StringBuilder,
    days_to_settlement: Int32Builder,
    trading_currency: StringBuilder,
    open_cents: Int64Builder,
    high_cents: Int64Builder,
    low_cents: Int64Builder,
    average_cents: Int64Builder,
    close_cents: Int64Builder,
    best_bid_cents: Int64Builder,
    best_ask_cents: Int64Builder,
    trade_quantity: Int64Builder,
    traded_contracts: Int64Builder,
    volume_cents: Int64Builder,
    strike_price_cents: Int64Builder,
    strike_price_adjustment_indicator: StringBuilder,
    maturity_date: Date32Builder,
    allocation_lot_size: Int64Builder,
    strike_price_in_points_millionths: Int64Builder,
    isin: StringBuilder,
    distribution_id: Int32Builder,
}

impl Builders {
    fn new() -> Self {
        Self {
            refdate: Date32Builder::new(),
            bdi_code: StringBuilder::new(),
            symbol: StringBuilder::new(),
            instrument_market: Int32Builder::new(),
            corporation_name: StringBuilder::new(),
            specification_code: StringBuilder::new(),
            days_to_settlement: Int32Builder::new(),
            trading_currency: StringBuilder::new(),
            open_cents: Int64Builder::new(),
            high_cents: Int64Builder::new(),
            low_cents: Int64Builder::new(),
            average_cents: Int64Builder::new(),
            close_cents: Int64Builder::new(),
            best_bid_cents: Int64Builder::new(),
            best_ask_cents: Int64Builder::new(),
            trade_quantity: Int64Builder::new(),
            traded_contracts: Int64Builder::new(),
            volume_cents: Int64Builder::new(),
            strike_price_cents: Int64Builder::new(),
            strike_price_adjustment_indicator: StringBuilder::new(),
            maturity_date: Date32Builder::new(),
            allocation_lot_size: Int64Builder::new(),
            strike_price_in_points_millionths: Int64Builder::new(),
            isin: StringBuilder::new(),
            distribution_id: Int32Builder::new(),
        }
    }

    fn finish(mut self) -> Result<RecordBatch, Error> {
        let arrays: Vec<ArrayRef> = vec![
            Arc::new(self.refdate.finish()),
            Arc::new(self.bdi_code.finish()),
            Arc::new(self.symbol.finish()),
            Arc::new(self.instrument_market.finish()),
            Arc::new(self.corporation_name.finish()),
            Arc::new(self.specification_code.finish()),
            Arc::new(self.days_to_settlement.finish()),
            Arc::new(self.trading_currency.finish()),
            Arc::new(self.open_cents.finish()),
            Arc::new(self.high_cents.finish()),
            Arc::new(self.low_cents.finish()),
            Arc::new(self.average_cents.finish()),
            Arc::new(self.close_cents.finish()),
            Arc::new(self.best_bid_cents.finish()),
            Arc::new(self.best_ask_cents.finish()),
            Arc::new(self.trade_quantity.finish()),
            Arc::new(self.traded_contracts.finish()),
            Arc::new(self.volume_cents.finish()),
            Arc::new(self.strike_price_cents.finish()),
            Arc::new(self.strike_price_adjustment_indicator.finish()),
            Arc::new(self.maturity_date.finish()),
            Arc::new(self.allocation_lot_size.finish()),
            Arc::new(self.strike_price_in_points_millionths.finish()),
            Arc::new(self.isin.finish()),
            Arc::new(self.distribution_id.finish()),
        ];
        RecordBatch::try_new(SchemaVersion::V1.schema(), arrays).map_err(Error::Arrow)
    }
}

fn parse_quote(line: &[u8], line_no: usize, b: &mut Builders) -> Result<(), Error> {
    b.refdate
        .append_value(date32(date(line, pos::REFDATE, line_no, "DATA")?));
    b.bdi_code.append_value(text(line, pos::BDI));
    b.symbol.append_value(text(line, pos::SYMBOL));
    b.instrument_market
        .append_value(int(line, pos::MARKET, line_no, "TPMERC")? as i32);
    b.corporation_name.append_value(text(line, pos::NAME));
    b.specification_code.append_value(text(line, pos::SPEC));
    match int_opt(line, pos::SETTLEMENT, line_no, "PRAZOT")? {
        Some(v) => b.days_to_settlement.append_value(v as i32),
        None => b.days_to_settlement.append_null(),
    }
    b.trading_currency.append_value(text(line, pos::CURRENCY));
    b.open_cents
        .append_value(int(line, pos::OPEN, line_no, "PREABE")?);
    b.high_cents
        .append_value(int(line, pos::HIGH, line_no, "PREMAX")?);
    b.low_cents
        .append_value(int(line, pos::LOW, line_no, "PREMIN")?);
    b.average_cents
        .append_value(int(line, pos::AVERAGE, line_no, "PREMED")?);
    b.close_cents
        .append_value(int(line, pos::CLOSE, line_no, "PREULT")?);
    b.best_bid_cents
        .append_value(int(line, pos::BEST_BID, line_no, "PREOFC")?);
    b.best_ask_cents
        .append_value(int(line, pos::BEST_ASK, line_no, "PREOFV")?);
    b.trade_quantity
        .append_value(int(line, pos::TRADES, line_no, "TOTNEG")?);
    b.traded_contracts
        .append_value(int(line, pos::CONTRACTS, line_no, "QUATOT")?);
    b.volume_cents
        .append_value(int(line, pos::VOLUME, line_no, "VOLTOT")?);
    b.strike_price_cents
        .append_value(int(line, pos::STRIKE, line_no, "PREEXE")?);
    b.strike_price_adjustment_indicator
        .append_value(text(line, pos::STRIKE_ADJ));
    // 99991231 = sem vencimento (mercado à vista) → null.
    let maturity_raw = text(line, pos::MATURITY);
    if maturity_raw == "99991231" {
        b.maturity_date.append_null();
    } else {
        b.maturity_date
            .append_value(date32(date(line, pos::MATURITY, line_no, "DATVEN")?));
    }
    b.allocation_lot_size
        .append_value(int(line, pos::LOT_SIZE, line_no, "FATCOT")?);
    b.strike_price_in_points_millionths.append_value(int(
        line,
        pos::STRIKE_PTS,
        line_no,
        "PTOEXE",
    )?);
    b.isin.append_value(text(line, pos::ISIN));
    b.distribution_id
        .append_value(int(line, pos::DIST, line_no, "DISMES")? as i32);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Array, Date32Array, Int32Array, Int64Array, StringArray};

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/COTAHIST_D20250602.TXT")
    }

    fn col_str<'a>(batch: &'a RecordBatch, name: &str) -> &'a StringArray {
        batch
            .column_by_name(name)
            .expect("coluna existe")
            .as_any()
            .downcast_ref()
            .expect("tipo Utf8")
    }

    fn col_i64<'a>(batch: &'a RecordBatch, name: &str) -> &'a Int64Array {
        batch
            .column_by_name(name)
            .expect("coluna existe")
            .as_any()
            .downcast_ref()
            .expect("tipo Int64")
    }

    #[test]
    fn parseia_fixture_sintetico() {
        let batch = parse(&fixture_path()).expect("fixture parseia");
        assert_eq!(batch.num_rows(), 6);
        assert_eq!(batch.schema(), SchemaVersion::V1.schema());

        let symbols = col_str(&batch, "symbol");
        assert_eq!(symbols.value(0), "PETR4");
        // Preços com 2 decimais implícitos viram centavos inteiros.
        assert_eq!(col_i64(&batch, "open_cents").value(0), 3210);
        assert_eq!(col_i64(&batch, "close_cents").value(0), 3255);
        assert_eq!(col_i64(&batch, "volume_cents").value(0), 147_993_516_000);

        // Papel sem negócio: zeros, não nulls (o arquivo traz zeros).
        assert_eq!(symbols.value(5), "XPTO3F");
        assert_eq!(col_i64(&batch, "close_cents").value(5), 0);

        // Mercado à vista: PRAZOT em branco → null; DATVEN 99991231 → null.
        let settlement: &Int32Array = batch
            .column_by_name("days_to_settlement")
            .expect("coluna")
            .as_any()
            .downcast_ref()
            .expect("Int32");
        assert!(settlement.is_null(0));
        let maturity: &Date32Array = batch
            .column_by_name("maturity_date")
            .expect("coluna")
            .as_any()
            .downcast_ref()
            .expect("Date32");
        assert!(maturity.is_null(0));
    }

    #[test]
    fn largura_errada_e_mudanca_de_layout() {
        let bad = b"01202506029XCURTO\n";
        let err = parse_bytes(bad).expect_err("deve falhar");
        assert!(matches!(err, Error::UnexpectedLayout { line: 1, .. }));
    }

    #[test]
    fn tipo_de_registro_desconhecido_e_erro() {
        let mut line = vec![b' '; RECORD_LEN];
        line[0] = b'7';
        line[1] = b'7';
        line.push(b'\n');
        let err = parse_bytes(&line).expect_err("deve falhar");
        assert!(matches!(err, Error::UnexpectedLayout { line: 1, .. }));
    }

    #[test]
    fn campo_nao_numerico_e_erro_com_linha() {
        let bytes = std::fs::read(fixture_path()).expect("fixture");
        let mut corrupted = bytes.clone();
        // Corrompe o PREABE da primeira cotação (linha 2).
        let line_start = 247; // 245 + \r\n
        corrupted[line_start + pos::OPEN.0] = b'X';
        let err = parse_bytes(&corrupted).expect_err("deve falhar");
        assert!(matches!(err, Error::InvalidRecord { line: 2, .. }));
    }

    #[test]
    fn roundtrip_parquet_preserva_esquema() {
        let batch = parse(&fixture_path()).expect("fixture parseia");
        let dir = std::env::temp_dir().join("rsb3-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let out = dir.join("cotahist_roundtrip.parquet");
        to_parquet(std::slice::from_ref(&batch), &out, SchemaVersion::V1).expect("escreve");

        let file = File::open(&out).expect("abre parquet");
        let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)
            .expect("reader");
        assert_eq!(
            reader.schema().as_ref(),
            SchemaVersion::V1.schema().as_ref()
        );
        let batches: Vec<_> = reader
            .build()
            .expect("build")
            .collect::<Result<_, _>>()
            .expect("lê");
        assert_eq!(batches.iter().map(|b| b.num_rows()).sum::<usize>(), 6);
    }

    #[test]
    fn nomes_de_arquivo_oficiais() {
        assert_eq!(Period::Year(2025).file_stem(), "COTAHIST_A2025");
        let d = NaiveDate::from_ymd_opt(2025, 6, 2).expect("data");
        assert_eq!(Period::Day(d).file_stem(), "COTAHIST_D02062025");
    }
}
