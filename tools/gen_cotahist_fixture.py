#!/usr/bin/env python3
"""Gera um arquivo COTAHIST diário sintético (layout posicional de 245 bytes,
latin-1) para testes do parser rsb3 e do oráculo {rb3}.

Determinístico: mesmos valores a cada execução. Uso:
    python3 tools/gen_cotahist_fixture.py data/fixtures/cotahist/COTAHIST_D20250602.TXT
"""
import sys

REFDATE = "20250602"


def quote_line(bdi, symbol, tpmerc, name, spec, prazot, moeda, precos, totneg,
               quatot, voltot, isin, dismes):
    """Monta um registro tipo 01 com 245 bytes exatos."""
    preabe, premax, premin, premed, preult, preofc, preofv = precos
    campos = [
        ("01", 2),                # TIPREG
        (REFDATE, 8),             # DATA DO PREGÃO
        (bdi, 2),                 # CODBDI
        (symbol.ljust(12), 12),   # CODNEG
        (tpmerc, 3),              # TPMERC
        (name.ljust(12)[:12], 12),  # NOMRES
        (spec.ljust(10)[:10], 10),  # ESPECI
        (prazot, 3),              # PRAZOT (branco no mercado à vista)
        (moeda.ljust(4), 4),      # MODREF
        (f"{preabe:013d}", 13),   # PREABE (2 decimais implícitos)
        (f"{premax:013d}", 13),   # PREMAX
        (f"{premin:013d}", 13),   # PREMIN
        (f"{premed:013d}", 13),   # PREMED
        (f"{preult:013d}", 13),   # PREULT
        (f"{preofc:013d}", 13),   # PREOFC
        (f"{preofv:013d}", 13),   # PREOFV
        (f"{totneg:05d}", 5),     # TOTNEG
        (f"{quatot:018d}", 18),   # QUATOT
        (f"{voltot:018d}", 18),   # VOLTOT (2 decimais implícitos)
        (f"{0:013d}", 13),        # PREEXE
        ("0", 1),                 # INDOPC
        ("99991231", 8),          # DATVEN (sem vencimento)
        (f"{1:07d}", 7),          # FATCOT
        (f"{0:013d}", 13),        # PTOEXE
        (isin.ljust(12)[:12], 12),  # CODISI
        (dismes, 3),              # DISMES
    ]
    line = ""
    for valor, largura in campos:
        assert len(valor) == largura, f"{valor!r} != {largura}"
        line += valor
    assert len(line) == 245, len(line)
    return line


def main(out_path):
    header = ("00COTAHIST.D25" + "BOVESPA" + " " * 2 + REFDATE).ljust(245)
    trailer = ("99COTAHIST.D25" + "BOVESPA" + " " * 2 + REFDATE + f"{7:011d}").ljust(245)
    lines = [
        header,
        quote_line("02", "PETR4", "010", "PETROBRAS", "PN", "   ", "R$",
                   (3210, 3299, 3180, 3240, 3255, 3250, 3260),
                   12345, 45678900, 147993516000, "BRPETRACNPR6", "102"),
        quote_line("02", "VALE3", "010", "VALE", "ON", "   ", "R$",
                   (5601, 5688, 5555, 5620, 5610, 5605, 5615),
                   23456, 33445566, 187964881200, "BRVALEACNOR0", "141"),
        quote_line("02", "ITUB4", "010", "ITAUUNIBANCO", "PN ED", "   ", "R$",
                   (3405, 3450, 3390, 3420, 3444, 3440, 3448),
                   18765, 22334455, 76383836100, "BRITUBACNPR1", "133"),
        quote_line("02", "BOVA11", "010", "ISHARES BOVA", "CI", "   ", "R$",
                   (11230, 11350, 11190, 11270, 11305, 11300, 11310),
                   9876, 5544332, 62484822640, "BRBOVACTF003", "  1"),
        quote_line("02", "WEGE3", "010", "WEG", "ON", "   ", "R$",
                   (3888, 3920, 3855, 3890, 3901, 3899, 3905),
                   7654, 11223344, 43658908160, "BRWEGEACNOR0", "231"),
        # Papel sem negócios no dia: preços zerados (caso de borda real).
        quote_line("96", "XPTO3F", "020", "XPTO FRAC", "ON", "   ", "R$",
                   (0, 0, 0, 0, 0, 0, 0),
                   0, 0, 0, "BRXPTOACNOR9", "100"),
        trailer,
    ]
    with open(out_path, "w", encoding="latin-1", newline="\r\n") as f:
        for line in lines:
            f.write(line + "\n")
    print(f"{out_path}: {len(lines)} registros")


if __name__ == "__main__":
    main(sys.argv[1])
