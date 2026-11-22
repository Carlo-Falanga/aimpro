# aimpro

Aim trainer nativo per Windows, a bassa latenza, di Carlo. Il contesto del progetto sta in
`D:\dev\contesti\aimpro\CONTESTO.md` (chiave `aimpro` in `~/.claude/carlo/progetti.json`).

## Stack

| Pezzo | Versione | Perché |
|---|---|---|
| Rust | 1.100.0, edition 2024, fissato in `rust-toolchain.toml` (target `x86_64-pc-windows-msvc`) | niente garbage collector né runtime: nessuna pausa imprevista nel frame; controllo su allocazioni e thread |
| winit | 0.30.13, senza X11 e Wayland | finestra, tastiera, schermo intero esclusivo alla frequenza più alta del monitor |
| wgpu | 30.0.2, solo backend DX12 e WGSL | DX12 è l'API nativa di Windows: swapchain flip, `Immediate` con vsync spento, frame latency 1 |
| Raw Input di Windows | crate `windows` 0.62.2 | count grezzi del mouse, senza accelerazione né cursore, con l'istante QPC di ogni evento |
| egui, egui-wgpu, egui-winit | 0.37.0 | solo menu, pausa, riepilogo, statistiche e pannello F3: nel frame del giro egui non gira (col pannello aperto, 4 volte al secondo) |
| glam, bytemuck | 0.34.3, 1.25.2 | vettori e buffer per la GPU |
| serde, toml | 1.0.229, 1.2.2 | le impostazioni in `%APPDATA%\aimpro\config.toml` e lo storico dei giri |

Release compilata con `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`.

### Alternative scartate (21/11/2026)
- **Webapp con WebGL2 o WebGPU** (il primo piano): il browser aggiunge il suo compositore, il ciclo di
  `requestAnimationFrame` legato al vsync e la sua gestione del mouse; il ritardo non si controlla e non si misura
  con PresentMon. Carlo ha scelto il programma nativo dal menu.
- **Three.js**: stessa strada del browser, con in più una libreria pesante per una scena di una sfera.
- **Rust compilato in WASM**: il codice sarebbe Rust ma il percorso input→schermo resterebbe quello del browser.
- **Bevy**: motore completo con ECS, scheduler multi-thread e pipeline di rendering sue; per una stanza e una sfera
  è un ciclo dei frame che non controlliamo noi, più dipendenze e più lavoro per frame. Con winit + wgpu il frame è
  scritto a mano e ogni passo si misura.

## Come è fatto

- `ingresso.rs`: un thread con una finestra solo messaggi, registrata per il Raw Input del mouse (RIDEV_INPUTSINK)
  solo durante il giro. Somma i count in atomici con l'istante QPC; i clic vanno in una coda fissa con i totali del
  loro istante. winit non si registra per gli stessi eventi (`listen_device_events(Never)`).
- `app.rs`, il frame del giro: limite di fps (se c'è) → `get_current_texture` (l'attesa della swapchain) → i clic,
  ognuno valutato con la visuale del suo istante → il movimento letto all'ultimo momento → matrici → disegno →
  `present`. Nel menu, in pausa e nel riepilogo non c'è ciclo: si ridisegna solo su evento, al massimo una volta per
  refresh (col menu non in primo piano solo se ci passa sopra il mouse). Finestra non in primo piano o ridotta: il
  giro va in pausa e non si disegna. Il pannello F3 si ricalcola dopo `present`, sugli ultimi 4096 campioni.
- `grafica.rs` e `scena.wgsl`: stanza a griglia, sfera disegnata per pixel (raggio contro sfera), mirino e tempo
  come rettangoli istanziati, riscritti solo quando cambiano.
- `giro.rs`, `camera.rs`, `sensibilita.rs`, `config.rs`: lo scenario, la visuale, la conversione delle sensibilità
  (CS2 0,022 gradi per count, Valorant 0,07; FOV di CS2 orizzontale su 4:3, di Valorant su 16:9) e le impostazioni.
  Gli scenari durano 30 secondi (lo standard scelto da Carlo il 22/11/2026). Record e statistiche sono per id di
  scenario: uno scenario con un'altra durata ha un altro id.
