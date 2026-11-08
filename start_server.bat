@echo off
rem ============================================
rem  PoP - Protocol: запуск игрового сервера
rem  Использование: start_server.bat [порт]
rem  Порт по умолчанию: 8080
rem ============================================
setlocal
cd /d "%~dp0"

set PORT=%1
if "%PORT%"=="" set PORT=8080

echo [1/2] Проверка сборки...
if not exist "target\release\popproto-server.exe" (
    echo     сборка release ^(первый запуск может занять пару минут^)...
    cargo build --release
    if errorlevel 1 (
        echo     ОШИБКА сборки. Установи Rust: https://rustup.rs
        pause
        exit /b 1
    )
)

echo [2/2] Запуск сервера на http://127.0.0.1:%PORT%
echo     Остановить: Ctrl+C
echo     Игра: http://127.0.0.1:%PORT%
start "" http://127.0.0.1:%PORT%
target\release\popproto-server.exe

pause
