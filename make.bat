@echo off
setlocal

if "%~1"=="lint" goto lint
if "%~1"=="test" goto test
if "%~1"=="regen-fixtures" goto regen_fixtures

echo Unknown target: %~1
echo Supported targets: lint, test, regen-fixtures
exit /b 1

:lint
python -m flake8 .
exit /b %errorlevel%

:test
python -m pytest
exit /b %errorlevel%

:regen_fixtures
python scripts\regen_fixtures.py
exit /b %errorlevel%
