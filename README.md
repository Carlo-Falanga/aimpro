# aimpro

Un aim trainer per Windows pensato per chi gioca a livello competitivo, con la latenza fra mouse e schermo più bassa
possibile, misurata e non stimata.

È un programma nativo in Rust: il mouse arriva dal Raw Input di Windows (count grezzi, senza accelerazione), la
grafica va su DirectX 12 con il vsync spento di default e lo schermo intero esclusivo.

## Il primo scenario

Scatto: 30 secondi, una sfera alla volta in un punto casuale della stanza. Colpita, ne compare un'altra; ogni clic a
vuoto è un errore. Alla fine il riepilogo con colpi, errori, precisione, tempo medio per bersaglio e il confronto
con il tuo record.

## Statistiche

Ogni giro finito resta sul PC, in `%APPDATA%\aimpro\storico\`: un file per giro con i numeri del giro, le
impostazioni usate e, per ogni bersaglio, il tempo per colpirlo, la distanza dal precedente e gli overshoot (le volte
che il mirino è entrato nella sfera e ne è uscito senza colpirla). Dal menu, "Statistiche" ha quattro schede, per
scenario e per periodo (7 giorni, 30 giorni, tutto): l'andamento di punteggio, precisione e tempo medio con la media
mobile e il record; i giorni uno per uno e gli ultimi 7 giorni contro i 7 prima; i punti deboli (direzioni e distanze
più lente, overshoot, giri migliori contro gli altri); l'elenco dei giri, dove un clic apre il dettaglio di un giro
con il tempo di ogni bersaglio.

La sensibilità si imposta come in CS2 o in Valorant, con il FOV e i DPI del mouse: i gradi per count sono quelli del
gioco scelto. Le impostazioni restano in `%APPDATA%\aimpro\config.toml`.

Tasti: Esc mette in pausa, F3 apre il pannello della latenza (frame time, jitter, ritardo fra mouse e presentazione).

## Scaricare

L'ultima versione è nelle [release](https://github.com/Carlo-Falanga/aimpro/releases): `aimpro.exe`, con il suo
SHA-256. L'eseguibile non è firmato, quindi Windows mostra l'avviso SmartScreen: "Ulteriori informazioni", poi
"Esegui comunque".

## Aggiornare

All'avvio aimpro chiede a GitHub se c'è una versione più nuova; se c'è, il menu lo dice e "Aggiorna e riavvia"
scarica il nuovo `aimpro.exe`, ne controlla lo SHA-256, lo mette al posto di quello in uso e lo riapre. Impostazioni
e storico restano. Il controllo si spegne dalle impostazioni ("Aggiornamenti: controlla all'avvio"); la richiesta
a GitHub non manda niente di tuo, solo il nome e la versione di aimpro. Le versioni uscite prima non sanno
aggiornarsi: la prima volta si scarica a mano.

## Compilare

Serve Rust (la versione la fissa `rust-toolchain.toml`):

```
cargo run --release
cargo run --release -- --bench    # misura la latenza da solo ed esce
```
