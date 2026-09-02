@echo off
rem The piano, alone, in its own window: rebuilds only the Concert Grand
rem crate (native, no wasm, no package) and launches the lab, replacing the
rem one already running. Edit the code or the tuning file, then run this.
rem   lab              -> build and open the piano (PRO X output, first MIDI port)
rem   lab stop         -> close it
rem   lab --midi kl    -> any lab option is passed through
if "%~1"=="stop" (
    cargo +1.98.0-x86_64-pc-windows-msvc run --release -p rackforge-concert-grand --example lab -- --stop
    exit /b
)
cargo +1.98.0-x86_64-pc-windows-msvc run --release -p rackforge-concert-grand --example lab -- --out "PRO X" %*
