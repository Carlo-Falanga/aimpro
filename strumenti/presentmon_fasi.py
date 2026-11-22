"""Riassume per fase del banco il CSV di PresentMon (strumenti/presentmon.ps1).

    python strumenti/presentmon_fasi.py misure/presentmon.csv misure/bench-presentmon.fasi.csv

Taglia le righe con la colonna TimeInQPC, l'istante del present (PresentMon --qpc_time), sugli istanti QPC delle
fasi scritti dal banco, e stampa, per ogni fase, mediana / 99° percentile delle colonne di latenza che ci sono, la modalità di
presentazione più frequente e l'uso della GPU (somma di MsGPUBusy sulla durata della fase).
"""

import csv
import sys
from collections import Counter

FREQ_QPC = 10_000_000  # QueryPerformanceFrequency su Windows 10 e 11

COLONNE = [
    ("MsBetweenPresents", "fra due present"),
    ("MsBetweenDisplayChange", "fra due immagini sullo schermo"),
    ("MsAllInputToPhotonLatency", "input→fotoni"),
    ("MsClickToPhotonLatency", "clic→fotoni"),
    ("MsUntilDisplayed", "present→schermo"),
    ("DisplayLatency", "display latency"),
    ("MsDisplayLatency", "display latency"),
    ("MsGPUBusy", "GPU occupata"),
]


def numero(v):
    try:
        x = float(v)
    except (TypeError, ValueError):
        return None
    return x if x == x else None  # scarta NaN


def percentile(ordinati, p):
    pos = p * (len(ordinati) - 1)
    i = int(pos)
    resto = pos - i
    if i + 1 < len(ordinati):
        return ordinati[i] + resto * (ordinati[i + 1] - ordinati[i])
    return ordinati[i]


def main(percorso_csv, percorso_fasi):
    sys.stdout.reconfigure(encoding="utf-8")  # le frecce e i gradi anche quando l'uscita va in un file
    with open(percorso_fasi, encoding="utf-8", newline="") as f:
        letti = list(csv.DictReader(f))
    # Una fase non valida (primo piano perso) si riporta, segnata, ma i suoi numeri non contano.
    fasi = [
        (r["fase"] + ("" if r.get("valida", "si") == "si" else " (NON VALIDA)"), int(r["qpc_inizio"]), int(r["qpc_fine"]))
        for r in letti
    ]
    # La frequenza del QPC la scrive il banco; se manca, quella solita di Windows 10 e 11.
    freq = int(letti[0].get("frequenza_qpc") or FREQ_QPC) if letti else FREQ_QPC
    with open(percorso_csv, encoding="utf-8-sig", newline="") as f:  # PresentMon scrive il BOM
        righe = list(csv.DictReader(f))
    if not righe:
        print("CSV vuoto")
        return
    presenti = [(c, n) for c, n in COLONNE if c in righe[0]]
    # Ogni riga va nella fase in cui è avvenuto il present (TimeInQPC). CPUStartQPC è l'inizio del frame: dopo una
    # pausa lunga cade nella fase prima e ci porta un intervallo di secondi che non è suo.
    qpc = "TimeInQPC" if "TimeInQPC" in righe[0] else "CPUStartQPC"
    if qpc not in righe[0]:
        sys.exit("mancano TimeInQPC e CPUStartQPC: PresentMon va lanciato con --qpc_time")
    print(f"Righe assegnate alle fasi con {qpc}.\n")
    # Le righe senza istante (l'ultima, se PresentMon è stato chiuso mentre scriveva) non si possono assegnare.
    prima = len(righe)
    righe = [r for r in righe if numero(r.get(qpc)) is not None]
    if len(righe) < prima:
        print(f"Righe senza {qpc} scartate: {prima - len(righe)}\n")
    for r in righe:
        r[qpc] = int(float(r[qpc]))

    intest = ["fase", "righe", "modalità"] + [n for _, n in presenti] + ["GPU"]
    print("| " + " | ".join(intest) + " |")
    print("|" + "---|" * len(intest))
    for nome, inizio, fine in fasi:
        dentro = [r for r in righe if inizio <= r[qpc] < fine]
        celle = [nome, str(len(dentro))]
        modi = Counter(r.get("PresentMode", "?") for r in dentro)
        celle.append(modi.most_common(1)[0][0] if modi else "-")
        for colonna, _ in presenti:
            valori = sorted(x for x in (numero(r.get(colonna)) for r in dentro) if x is not None)
            celle.append(
                f"{percentile(valori, 0.5):.2f} / {percentile(valori, 0.99):.2f} ({len(valori)})" if valori else "-"
            )
        durata_ms = (fine - inizio) / freq * 1000
        occupata = [x for x in (numero(r.get("MsGPUBusy")) for r in dentro) if x is not None]
        celle.append(f"{sum(occupata) / durata_ms * 100:.1f}%" if occupata and durata_ms > 0 else "-")
        print("| " + " | ".join(celle) + " |")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
