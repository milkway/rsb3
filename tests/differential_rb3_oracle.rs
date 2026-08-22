//! Testes diferenciais do parser contra o oráculo R `{rb3}` (rOpenSci): o
//! mesmo arquivo é lido pelo leitor fwf do próprio pacote (via
//! `tools/oracle-rb3-cotahist.R`) e comparado linha a linha, sem tolerância —
//! a peça é determinística.
//!
//! Pulados com mensagem (sem falha) quando `Rscript`/`{rb3}` não estão
//! disponíveis; o CI não depende de R. Para o teste sobre arquivo oficial
//! real, aponte `RSB3_REAL_COTAHIST` para um TXT baixado (ex.: pelo exemplo
//! `backfill_cotahist`).

use std::path::{Path, PathBuf};
use std::process::Command;

use arrow::array::{Array, Date32Array, Int32Array, Int64Array, RecordBatch, StringArray};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

struct OracleOutput {
    total: usize,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl OracleOutput {
    fn idx(&self, name: &str) -> usize {
        self.header
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| panic!("coluna {name} ausente no oráculo"))
    }
}

/// Roda o script oráculo; `None` = indisponível (o chamador pula o teste).
fn run_oracle(file: &Path, symbols: Option<&str>) -> Option<OracleOutput> {
    let script = crate_root().join("tools/oracle-rb3-cotahist.R");
    let mut cmd = Command::new("Rscript");
    cmd.arg("--vanilla").arg(&script).arg(file);
    if let Some(s) = symbols {
        cmd.arg(s);
    }
    let out = cmd.output().ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || stdout.contains("ORACLE_UNAVAILABLE") {
        eprintln!("SKIP: oráculo {{rb3}} indisponível — {}", stdout.trim());
        return None;
    }
    let mut lines = stdout.lines();
    let total: usize = lines
        .next()?
        .strip_prefix("#total=")
        .expect("primeira linha deve ser #total=<n>")
        .parse()
        .expect("total numérico");
    let header: Vec<String> = lines.next()?.split(',').map(str::to_string).collect();
    let rows = lines
        .map(|l| l.split(',').map(str::to_string).collect())
        .collect();
    Some(OracleOutput {
        total,
        header,
        rows,
    })
}

fn str_col<'a>(batch: &'a RecordBatch, name: &str) -> &'a StringArray {
    batch
        .column_by_name(name)
        .expect("coluna existe")
        .as_any()
        .downcast_ref()
        .expect("tipo Utf8")
}

fn i64_col(batch: &RecordBatch, name: &str, i: usize) -> i64 {
    let a: &Int64Array = batch
        .column_by_name(name)
        .expect("coluna existe")
        .as_any()
        .downcast_ref()
        .expect("tipo Int64");
    a.value(i)
}

/// Compara, em ordem de arquivo, as linhas do batch (opcionalmente filtradas
/// por símbolo) com as linhas do oráculo.
fn assert_rows_match(batch: &RecordBatch, filter: Option<&[&str]>, oracle: &OracleOutput) {
    let symbols = str_col(batch, "symbol");
    let ours: Vec<usize> = (0..batch.num_rows())
        .filter(|&i| filter.is_none_or(|f| f.contains(&symbols.value(i))))
        .collect();
    assert_eq!(
        ours.len(),
        oracle.rows.len(),
        "quantidade de linhas comparáveis diverge do oráculo"
    );

    let refdate: &Date32Array = batch
        .column_by_name("refdate")
        .expect("refdate")
        .as_any()
        .downcast_ref()
        .expect("Date32");
    let isin = str_col(batch, "isin");
    let dist: &Int32Array = batch
        .column_by_name("distribution_id")
        .expect("distribution_id")
        .as_any()
        .downcast_ref()
        .expect("Int32");
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch");

    for (&row, v) in ours.iter().zip(&oracle.rows) {
        let sym = symbols.value(row);
        assert_eq!(sym, v[oracle.idx("symbol")], "símbolo na linha {row}");

        let d = epoch + chrono::Days::new(refdate.value(row) as u64);
        assert_eq!(d.format("%Y-%m-%d").to_string(), v[oracle.idx("refdate")]);

        for name in [
            "open_cents",
            "high_cents",
            "low_cents",
            "average_cents",
            "close_cents",
            "best_bid_cents",
            "best_ask_cents",
            "trade_quantity",
            "traded_contracts",
            "volume_cents",
        ] {
            let oracle_value: i64 = v[oracle.idx(name)].parse().expect("valor numérico");
            assert_eq!(
                i64_col(batch, name, row),
                oracle_value,
                "{name} diverge do oráculo para {sym} em {d}"
            );
        }

        assert_eq!(isin.value(row), v[oracle.idx("isin")]);
        let oracle_dist: i32 = v[oracle.idx("distribution_id")].parse().expect("dist");
        assert_eq!(dist.value(row), oracle_dist);
    }
}

#[test]
fn fixture_sintetico_identico_ao_oraculo() {
    let fixture = crate_root().join("tests/fixtures/COTAHIST_D20250602.TXT");
    let Some(oracle) = run_oracle(&fixture, None) else {
        eprintln!("SKIP: Rscript/{{rb3}} indisponível — teste diferencial não executado");
        return;
    };
    let batch = rsb3::cotahist::parse(&fixture).expect("parser rsb3");
    assert_eq!(oracle.total, batch.num_rows());
    assert!(oracle.total > 0, "oráculo não retornou linhas");
    assert_rows_match(&batch, None, &oracle);
}

/// Diferencial sobre um arquivo OFICIAL real: aponte `RSB3_REAL_COTAHIST`
/// para o TXT (ex.: baixado pelo exemplo `backfill_cotahist`). Compara a
/// contagem total do arquivo inteiro e, integralmente, todas as linhas de uma
/// amostra de símbolos líquidos.
#[test]
fn arquivo_real_identico_ao_oraculo() {
    const SAMPLE: &[&str] = &["PETR4", "VALE3", "ITUB4", "BOVA11", "WEGE3"];
    let Ok(real) = std::env::var("RSB3_REAL_COTAHIST") else {
        eprintln!("SKIP: RSB3_REAL_COTAHIST não definido — teste sobre arquivo real não executado");
        return;
    };
    let real = PathBuf::from(real);
    let Some(oracle) = run_oracle(&real, Some(&SAMPLE.join(","))) else {
        eprintln!("SKIP: Rscript/{{rb3}} indisponível — teste diferencial não executado");
        return;
    };
    let batch = rsb3::cotahist::parse(&real).expect("parser rsb3 no arquivo real");
    assert_eq!(
        oracle.total,
        batch.num_rows(),
        "contagem total do arquivo real diverge do oráculo"
    );
    assert!(!oracle.rows.is_empty(), "amostra vazia no oráculo");
    assert_rows_match(&batch, Some(SAMPLE), &oracle);
    println!(
        "arquivo real: {} linhas totais, {} linhas da amostra comparadas 1:1",
        oracle.total,
        oracle.rows.len()
    );
}