- `registro.rs`: per ogni bersaglio colpito il tempo comparsa→colpo (pause tolte), la distanza angolare dal
  precedente (per il primo, dalla visuale di partenza) e gli overshoot (il mirino entra nella sfera e ne esce senza
  colpirla, controllato a ogni clic con la visuale del suo istante e a ogni frame dopo il movimento: un passaggio
  che entra ed esce dentro lo stesso frame non si vede, e col limite a 240 fps un frame dura 4,17 ms). Legge il giro
  solo dalla sua parte pubblica; la memoria (1024 bersagli) si prepara all'avvio, nel frame non alloca.
- `storico.rs`, `esporta.rs`: lo storico dei giri (formato qui sotto), il record per scenario, la sessione e
  l'esportazione CSV. La schermata Statistiche sta in `interfaccia.rs`, col grafico disegnato dal painter di egui.
- `latenza.rs`, `misure.rs`, `banco.rs`, `allocazioni.rs`: le misure, il pannello F3 e il banco `--bench`.

## Lo storico dei giri

Un file TOML per giro in `%APPDATA%\aimpro\storico\`, scritto in `fine_giro` dopo il present dell'ultimo frame
(prima in `<nome>.toml.nuovo`, poi rinominato). Il nome è `AAAAMMGG-hhmmss-mmm-<scenario>.toml` (ora locale; se
esiste già, `-2`, `-3`…; dell'id nel nome restano solo lettere e cifre ASCII e `-`, il resto diventa `_`). Il banco non
scrive nello storico.

```toml
formato = 1                     # versione del formato; un file con un numero più alto si salta
scenario = "scatto"             # id dello scenario: record e statistiche sono per id
data = "2026-11-22 14:00:34"    # ora locale di fine giro
istante_ms = 1795352434530      # ms dal 1970 in UTC: l'ordine dei giri
durata_s = 30.0
colpi = 88
errori = 0
precisione = 1.0                # colpi / clic, fra 0 e 1; manca se non ci sono clic
tempo_medio_ms = 338.28         # manca se non ci sono colpi
overshoot = 89                  # tutti, anche sul bersaglio rimasto in campo alla fine

[impostazioni]                  # quelle con cui si è giocato il giro
gioco = "cs2"                   # "cs2" o "valorant"
sensibilita = 1.25
cm_360 = 41.56
fov = 90.0
dpi = 800.0
vsync = false
limite_fps = 0                  # 0 = nessun limite
versione = "0.1.0"              # versione di aimpro

[[bersagli]]                    # uno per bersaglio colpito, in ordine (al più 1024)
tempo_ms = 297.44               # comparsa → colpo
distanza_gradi = 24.78          # dal bersaglio precedente
overshoot = 1
```

All'avvio un thread legge tutti i file, mentre la finestra si apre (il primo che serve lo storico, a fine giro o
nelle Statistiche, lo aspetta); uno rovinato (o di un formato più nuovo, o senza i campi obbligatori) si salta e
si conta, e la schermata Statistiche lo dice. In memoria i giri restano senza i bersagli. Il record di uno scenario
sono i colpi del suo giro migliore; "nuovo record" quando un giro lo supera (pareggiarlo no; il primo giro di uno
scenario è record se ha almeno un colpo). Uno scenario nuovo usa un suo `scenario = "<id>"` e gli stessi campi; campi in più vanno
aggiunti con un valore predefinito (`#[serde(default)]`), così i file vecchi si leggono ancora; un cambio che i
file vecchi non reggono alza `formato`.

"Esporta CSV" scrive `aimpro-giri-AAAAMMGG-hhmmss.csv` nella cartella `aimpro` dei Documenti di Windows (quella vera,
anche se spostata, da `SHGetKnownFolderPath`), una riga per giro. Per Excel in italiano: separatore `;`, decimali
con la virgola, data `GG/MM/AAAA hh:mm:ss`, UTF-8 con il BOM, righe chiuse da CRLF.

