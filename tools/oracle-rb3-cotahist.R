# Oráculo diferencial do parser COTAHIST do crate rsb3: lê o mesmo arquivo com
# o leitor fwf do próprio {rb3} (template b3-cotahist-daily, mesmos
# widths/handlers do pacote) e imprime, no stdout:
#   linha 1: "#total=<n>" (registros tipo 01 no arquivo inteiro)
#   demais:  TSV normalizado (centavos inteiros), opcionalmente filtrado por
#            símbolos, para comparação linha a linha no teste Rust.
#
# Todas as colunas do esquema v1 do rsb3 saem aqui, nas mesmas unidades:
# preços em centavos, PTOEXE em milionésimos, datas ISO, NA como vazio.
# Separador TAB porque nomes de empresa podem conter vírgula.
#
# Se qualquer passo falhar, imprime ORACLE_UNAVAILABLE e sai com status 0 —
# o teste Rust pula com mensagem em vez de falhar.
# Uso: Rscript tools/oracle-rb3-cotahist.R <arquivo> [simbolos,separados,por,virgula]
result <- tryCatch({
  suppressMessages(library(rb3))
  args <- commandArgs(trailingOnly = TRUE)
  f <- args[1]
  symbols <- if (length(args) >= 2) strsplit(args[2], ",")[[1]] else NULL
  tpl <- template_retrieve("b3-cotahist-daily")
  reader <- get(tpl$reader[["function"]], envir = asNamespace("rb3"))
  df <- reader(tpl, f)
  df <- df[df$regtype == 1, ]
  cat(sprintf("#total=%d\n", nrow(df)))
  if (!is.null(symbols)) {
    df <- df[trimws(df$symbol) %in% symbols, ]
  }
  cents <- function(x) format(round(x * 100), scientific = FALSE, trim = TRUE)
  whole <- function(x) format(x, scientific = FALSE, trim = TRUE)
  out <- data.frame(
    refdate = format(df$refdate, "%Y-%m-%d"),
    bdi_code = as.integer(df$bdi_code),
    symbol = trimws(df$symbol),
    instrument_market = as.integer(df$instrument_market),
    corporation_name = trimws(df$corporation_name),
    specification_code = trimws(df$specification_code),
    days_to_settlement = as.integer(df$days_to_settlement),
    trading_currency = trimws(df$trading_currency),
    # {rb3} entrega preços em reais (numeric); voltamos a centavos inteiros
    # para comparar sem tolerância com o rsb3.
    open_cents = cents(df$open),
    high_cents = cents(df$high),
    low_cents = cents(df$low),
    average_cents = cents(df$average),
    close_cents = cents(df$close),
    best_bid_cents = cents(df$best_bid),
    best_ask_cents = cents(df$best_ask),
    trade_quantity = whole(df$trade_quantity),
    traded_contracts = whole(df$traded_contracts),
    volume_cents = cents(df$volume),
    strike_price_cents = cents(df$strike_price),
    strike_price_adjustment_indicator = trimws(df$strike_price_adjustment_indicator),
    # 9999-12-31 é "sem vencimento"; o rsb3 grava null.
    maturity_date = format(df$maturity_date, "%Y-%m-%d"),
    allocation_lot_size = whole(df$allocation_lot_size),
    strike_price_in_points_millionths = whole(round(df$strike_price_in_points * 1e6)),
    isin = trimws(df$isin),
    distribution_id = as.integer(df$distribution_id)
  )
  write.table(out, stdout(), sep = "\t", row.names = FALSE, quote = FALSE, na = "")
  TRUE
}, error = function(e) {
  cat("ORACLE_UNAVAILABLE:", conditionMessage(e), "\n")
  FALSE
})
invisible(result)
