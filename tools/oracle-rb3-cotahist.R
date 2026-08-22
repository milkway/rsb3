# Oráculo diferencial do parser COTAHIST do crate rsb3 (metodologia da seção 8
# do plano): lê o mesmo arquivo com o leitor fwf do próprio {rb3} (template
# b3-cotahist-daily, mesmos widths/handlers do pacote) e imprime, no stdout:
#   linha 1: "#total=<n>" (registros tipo 01 no arquivo inteiro)
#   demais:  CSV normalizado (centavos inteiros), opcionalmente filtrado por
#            símbolos, para comparação linha a linha no teste Rust.
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
  out <- data.frame(
    refdate = format(df$refdate, "%Y-%m-%d"),
    symbol = trimws(df$symbol),
    # {rb3} entrega preços em reais (numeric); voltamos a centavos inteiros
    # para comparar sem tolerância com o rsb3.
    open_cents = as.integer(round(df$open * 100)),
    high_cents = as.integer(round(df$high * 100)),
    low_cents = as.integer(round(df$low * 100)),
    average_cents = as.integer(round(df$average * 100)),
    close_cents = as.integer(round(df$close * 100)),
    best_bid_cents = as.integer(round(df$best_bid * 100)),
    best_ask_cents = as.integer(round(df$best_ask * 100)),
    trade_quantity = as.integer(df$trade_quantity),
    traded_contracts = format(df$traded_contracts, scientific = FALSE, trim = TRUE),
    volume_cents = format(round(df$volume * 100), scientific = FALSE, trim = TRUE),
    isin = trimws(df$isin),
    distribution_id = as.integer(df$distribution_id)
  )
  write.csv(out, stdout(), row.names = FALSE, quote = FALSE)
  TRUE
}, error = function(e) {
  cat("ORACLE_UNAVAILABLE:", conditionMessage(e), "\n")
  FALSE
})
invisible(result)