## Come si prova

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                      # 71 test: sensibilità, impostazioni, visuale, giro, registro, storico, CSV, misure, tempo
cargo run --release             # il gioco, a schermo intero (Esc pausa, F3 pannello della latenza)
cargo run --release -- --finestra            # in una finestra normale
cargo run --release -- --bench --secondi 15  # il banco: misura da solo ed esce, scrive misure\bench-<ora>.md
```

Il banco prende lo schermo intero per circa due minuti e simula il mouse con `SendInput` solo quando aimpro è in
primo piano. Ogni fase di giro riparte da capo se la finestra perde il primo piano. Accanto al file dei risultati
scrive `*.fasi.csv` con gli istanti QPC di ogni fase, per tagliare il CSV di PresentMon sulle stesse fasi.

Per provare storico e statistiche senza toccare quelli veri si avvia aimpro con `APPDATA` puntato a una cartella di
prova (lo storico e le impostazioni stanno lì dentro, in `aimpro\`).

Il gioco col mouse vero e la sensazione al tatto li prova Carlo: i tester del browser non possono.

## Misure

Misurate il 21/11/2026 su questo PC: AMD Ryzen 7 9800X3D (16 core logici), NVIDIA GeForce RTX 4070 SUPER (driver
32.0.16.2101), 32 GB, Windows 11 Pro build 26200, schermo 1920x1080 a 240 Hz, schermo intero esclusivo.

### Dal programma (`aimpro --bench --secondi 15`, mouse simulato a 1000 Hz)

Il banco gira da solo, senza clic: chiede da sé il primo piano per la sua finestra, entra nel giro, nel giro tiene
il cursore di Windows fermo al centro (il Raw Input riceve comunque i count) e alla fine lo rimette dov'era. A fine
fase controlla di aver misurato davvero quello che dice il nome: stato del programma (giro, menu, riepilogo, pausa,
icona), finestra in primo piano, cursore bloccato, presentazione giusta, frame del giro misurati, movimenti mandati
almeno al 90% e visti dal Raw Input almeno al 90%. Se un controllo non passa, la riga esce "NON VALIDA" con il
motivo nelle note, e qui non si riporta. Nella fase "menu, mouse in movimento" i movimenti arrivano alla finestra
come messaggi, senza muovere il cursore.

Tempi in ms. "count→present" va dall'istante QPC dell'evento del mouse al ritorno di `present`: "vecchio" è il
primo evento entrato nel frame, "recente" l'ultimo. Non comprende la coda della scheda video né la scansione dello
schermo: quelle le misura PresentMon. Frame time e count→present vecchio: mediana / 99° / 99,9° percentile /
massimo; le altre colonne mediana / 99°. Ogni anello tiene gli ultimi 65 536 campioni (circa 24 s a 2700 fps).
Tutte le fasi valide.

| fase | fps | frame time | jitter (dev. std) | count→present vecchio | count→present recente | clic→present | CPU (tempo / cicli) |
|---|---|---|---|---|---|---|---|
| vsync spento, senza limite | 2619 | 0,35 / 1,30 / 1,64 / 2,97 | 0,177 | 0,45 / 1,40 / 1,80 / 3,02 | 0,44 / 1,13 | 0,73 / 1,37 | 65% / 66% |
| vsync acceso (Fifo) | 239 | 4,17 / 4,57 / 8,34 / 12,61 | 0,237 | 3,94 / 4,41 / 7,90 / 12,09 | 0,79 / 1,34 | 3,01 / 4,22 | 7% / 10% |
| vsync spento, limite 240 fps | 240 | 4,17 / 4,55 / 4,63 / 4,66 | 0,101 | 3,97 / 4,42 / 4,79 / 6,48 | 0,76 / 1,44 | 0,54 / 4,58 | 12% / 14% |
| vsync spento, limite 480 fps | 480 | 2,08 / 2,44 / 2,58 / 2,62 | 0,087 | 1,85 / 2,39 / 2,72 / 3,41 | 0,74 / 1,43 | 0,61 / 2,26 | 46% / 25% |
| vsync spento, pannello F3 aperto | 2715 | 0,33 / 1,32 / 1,83 / 3,04 | 0,185 | 0,43 / 1,44 / 1,89 / 3,10 | 0,43 / 1,11 | 0,85 / 1,38 | 68% / 67% |

Sul PC girano anche altri programmi, quindi le code cambiano da un giro all'altro: count→present vecchio a vsync
spento ha avuto mediana 0,43-0,45 ms e 99° percentile 1,20-1,40 ms nei due giri da 15 s di oggi (`bench-15s.md`, e
`bench-presentmon.md` con PresentMon acceso), e mediana fino a 0,51 ms e 99° fino a 2,16 ms nei giri corti di prova
da 2-3 s.
Il file completo di ogni giro sta in `misure\` (ignorata da git).

Consumi fuori dal giro: menu fermo 0,1% di CPU e nessun frame dopo il primo; menu con il mouse che si muove sopra
222 frame al secondo (al massimo uno per refresh dello schermo) e 10-18% di CPU; riepilogo e pausa sotto l'1%;
finestra ridotta a icona 0,1% e nessun frame. Nel giro con PresentMon del 21/11, con il mouse vero di Carlo che si
muoveva sopra il menu, il menu era arrivato a 1214 frame al secondo e al 31% di CPU: qualcosa chiedeva ridisegni
oltre il limite. La causa precisa non l'ho riprodotta col banco; ora il frame del menu parte solo da
`RedrawRequested` e lì c'è il limite di uno per refresh, quindi nessuna richiesta, di Windows o di egui, lo può
superare. Il banco lo controlla nella fase "menu, mouse in movimento": oltre il 105% della frequenza dello schermo
la fase è NON VALIDA. CPU del processo, senza il
thread che simula il mouse, sul tempo della fase (100% = un core intero), misurata in due modi perché nessuno è
esatto da solo: a tempo (`GetProcessTimes`, aggiornato a scatti di 15,6 ms: con il limite a 480 fps dà 46% contro
25%) e a cicli (`QueryProcessCycleTime`, convertiti con la frequenza calibrata a pieno carico, quindi un po' più
bassa del vero se il core rallenta).

Allocazioni nel frame del giro, da `get_current_texture` al ritorno di `present`: 0 del codice di aimpro e 83 dentro
wgpu (vista della texture, encoder, render pass, submit, present), contate dal banco con un allocatore che conta.
Col pannello F3 aperto se ne aggiungono circa 1,5 per frame in media: una dentro il disegno di egui e il resto nel
pannello, che si ricalcola 4 volte al secondo dopo il present.

Cosa costa il limite di fps: il movimento più recente arriva a `present` in circa 0,75 ms con qualunque limite
(0,44 senza), ma un evento può aspettare fino a un frame intero prima di essere letto, quindi il ritardo del primo
evento del frame cresce con il periodo: 0,45 ms senza limite, 1,85 a 480 fps, 3,97 a 240 fps. In cambio la CPU
scende da circa il 65% al 25% (480 fps) o al 12-14% (240 fps). Senza limite si gioca con il ritardo più basso; il
limite serve a chi vuole consumare meno. Col vsync il ritardo misurato fino a `present` è simile a quello del limite
a 240 fps, ma col vsync il frame aspetta poi in coda il refresh dello schermo: quella parte la vede solo PresentMon.

### Da PresentMon (22/11/2026, PresentMon 2.6.0 insieme a `aimpro --bench --secondi 15`)

PresentMon (Intel, open source) legge gli eventi di Windows e vede cosa succede dopo `present`: quando l'immagine
arriva davvero sullo schermo e quanto lavora la scheda video. Va lanciato come amministratore:
`strumenti\presentmon.ps1` scarica la 2.6.0 dalla release ufficiale, ne controlla la firma (deve essere di Intel),
lo fa girare insieme al banco e salva anche i suoi messaggi in `misure\`; poi
`python strumenti\presentmon_fasi.py misure\presentmon.csv misure\bench-presentmon.fasi.csv` fa il riassunto per
fase (ogni riga va nella fase in cui è avvenuto il present, `TimeInQPC`). Nel giro di misura tutte le fasi del
banco sono risultate valide.

Tempi in ms, mediana / 99° percentile. "present→schermo" è `MsUntilDisplayed`, dalla chiamata a `present` al
momento in cui l'immagine entra nello schermo; GPU = somma di `MsGPUBusy` sulla durata della fase.

| fase | present al secondo | modalità | present→schermo | GPU |
|---|---|---|---|---|
| vsync spento, senza limite | 1911 | Hardware: Independent Flip | 0,18 / 1,15 | 47% |
| vsync acceso (Fifo) | 239 | Hardware Composed: Independent Flip | 4,00 / 4,05 | 25% |
| vsync spento, limite 240 fps | 240 | Hardware: Independent Flip | 0,74 / 3,09 | 24% |
| vsync spento, limite 480 fps | 475 | Hardware: Independent Flip | 0,25 / 2,04 | 21% |
| vsync spento, pannello F3 aperto | 1901 | Hardware: Independent Flip | 0,16 / 4,68 | 45% |
| menu, mouse in movimento | 221 | Hardware: Independent Flip | 0,25 / 3,44 | 12% |

A vsync spento la presentazione è "Independent Flip" in hardware: l'immagine va allo schermo senza passare dal
compositore di Windows, e arriva in 0,18 ms di mediana. Col vsync aspetta il refresh, circa 4 ms in più a 240 Hz.
A vsync spento, nello stesso giro, il programma ha contato 2676 frame al secondo e PresentMon 1911 present al
secondo: a queste frequenze PresentMon non registra tutti gli eventi (dedotto dal confronto), quindi i suoi
percentili a vsync spento si basano su una parte dei frame. Col vsync e col limite a 240 fps i due conti
coincidono; col limite a 480 fps sono 475 contro 480.

Il ritardo dal mouse all'immagine, sommando le mediane (dedotto, non misurato in un pezzo solo): count→present del
banco più present→schermo di PresentMon, cioè circa 0,6 ms a vsync spento (0,43 + 0,18) e circa 7,9 ms col vsync
(3,92 + 4,00). Restano fuori la scansione dello schermo (fino a 4,17 ms dall'alto in basso a 240 Hz) e il tempo
interno del monitor e del mouse: quelli si misurano solo da fuori, per esempio filmando mouse e schermo con lo slow
motion del telefono.

Limite dichiarato: la latenza input→fotoni e clic→fotoni di PresentMon (`MsAllInputToPhotonLatency`,
`MsClickToPhotonLatency`) col mouse simulato dal banco non si misura. PresentMon la ricava dagli eventi di input
che Windows registra per la finestra, e i movimenti mandati con `SendInput` e letti dal Raw Input non ci finiscono:
nel giro di misura c'è un solo campione, nel riscaldamento, su circa 77 000 frame registrati fra riscaldamento e
fasi di giro (nel primo giro del 21/11, a vsync spento, erano 552 su 28 634, con valori incoerenti fino a
centinaia di ms per i clic). Per questo il ritardo totale qui sopra è una somma; una misura fino ai
fotoni in un pezzo solo richiede il mouse vero.

## Come si consegna

Il percorso è quello di casa (`~/.claude/skills/start/repo.md`): un worktree per task da `origin/main`, PR su
`main`, revisione prima del primo push, CI verde, merge con squash, release e verifica dal vivo.

- **CI** (`.github/workflows/ci.yml`): su ogni PR e su `main`, su `windows-latest`: `cargo fmt --check`, clippy con
  `-D warnings`, `cargo test`, build release; l'.exe e il suo SHA-256 come artefatto.
- **Release**: dopo ogni merge su `main` il job `release` pubblica su GitHub una release `v<versione>-build.<numero>`
  con `aimpro.exe` e `aimpro.exe.sha256`. Si verifica dalla pagina delle release, scaricando l'.exe e avviandolo.
  L'eseguibile non è firmato: Windows mostra l'avviso SmartScreen ("Ulteriori informazioni", poi "Esegui comunque").
- **Commit**: con l'indirizzo noreply di GitHub, mai l'email di Carlo (il repo è pubblico):
  `git -c user.email=243202532+Carlo-Falanga@users.noreply.github.com commit ...`, e nel merge squash
  `gh pr merge <PR> --squash --delete-branch --author-email 243202532+Carlo-Falanga@users.noreply.github.com`.

Il repo è pubblico: niente segreti, chiavi o dati personali nei file e nei commit.
