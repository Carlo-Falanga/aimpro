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
| serde_json | 1.0.154 | solo per leggere la risposta delle API di GitHub nel controllo degli aggiornamenti |
| WinHTTP e CNG di Windows | crate `windows` 0.62.2 | la rete (una GET, nel thread dell'aggiornamento) e lo SHA-256 (`BCryptHash`): niente librerie di rete né di crittografia in più |

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
  `present`. Nel menu, in pausa e nel riepilogo non c'è ciclo: si ridisegna solo su evento, al massimo 60 volte al
  secondo (scelta di Carlo, 22/11; col menu non in primo piano solo se ci passa sopra il mouse). Finestra non in primo piano o ridotta: il
  giro va in pausa e non si disegna. Il pannello F3 si ricalcola dopo `present`, sugli ultimi 4096 campioni.
- `grafica.rs` e `scena.wgsl`: stanza a griglia, sfere disegnate per pixel (raggio contro sfera, fino a 8 istanze in
  una draw), mirino, tempo, conto alla rovescia e punteggio dal vivo come rettangoli istanziati, riscritti solo
  quando cambiano. I dati del frame (visuale, tempo di gioco, schermo: 128 byte) vanno nella lista dei comandi come
  costanti immediate (root constants di DX12), senza `write_buffer`. Il moto delle sfere lo calcola anche lo shader:
  ogni sfera ha un tratto (angoli e velocità di partenza, istante) e un istante di comparsa; il buffer delle sfere
  si riscrive solo quando un tratto cambia (un colpo, un cambio di direzione), non a ogni frame.
- `scenari.rs` e `scenari/*.toml`: gli scenari (formato qui sotto). Quelli di aimpro sono dentro l'.exe
  (`include_str!`), quelli di chi gioca in `%APPDATA%\aimpro\scenari\`.
- `giro.rs`, `camera.rs`, `sensibilita.rs`, `config.rs`: il giro (sfere, moto, colpi, tracking, conto alla
  rovescia), la visuale, la conversione delle sensibilità (CS2 0,022 gradi per count, Valorant 0,07; FOV di CS2
  orizzontale su 4:3, di Valorant su 16:9) e le impostazioni (con i preferiti e il primo avvio). Gli scenari durano
  30 secondi (lo standard scelto da Carlo il 22/11/2026). Record e statistiche sono per id di scenario: uno scenario
  con un'altra durata ha un altro id. Ogni clic si valuta con la visuale e con le posizioni delle sfere del suo
  istante; il tracking somma il tempo con il mirino su una sfera e il tasto sinistro giù, fra un evento del tasto e
  l'altro e a ogni frame.
- `home.rs`: la home (barra Gioca, Libreria, Playlist, Statistiche, Impostazioni), la libreria con ricerca e filtri,
  la pagina dello scenario e la pagina del primo avvio. Ogni pagina è una funzione: Statistiche e Impostazioni
  chiamano quelle di `interfaccia.rs`, Playlist la sua in `home.rs`.
- `registro.rs`: per ogni bersaglio colpito il tempo dal colpo prima (con una sfera sola, o con l'attesa di
  comparsa, dalla comparsa; pause tolte), la distanza angolare dal
  precedente (per il primo, dalla visuale di partenza) e gli overshoot (il mirino entra nella sfera e ne esce senza
  colpirla, controllato a ogni clic con la visuale del suo istante e a ogni frame dopo il movimento: un passaggio
  che entra ed esce dentro lo stesso frame non si vede, e col limite a 240 fps un frame dura 4,17 ms). Legge il giro
  solo dalla sua parte pubblica; la memoria (1024 bersagli) si prepara all'avvio, nel frame non alloca.
- `storico.rs`, `esporta.rs`: lo storico dei giri (formato qui sotto), il record per scenario, la sessione, gli
  ultimi giocati e i record recenti della home, e l'esportazione CSV. La pagina Statistiche sta in `interfaccia.rs`,
  col grafico disegnato dal painter di egui.
- `latenza.rs`, `misure.rs`, `banco.rs`, `allocazioni.rs`: le misure, il pannello F3 e il banco `--bench`.
- `aggiornamento.rs`, `rete.rs`: l'aggiornamento dall'app (qui sotto).

## L'aggiornamento dall'app

- **Il numero di build** lo scrive la CI dentro l'.exe: `AIMPRO_BUILD` = `github.run_number`, letto con
  `option_env!`, lo stesso numero del tag `v<versione>-build.<numero>` della release. Il menu lo mostra in fondo
  ("build 13"); una build locale non ha numero ("build locale") e non controlla niente.
- **Il controllo**: all'avvio, se "Aggiornamenti: controlla all'avvio" è acceso (lo è di base; un `config.toml`
  di prima senza il campo `controlla_aggiornamenti` lo prende acceso) e non è il banco, un thread fa una GET con
  WinHTTP a `https://api.github.com/repos/Carlo-Falanga/aimpro/releases/latest`, senza token e con il solo
  `User-Agent: aimpro/<versione>` (proxy di sistema, timeout 10 s, letture 30 s). Se la build del tag è più nuova e
  la release ha `aimpro.exe` e `aimpro.exe.sha256`, il menu mostra "Aggiornamento disponibile" con le due build e
  "Aggiorna e riavvia". Senza rete, con un errore delle API o all'ultima versione non compare niente. L'avvio non
  aspetta il thread: lo sveglia un evento del ciclo (`EventLoopProxy`) quando ha qualcosa da mostrare.
- **"Aggiorna e riavvia"**: un thread scarica `aimpro.exe.sha256` e `aimpro.exe` (al più 64 MB; il menu mostra i MB
  e la percentuale), controlla lo SHA-256 e l'intestazione `MZ`, scrive `<exe>.nuovo` accanto all'.exe in uso,
  rinomina l'.exe in uso in `<exe>.vecchio` (un .exe in uso si può rinominare, non cancellare) e mette il nuovo al
  suo posto; poi il thread principale salva le impostazioni, avvia il nuovo con gli stessi argomenti ed esce. Vale
  per qualunque nome abbia l'.exe (anche `aimpro (1).exe`). All'avvio dopo, un thread cancella `<exe>.vecchio`
  (riprova per 10 s: il processo di prima può essere ancora in chiusura). Ogni errore (rete, download interrotto,
  SHA diverso, cartella non scrivibile) lascia l'.exe di prima e nel menu "Aggiornamento non riuscito", il motivo
  e "Riprova". Durante il download "Inizia" è spento. Il nuovo .exe si scrive con `sync_all` prima delle rinomine, e
  chi chiude aimpro mentre le rinomine sono a metà aspetta che finiscano (dopo, non ne partono altre). Se il nuovo non riparte da solo, il menu
  dice "Build N installata" e di riaprire aimpro, senza "Riprova".
- **Mai durante il giro**: entrando nel giro (`cursore_di_gioco`, anche alla ripresa dalla pausa) la richiesta in
  corso si chiude subito con `WinHttpCloseHandle` (anche a metà di nome, connessione, TLS o attesa della risposta) e
  nessuna ne parte finché si gioca; il controllo la rifà quando si esce dal giro (al più 5 volte). Nel frame non c'è
  niente dell'aggiornamento. Il banco non va in rete. Provato col finto GitHub in modo `lento`: le richieste
  iniziate prima di un giro si chiudono quando il giro parte, e la successiva riesce a giro finito.
- **La prima volta**: le release pubblicate prima di questa funzione non hanno il numero dentro e non si
  aggiornano da sole: la prima release con questa funzione va scaricata a mano una volta.

Come si prova senza toccare le release vere: si compila con una build finta e con le API puntate al finto GitHub
locale, e si avvia in una cartella di prova con `APPDATA` di prova:

```
AIMPRO_BUILD=5 AIMPRO_API_PROVA=http://127.0.0.1:8765/latest.json cargo build --release
python strumenti/finto_github.py <un aimpro.exe da servire> sha-sbagliato   # o giusto, exe-404, interrotto
```

`AIMPRO_API_PROVA` esiste solo nelle build che lo ricevono al momento della compilazione: le release non ce l'hanno
e accettano solo indirizzi `https`. Con il solo `AIMPRO_BUILD=5` la prova va contro le release vere (scarica
l'ultima).

## Gli scenari

La libreria di aimpro ha 26 scenari originali in 5 categorie, tutti da 30 secondi: clic su bersagli fermi (scatto,
pianura, ventaglio, altura, orizzonte), clic su bersagli in movimento (deriva, corrente, rapide, scarto, vortice),
tracking (nastro, filo, binario, guizzo, lucciola, raffica), cambio di bersaglio (coppia, terna, costellazione,
sciame, stormo), precisione e reazione (chicco, spillo, cruna, riflesso, sentinella). KovaaK's e Aim Lab sono solo
l'ispirazione del genere: nomi, parametri e descrizioni sono nostri.

Uno scenario è un file TOML. Per aggiungerne uno basta metterlo in `%APPDATA%\aimpro\scenari\` (al più 200 file
`.toml`, letti all'avvio in ordine di nome): compare nella libreria, nella sua categoria. Un file rovinato o con un
id già usato si salta, e la home lo dice in una riga con il nome del file e il motivo (al più 3 righe e il numero
degli altri); non blocca niente.

```toml
formato = 1                       # versione del formato; un numero più alto si salta
id = "mio"                        # lettere, cifre, - e _ (al più 40): record e statistiche sono per id
nome = "Il mio scenario"          # al più 40 caratteri
categoria = "cambio"              # fermi, movimento, tracking, cambio, precisione; se manca, dalle regole
difficolta = "media"              # facile, media (se manca), difficile, estrema
descrizione = "Due sfere ferme."  # al più 200 caratteri
durata_s = 30.0                   # 5-300, 30 se manca (lo standard)
punteggio = "colpi"               # "colpi" o "tracking"

[bersagli]
numero = 2                        # 1-8 sfere in campo insieme
raggio = 0.4                      # metri, 0,05-2
distanza = 12.0                   # metri dall'occhio, 3-16 (12 se manca)
distanza_minima_gradi = 10.0      # fra le sfere e dalla sfera di prima, 0-40
ritardo_min_ms = 0                # attesa a caso prima che una sfera nuova compaia (reazione), 0-10000
ritardo_max_ms = 0

[zona]                            # dove nascono le sfere, in gradi dal centro
imbardata_gradi = 40.0            # 0-80, a destra e a sinistra
beccheggio_min_gradi = -10.0      # -45-45
beccheggio_max_gradi = 20.0

[movimento]                       # se manca, le sfere stanno ferme
tipo = "lineare"                  # "fermo", "lineare" (rimbalza ai bordi della zona) o "casuale"
velocita_gradi_s = 12.0           # 0-180
cambio_min_ms = 400               # solo per "casuale": ogni quanto cambia direzione, 50-10000
cambio_max_ms = 900
```

Campi sconosciuti o valori fuori dai limiti rendono il file rovinato (l'avviso dice la riga o il campo); anche una
sfera troppo grande per la sua distanza (il raggio, al 200%, non può superare un quarto della distanza). Se la zona
non ha altezza (o larghezza) tutta la velocità va sull'altro asse. Nei giri
"colpi" ogni clic sulla sfera guardata è un colpo e ogni clic a vuoto un errore; con più sfere conta quella col
centro più vicino al mirino. Nei giri "tracking" il punteggio è la percentuale del giro con il mirino su una sfera e
il tasto sinistro premuto (scelta di Carlo, 25/11/2026). Con un'attesa di comparsa il tempo medio del riepilogo è
quello dalla comparsa al colpo.

Nella pagina dello scenario si cambiano dimensione e velocità dei bersagli (50-200%, uguali per tutti gli
scenari): con parametri diversi dal 100% il giro si salva ma non fa record, e riepilogo e pagina dello scenario lo
dicono (scelta di Carlo, 25/11/2026). Negli scenari con le sfere ferme la velocità non conta: il giro resta standard.

Il giro parte con un conto alla rovescia di 3 secondi (le sfere non ci sono ancora), che si salta con un clic o con
lo spazio; nel conto non si contano overshoot. R ricomincia subito lo stesso scenario dal giro, dalla pausa o dal
riepilogo (il giro interrotto non si salva). Riprendi e Rigioca rispondono solo dopo 400 ms (scelta di Carlo,
25/11/2026): un clic di troppo dal giro non li preme. Nel tracking, dopo R o alla ripresa dalla pausa, il tasto
sinistro tenuto giù conta solo dopo averlo rilasciato e ripremuto: il Raw Input dice quando si preme, non se è
già giù. Punteggio e precisione dal vivo stanno in alto a sinistra (nel tracking la percentuale del tempo giocato
finora); si spengono in Impostazioni.

## La home

All'avvio la home: a sinistra la barra Gioca, Libreria, Playlist, Statistiche, Impostazioni. Gioca mostra lo scenario
scelto, gli ultimi giocati, i preferiti e i record recenti (dallo storico, appena il thread che lo legge ha finito).
La libreria divide gli scenari per categoria, con ricerca per nome e descrizione (senza badare a maiuscole e
accenti) e filtri per categoria, difficoltà e preferiti. La pagina dello scenario ha descrizione, record, ultimi
punteggi, preferito, parametri, Statistiche e Gioca. I preferiti stanno in `config.toml` (`preferiti`, una lista di
id). Al primo avvio, senza `config.toml`, una pagina sola chiede gioco, sensibilità e DPI; poi
`primo_avvio_fatto = true`, e un `config.toml` di prima senza il campo la salta.

## Lo storico dei giri

Un file TOML per giro in `%APPDATA%\aimpro\storico\`, scritto in `fine_giro` dopo il present dell'ultimo frame
(prima in `<nome>.toml.nuovo`, poi rinominato). Il nome è `AAAAMMGG-hhmmss-mmm-<scenario>.toml` (ora locale; se
esiste già, `-2`, `-3`…; dell'id nel nome restano solo lettere e cifre ASCII e `-`, il resto diventa `_`). Il banco non
scrive nello storico.

```toml
formato = 2                     # versione del formato; un file con un numero più alto si salta
scenario = "scatto"             # id dello scenario: record e statistiche sono per id
data = "2026-11-22 14:00:34"    # ora locale di fine giro
istante_ms = 1795352434530      # ms dal 1970 in UTC: l'ordine dei giri
durata_s = 30.0
colpi = 88
errori = 0
precisione = 1.0                # colpi / clic, fra 0 e 1; manca se non ci sono clic
tempo_medio_ms = 338.28         # manca se non ci sono colpi
overshoot = 89                  # tutti, anche sul bersaglio rimasto in campo alla fine
punteggio = 88.0                # i colpi, o nel tracking la percentuale (0-100); nel formato 1 manca: sono i colpi
tracking = 0.62                 # solo negli scenari di tracking: la frazione del giro sulla sfera, tasto premuto

[impostazioni]                  # quelle con cui si è giocato il giro
gioco = "cs2"                   # "cs2" o "valorant"
sensibilita = 1.25
cm_360 = 41.56
fov = 90.0
dpi = 800.0
vsync = false
limite_fps = 0                  # 0 = nessun limite
versione = "0.1.0"              # versione di aimpro

[parametri]                     # dimensione e velocità dei bersagli; nel formato 1 manca: standard
dimensione = 1.0
velocita = 1.0

[[bersagli]]                    # uno per bersaglio colpito, in ordine (al più 1024)
tempo_ms = 297.44               # dal colpo prima (con una sfera sola, dalla comparsa)
distanza_gradi = 24.78          # dal bersaglio precedente
overshoot = 1
```

All'avvio un thread legge tutti i file, mentre la finestra si apre (il primo che serve lo storico, a fine giro o
nelle Statistiche, lo aspetta); uno rovinato (o di un formato più nuovo, o senza i campi obbligatori) si salta e
si conta, e la schermata Statistiche lo dice. I file del formato 1 si leggono ancora. In memoria i giri restano senza
i bersagli. Il record di uno scenario è il punteggio del suo giro migliore fra quelli coi parametri standard; "nuovo
record" quando un giro lo supera (pareggiarlo no; il primo giro di uno scenario è record se ha punteggio più di
zero). Uno scenario nuovo usa un suo `scenario = "<id>"` e gli stessi campi; campi in più vanno
aggiunti con un valore predefinito (`#[serde(default)]`), così i file vecchi si leggono ancora; un cambio che i
file vecchi non reggono alza `formato`.

"Esporta CSV" scrive `aimpro-giri-AAAAMMGG-hhmmss.csv` nella cartella `aimpro` dei Documenti di Windows (quella vera,
anche se spostata, da `SHGetKnownFolderPath`), una riga per giro. Per Excel in italiano: separatore `;`, decimali
con la virgola, data `GG/MM/AAAA hh:mm:ss`, UTF-8 con il BOM, righe chiuse da CRLF.

## Come si prova

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                      # 143 test: scenari, giro, tracking, shader, home, sensibilità, impostazioni, menu, visuale, registro, storico, CSV, misure, tempo, aggiornamento
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

Consumi fuori dal giro (21/11; quelli dopo la task dei consumi sono nella tabella "Consumi prima e dopo" qui
sotto): menu fermo 0,1% di CPU e nessun frame dopo il primo; menu con il mouse che si muove sopra 222 frame al
secondo (allora al massimo uno per refresh dello schermo) e 10-18% di CPU; riepilogo e pausa sotto l'1%;
finestra ridotta a icona 0,1% e nessun frame. Nel giro con PresentMon del 21/11, con il mouse vero di Carlo che si
muoveva sopra il menu, il menu era arrivato a 1214 frame al secondo e al 31% di CPU: qualcosa chiedeva ridisegni
oltre il limite. La causa precisa non l'ho riprodotta col banco; ora il frame del menu parte solo da
`RedrawRequested` e lì c'è il limite (dal 22/11 60 frame al secondo), quindi nessuna richiesta, di Windows o di
egui, lo può superare. Il banco lo controlla nella fase "menu, mouse in movimento": oltre il 105% del limite la fase
è NON VALIDA. CPU del processo, senza il
thread che simula il mouse, sul tempo della fase (100% = un core intero), misurata in due modi perché nessuno è
esatto da solo: a tempo (`GetProcessTimes`, aggiornato a scatti di 15,6 ms: con il limite a 480 fps dà 46% contro
25%) e a cicli (`QueryProcessCycleTime`, convertiti con la frequenza calibrata a pieno carico, quindi un po' più
bassa del vero se il core rallenta).

Allocazioni nel frame del giro, da `get_current_texture` al ritorno di `present`: 0 del codice di aimpro e 68 dentro
wgpu (vista della texture, encoder, render pass, submit, present; erano 83 prima della task dei consumi del 22/11),
contate dal banco con un allocatore che conta.
Col pannello F3 aperto se ne aggiungono circa 1,5 per frame in media: una dentro il disegno di egui e il resto nel
pannello, che si ricalcola 4 volte al secondo dopo il present.

Cosa costa il limite di fps (banco del 22/11 dopo la task dei consumi, `misure\dopo-2.md`): il movimento più
recente arriva a `present` in circa 0,72 ms con qualunque limite (0,37 senza), ma un evento può aspettare fino a un
frame intero prima di essere letto, quindi il ritardo del primo evento del frame cresce con il periodo: count→present
vecchio, mediana / 99°, 0,37 / 0,96 ms senza limite, 1,83 / 2,84 a 480 fps, 3,90 / 4,38 a 240 fps. In cambio la CPU
(a cicli) scende da circa il 64% di un core al 21% (480 fps) o al 12% (240 fps). Senza limite si gioca con il
ritardo più basso; il limite serve a chi vuole consumare meno. Col vsync il ritardo misurato fino a `present` è
simile a quello del limite a 240 fps (3,93 / 4,37 ms, 8,5% di CPU), ma col vsync il frame aspetta poi in coda il
refresh dello schermo: quella parte la vede solo PresentMon (4,00 ms in più, tabella qui sotto). I predefiniti
(vsync spento, nessun limite) restano una scelta di Carlo.

### Consumi prima e dopo (22/11/2026, task dei consumi)

Due banchi completi di fila, `--secondi 15`, tutte le fasi valide: "prima" è `main` dopo la task delle statistiche
con solo la misura della memoria aggiunta al banco (`misure\prima-2.md`), "dopo" il branch della task
(`misure\dopo-2.md`). Schermo intero senza bordi (le impostazioni di questo PC), 240 Hz. Memoria = set di lavoro
privato a fine fase, la colonna "Memoria" di Gestione attività. CPU = percentuale di un core, a cicli / a tempo
(quella a tempo va a scatti di 15,6 ms e col limite di fps sbaglia molto). In Gestione attività la CPU si divide
per i 16 core logici: 64% di un core è il 4% che si vede lì.

| stato | memoria prima | memoria dopo | CPU prima | CPU dopo | frame al secondo prima / dopo |
|---|---|---|---|---|---|
| menu fermo | 124,8 MB | 57,8 MB | 0,1% / 0,4% | 0,1% / 0,0% | 0 / 0 |
| menu, mouse che si muove sopra | 125,3 MB | 58,3 MB | 10,7% / 19,1% | 4,2% / 2,5% | 221 / 59 |
| pausa | 125,5 MB | 59,3 MB | 0,1% / 0,0% | 0,1% / 0,2% | 0 / 0 |
| riepilogo | 125,5 MB | 59,3 MB | 0,1% / 0,0% | 0,1% / 0,3% | 0 / 0 |
| giro, vsync spento, senza limite | 125,3 MB | 58,3 MB | 67,7% / 64,9% | 64,1% / 64,7% | 2763 / 3128 |
| giro, limite 240 fps | 125,2 MB | 58,2 MB | 13,0% / 0,3% | 12,4% / 7,7% | 240 / 240 |
| giro, limite 480 fps | 125,2 MB | 58,2 MB | 23,6% / 28,6% | 21,4% / 23,5% | 480 / 480 |
| giro, vsync acceso | 125,3 MB | 58,3 MB | 9,0% / 8,2% | 8,5% / 6,8% | 237 / 240 |

Nel giro senza limite, nello stesso giro di misura: count→present vecchio 0,43 / 1,10 / 1,63 / 2,33 ms prima e
0,37 / 0,96 / 1,47 / 2,20 dopo (mediana / 99° / 99,9° / massimo); lavoro del frame (dalla lettura del mouse al
ritorno di `present`) 0,184 ms di mediana dopo (il banco di prima non lo riportava; nei banchi corti era 0,236 ms
prima delle costanti immediate e 0,205 subito dopo); allocazioni dentro wgpu da 83 a 68 per frame, 0 del nostro
codice prima e dopo. La CPU senza limite resta quella: il frame costa meno, quindi se ne fanno di più (3128 al
secondo invece di 2763) e il core resta occupato uguale.

Da dove viene la memoria (tappe dell'avvio nel file del banco, set di lavoro privato):

| tappa | prima | dopo | perché |
|---|---|---|---|
| programma pronto | 4,2 MB | 3,2 MB | gli anelli di attesa e lavoro del frame sono lunghi quanto il pannello F3 (4096 campioni, non 65 536) |
| istanza DirectX 12 e scheda video | +22,2 MB | +20,2 MB | i driver: wgpu apre un dispositivo DX12 su ogni scheda (qui anche la Radeon integrata) per sceglierne una; non si evita da wgpu. I 2 MB in meno non vengono da una modifica, che qui non c'è: è la variazione da un avvio all'altro. Senza le modifiche dei consumi (un banco corto del 22/11 sul codice di prima delle statistiche, che non toccano DirectX) la tappa ha dato +20,7 MB; con le modifiche, sull'.exe della release v0.1.0-build.13, +21,8 MB |
| dispositivo DirectX 12 | +82,2 MB | +21,8 MB | con `MemoryHints::Performance` wgpu crea subito un blocco di memoria visibile alla CPU da 64 MB: con `MemoryUsage` i blocchi partono da 4 MB (-60 MB). L'heap dei descrittori era da 1 000 000 di voci, ora 16 384: -31 MB di memoria impegnata, il set di lavoro non cambia |
| swapchain, pipeline, egui | +3,9 MB | +1,9 MB | nessuna modifica qui: i 2 MB sono la variazione da un avvio all'altro. Senza le modifiche dei consumi, nello stesso banco corto, la tappa ha dato +1,9 MB; con le modifiche, sull'.exe della release, +3,7 MB |
| primo frame del giro e del menu | +10,9 MB | +9,4 MB | l'atlante dei caratteri di egui era largo quanto il lato massimo delle texture (16 384 pixel), ora 2048 (-1,5 MB) |

Dove va la CPU col limite di fps: il lavoro del frame è circa 0,19 ms (5% di un core a 240 fps); il resto è
l'attesa fino alla scadenza. Il timer ad alta risoluzione di Windows su questo PC si sveglia in ritardo a scatti di
circa 0,5 ms (misurato il 22/11 nel programma, due fasi col limite: da un terzo a metà dei risvegli oltre 0,2 ms, uno su otto o
su sei oltre 0,5 ms, fino a 1,1 ms),
quindi l'ultimo tratto, 0,5 ms prima della scadenza, si fa girando a vuoto. Un tratto più corto (provato: adattato
al ritardo visto) qui non scende, perché i ritardi alti sono frequenti; accorciarlo a forza farebbe arrivare più
frame in ritardo (frame pacing peggiore) e resta fuori, perché la latenza e il ritmo dei frame vengono prima. Il
tratto a vuoto costa circa il 7% di un core a 240 fps.

### Scenari prima e dopo (25/11/2026, task degli scenari e della home)

Quattro banchi completi alternati, `--secondi 15`, tutte le fasi valide: "prima" è `main` eb3d118, "dopo" il branch
con scenari, home e punteggio dal vivo acceso. Count→present vecchio nel giro di scatto a vsync spento, senza limite
(mediana / 99° / 99,9°, ms):

| giro | prima | dopo |
|---|---|---|
| primo | 0,33 / 0,58 / 1,04 | 0,33 / 0,73 / 1,29 |
| secondo | 0,33 / 0,75 / 1,31 | 0,33 / 0,70 / 1,33 |

La mediana non cambia e il 99° sta nella stessa variazione da un giro all'altro (0,58-0,75 prima, 0,70-0,73 dopo).
Allocazioni del codice di aimpro nel frame: 0 prima e dopo, in ogni fase; dentro wgpu 68 per frame in tutte e due.
Gli scenari nuovi, a vsync spento: deriva 0,32 / 0,49, filo 0,31 / 0,49, guizzo 0,31 / 0,49, terna 0,32 / 0,50,
sciame 0,32 / 0,49 (mediana / 99°), circa 3900-4000 frame al secondo, 0 allocazioni di aimpro. I file stanno in
`verifiche\aimpro-20261121-scenari\banco\` sul PC di Carlo.

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
- **Build nell'.exe**: il job `controlli` compila con `AIMPRO_BUILD` = numero della run, lo stesso del tag della
  release: è quello che l'app confronta per sapere se c'è un aggiornamento.
- **Release**: dopo ogni merge su `main` il job `release` pubblica su GitHub una release `v<versione>-build.<numero>`
  con `aimpro.exe` e `aimpro.exe.sha256`. Si verifica dalla pagina delle release, scaricando l'.exe e avviandolo.
  L'eseguibile non è firmato: Windows mostra l'avviso SmartScreen ("Ulteriori informazioni", poi "Esegui comunque").
- **Commit**: con l'indirizzo noreply di GitHub, mai l'email di Carlo (il repo è pubblico):
  `git -c user.email=243202532+Carlo-Falanga@users.noreply.github.com commit ...`, e nel merge squash
  `gh pr merge <PR> --squash --delete-branch --author-email 243202532+Carlo-Falanga@users.noreply.github.com`.

Il repo è pubblico: niente segreti, chiavi o dati personali nei file e nei commit.
