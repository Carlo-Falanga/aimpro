# Misura di aimpro con PresentMon (Intel, open source): la latenza fino allo schermo, che il programma da solo
# non vede. Va lanciato come amministratore: PresentMon legge gli eventi ETW di Windows.
#
#   powershell -ExecutionPolicy Bypass -File strumenti\presentmon.ps1
#
# 1. scarica PresentMon-2.6.0-x64.exe (circa 1 MB) dalla release ufficiale GameTechDev/PresentMon su GitHub, in
#    misure\presentmon\, e controlla la firma digitale: se non è valida si ferma;
# 2. avvia PresentMon solo su aimpro.exe, con gli istanti QPC (per tagliare le fasi del banco);
# 3. avvia aimpro --bench (circa due minuti a schermo intero: non toccare mouse e tastiera);
# 4. a fine banco PresentMon si chiude da solo. In misure\ restano presentmon.csv, bench-presentmon.md e
#    bench-presentmon.fasi.csv, più l'uscita di PresentMon (presentmon-uscita.txt, presentmon-errori.txt); il
#    riassunto per fase lo fa strumenti\presentmon_fasi.py.
param(
    [string]$Exe = (Join-Path $PSScriptRoot "..\target\release\aimpro.exe"),
    [string]$Misure = (Join-Path $PSScriptRoot "..\misure"),
    [int]$Secondi = 15
)
$ErrorActionPreference = "Stop"
# Senza amministratore PresentMon esce subito con il codice 6 ("failed to start trace session: access denied").
$io = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $io.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Serve PowerShell come amministratore: Start, scrivi PowerShell, tasto destro, Esegui come amministratore."
}
$cartellaPm = Join-Path $Misure "presentmon"
$pm = Join-Path $cartellaPm "PresentMon-2.6.0-x64.exe"
New-Item -ItemType Directory -Force $cartellaPm | Out-Null

if (-not (Test-Path $pm)) {
    Write-Host "Scarico PresentMon 2.6.0 dalla release ufficiale..."
    # Prima in un file temporaneo: uno scaricamento interrotto non deve sembrare completo al giro dopo.
    $parziale = "$pm.parziale"
    Invoke-WebRequest -UseBasicParsing -OutFile $parziale `
        "https://github.com/GameTechDev/PresentMon/releases/download/v2.6.0/PresentMon-2.6.0-x64.exe"
    Move-Item -Force $parziale $pm
}
$firma = Get-AuthenticodeSignature $pm
Write-Host "Firma: $($firma.Status), $($firma.SignerCertificate.Subject)"
if ($firma.Status -ne "Valid" -or $firma.SignerCertificate.Subject -notmatch "Intel") {
    Remove-Item -Force $pm
    throw "PresentMon non è firmato da Intel con una firma valida: l'ho cancellato e mi fermo."
}
if (-not (Test-Path $Exe)) { throw "Manca ${Exe}: prima cargo build --release" }

$csv = Join-Path $Misure "presentmon.csv"
$bench = Join-Path $Misure "bench-presentmon.md"
$log = Join-Path $Misure "presentmon-uscita.txt"
$logErr = Join-Path $Misure "presentmon-errori.txt"
Remove-Item -ErrorAction SilentlyContinue $csv, $bench, $log, $logErr
# L'uscita di PresentMon va in due file, così un errore si legge anche dopo.
$p = Start-Process -FilePath $pm -PassThru -NoNewWindow -RedirectStandardOutput $log -RedirectStandardError $logErr `
    -ArgumentList @(
        "--process_name", "aimpro.exe", "--output_file", "`"$csv`"", "--qpc_time", "--no_console_stats",
        "--terminate_on_proc_exit", "--stop_existing_session", "--session_name", "aimpro_misura"
    )
$null = $p.Handle  # senza leggere l'handle, ExitCode resta vuoto
Start-Sleep -Seconds 3
if ($p.HasExited) {
    Get-Content -ErrorAction SilentlyContinue $log, $logErr | Write-Host
    throw "PresentMon si è chiuso subito (codice $($p.ExitCode)). Il suo messaggio è qui sopra e in $logErr."
}
Write-Host "Avvio aimpro --bench: circa due minuti a schermo intero, non toccare mouse e tastiera."
$a = Start-Process -FilePath $Exe -PassThru -ArgumentList @("--bench", "--secondi", "$Secondi", "--uscita", "`"$bench`"")
$null = $a.Handle  # senza leggere l'handle, ExitCode resta vuoto
$a.WaitForExit()
$p.WaitForExit(30000) | Out-Null
if (-not $p.HasExited) { Stop-Process $p }
$righe = if (Test-Path $csv) { (Get-Content $csv | Measure-Object -Line).Lines } else { 0 }
Write-Host "Fatto. aimpro: uscita $($a.ExitCode). PresentMon: $righe righe in $csv"
Read-Host "Premi Invio per chiudere"
