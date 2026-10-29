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
    let header: Vec<String> = lines.next()?.split('\t').map(str::to_string).collect();
    let rows = lines
        .map(|l| l.split('\t').map(str::to_string).collect())
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

fn col<'a, T: 'static>(batch: &'a RecordBatch, name: &str) -> &'a T {
    batch
        .column_by_name(name)
        .unwrap_or_else(|| panic!("coluna {name} existe"))
        .as_any()
        .downcast_ref()
        .unwrap_or_else(|| panic!("tipo de {name}"))
}

/// Date32 → ISO; null → `9999-12-31` (como o {rb3} entrega "sem vencimento").
fn date_str(a: &Date32Array, i: usize, null_as: &str) -> String {
    if a.is_null(i) {
        return null_as.to_string();
    }
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch");
    (epoch + chrono::Days::new(a.value(i) as u64))
        .format("%Y-%m-%d")
        .to_string()
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

    // Todas as 25 colunas do esquema v1 são comparadas; nenhuma fica de fora.
    let refdate: &Date32Array = col(batch, "refdate");
    let maturity: &Date32Array = col(batch, "maturity_date");
    let settlement: &Int32Array = col(batch, "days_to_settlement");
    let bdi = str_col(batch, "bdi_code");

    const I32_COLS: &[&str] = &["instrument_market", "distribution_id"];
    const I64_COLS: &[&str] = &[
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
        "strike_price_cents",
        "allocation_lot_size",
        "strike_price_in_points_millionths",
    ];
    const STR_COLS: &[&str] = &[
        "corporation_name",
        "specification_code",
        "trading_currency",
        "strike_price_adjustment_indicator",
        "isin",
    ];

    for (&row, v) in ours.iter().zip(&oracle.rows) {
        let sym = symbols.value(row);
        assert_eq!(sym, v[oracle.idx("symbol")], "símbolo na linha {row}");
        let d = date_str(refdate, row, "");
        assert_eq!(d, v[oracle.idx("refdate")], "refdate de {sym}");

        // CODBDI: o rsb3 preserva o texto ("02"); o {rb3} entrega inteiro.
        let our_bdi: i32 = bdi.value(row).parse().expect("bdi numérico");
        let oracle_bdi: i32 = v[oracle.idx("bdi_code")].parse().expect("bdi do oráculo");
        assert_eq!(our_bdi, oracle_bdi, "bdi_code diverge para {sym} em {d}");

        for name in I32_COLS {
            let a: &Int32Array = col(batch, name);
            let oracle_value: i32 = v[oracle.idx(name)].parse().expect("valor numérico");
            assert_eq!(
                a.value(row),
                oracle_value,
                "{name} diverge para {sym} em {d}"
            );
        }
        for name in I64_COLS {
            let a: &Int64Array = col(batch, name);
            let oracle_value: i64 = v[oracle.idx(name)].parse().expect("valor numérico");
            assert_eq!(
                a.value(row),
                oracle_value,
                "{name} diverge para {sym} em {d}"
            );
        }
        for name in STR_COLS {
            assert_eq!(
                str_col(batch, name).value(row),
                v[oracle.idx(name)],
                "{name} diverge para {sym} em {d}"
            );
        }

        // Nulos: PRAZOT em branco ↔ NA (vazio no TSV); DATVEN 99991231 ↔ 9999-12-31.
        let ours = if settlement.is_null(row) {
            String::new()
        } else {
            settlement.value(row).to_string()
        };
        assert_eq!(
            ours,
            v[oracle.idx("days_to_settlement")],
            "days_to_settlement diverge para {sym} em {d}"
        );
        assert_eq!(
            date_str(maturity, row, "9999-12-31"),
            v[oracle.idx("maturity_date")],
            "maturity_date diverge para {sym} em {d}"
        );
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
/// contagem total do arquivo inteiro e, integralmente (todas as colunas),
/// todas as linhas de uma amostra de símbolos líquidos — incluindo uma opção
/// e um futuro, para cobrir PRAZOT, DATVEN, PREEXE e PTOEXE não vazios.
#[test]
fn arquivo_real_identico_ao_oraculo() {
    const SAMPLE: &[&str] = &[
        "PETR4", "VALE3", "ITUB4", "BOVA11", "WEGE3", // à vista
        "PETRA300", "VALEB700", // opções (PREEXE, DATVEN preenchidos)
        "PETR4T",   // termo (PRAZOT preenchido)
    ];
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
