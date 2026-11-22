# aimpro

Un aim trainer per Windows pensato per chi gioca a livello competitivo, con la latenza fra mouse e schermo più bassa
possibile, misurata e non stimata.

È un programma nativo in Rust: il mouse arriva dal Raw Input di Windows (count grezzi, senza accelerazione), la
grafica va su DirectX 12 con il vsync spento di default e lo schermo intero esclusivo.

## Il primo scenario

Scatto: 30 secondi, una sfera alla volta in un punto casuale della stanza. Colpita, ne compare un'altra; ogni clic a
vuoto è un errore. Alla fine il riepilogo con colpi, errori, precisione e tempo medio per bersaglio.

La sensibilità si imposta come in CS2 o in Valorant, con il FOV e i DPI del mouse: i gradi per count sono quelli del
gioco scelto. Le impostazioni restano in `%APPDATA%\aimpro\config.toml`.

Tasti: Esc mette in pausa, F3 apre il pannello della latenza (frame time, jitter, ritardo fra mouse e presentazione).

## Scaricare

L'ultima versione è nelle [release](https://github.com/Carlo-Falanga/aimpro/releases): `aimpro.exe`, con il suo
SHA-256. L'eseguibile non è firmato, quindi Windows mostra l'avviso SmartScreen: "Ulteriori informazioni", poi
"Esegui comunque".

## Compilare

Serve Rust (la versione la fissa `rust-toolchain.toml`):

```
cargo run --release
cargo run --release -- --bench    # misura la latenza da solo ed esce
```
