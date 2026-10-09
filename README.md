# MetaXtract

MetaXtract는 로컬 파일에서 메타데이터와 SHA-256을 수집하고, 결과를 리포트하거나
검증 가능한 케이스 ZIP으로 묶는 Python 도구입니다. 모든 분석은 로컬에서 수행되며
CLI와 데스크톱 GUI를 제공합니다.

## 주요 기능

- 파일·디렉터리 스캔과 JSONL 출력
- 이미지, PDF, DOCX, 비디오 메타데이터 정규화
- JSON·HTML 리포트와 두 스캔 결과 비교
- 원본 및 케이스 번들의 경로·크기·해시 검증
- 개인정보 필드를 제거한 메타데이터 전용 번들
- 동일 입력에서 동일한 바이트가 생성되는 결정적 ZIP
- 백그라운드 스캔, 진행률, 취소, 오류 표시를 지원하는 GUI

## 요구 사항과 설치

- Python 3.11 이상
- 필수 Python 패키지: Pillow, pypdf, python-docx
- 선택 도구: 비디오 메타데이터 추출용 `ffprobe`

```bash
git clone https://github.com/Koreapanda4444/MetaXtract.git
cd MetaXtract
python -m venv .venv
```

Windows PowerShell:

```powershell
.venv\Scripts\Activate.ps1
python -m pip install --upgrade pip
python -m pip install .
```

Linux 또는 macOS:

```bash
source .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install .
```

설치 상태는 다음 명령으로 확인합니다. `ffprobe`가 없어도 나머지 기능은 사용할 수
있습니다.

```bash
metaxtract doctor
```

## 지원 포맷

| 포맷 | 확장자 | 주요 추출 항목 |
|---|---|---|
| 이미지 | `.jpg`, `.jpeg`, `.png` | 형식, 색상 모드, 크기, 프레임, 기기·렌즈·방향·GPS 등 EXIF |
| PDF | `.pdf` | 암호화 여부, 페이지 수, 제목, 작성자, 주제, 키워드, 생성·수정 정보 |
| DOCX | `.docx` | 문단·표·섹션 수, 제목, 작성자, 수정자, 날짜, 리비전 등 문서 속성 |
| 비디오 | `.mp4`, `.mov`, `.m4v` | 컨테이너, 길이, 비트레이트, 스트림, 코덱, 해상도, 프레임레이트, 오디오 |

그 밖의 파일도 상대 경로, 크기, 수정 시각, MIME 기본값, SHA-256을 기록합니다.

## 빠른 시작

```bash
metaxtract scan evidence --out scan.jsonl
metaxtract report scan.jsonl --out report.json
metaxtract verify scan.jsonl evidence
metaxtract export-case scan.jsonl case.zip --include-files --files-base evidence
metaxtract verify-bundle case.zip
```

정상 완료 시 종료 코드는 `0`, 검증 문제나 명령 실패 시 `1`입니다. 예외 traceback을
노출하는 대신 명령명과 실패 원인을 표준 오류로 출력합니다.

## 명령

### 스캔과 캐시

```bash
metaxtract scan evidence --out scan.jsonl
metaxtract scan evidence --cache off --max-files 1000
metaxtract scan evidence --include-hidden --out scan.jsonl
metaxtract cache purge
```

기본 캐시는 현재 디렉터리의 `.metaxtract_cache/`에 저장됩니다. 콘텐츠 해시는 한 번만
계산하고 캐시 인덱스는 스캔당 한 번만 기록합니다. 기본 파일 한도는 5,000개이며
`.git`, `__pycache__`, `.metaxtract_cache`와 숨김 경로는 제외됩니다. 숨김 경로는
`--include-hidden`으로 포함할 수 있습니다. 심볼릭 링크와 스캔 루트 이탈은 거부합니다.

각 JSONL 레코드는 다음 필드를 가집니다.

```json
{"errors":[],"metadata":{},"mime":"application/octet-stream","path":"evidence.bin","sha256":"…","size_bytes":123,"warnings":[]}
```

### 리포트와 비교

```bash
metaxtract report scan.jsonl --out report.json
metaxtract report scan.jsonl --format html --out report.html
metaxtract diff old.jsonl new.jsonl --out diff.json
```

리포트에는 전체 파일 수, MIME·경고·오류 집계와 GPS, 작성자, 생성 도구, 기기 모델
발견 항목이 포함됩니다. `diff`는 추가·삭제·변경된 상대 경로를 구분합니다.

### 원본 검증

```bash
metaxtract verify scan.jsonl evidence
```

JSONL의 상대 경로, 파일 크기, SHA-256을 현재 원본과 대조합니다. 누락, 변경, 중복,
심볼릭 링크, 기준 디렉터리 이탈을 각각 문제로 보고합니다.

### 케이스 번들

메타데이터 전용 번들:

```bash
metaxtract export-case scan.jsonl case.zip --case-id CASE-001 --notes "Initial scan"
```

원본 포함 번들:

```bash
metaxtract export-case scan.jsonl case.zip \
  --include-files --files-base evidence
```

`--files-base`를 생략하면 `scan.jsonl`이 있는 디렉터리를 사용합니다. 스캔 이후 원본의
크기나 해시가 달라졌거나 경로가 안전하지 않으면 번들을 생성하지 않습니다.

번들은 고정된 멤버 순서·타임스탬프·권한과 정규화된 JSON을 사용합니다. 레코드와
원본이 같으면 다시 생성해도 ZIP 바이트가 같습니다. `manifest.json`의 `artifacts`는
`scan.jsonl`, `hashes.txt`, 리포트, 포함 원본 각각의 크기와 SHA-256을 기록합니다.
자기 자신을 해시할 수 없는 `manifest.json`만 이 목록에서 제외됩니다.

개인정보 제거 번들:

```bash
metaxtract export-case scan.jsonl redacted.zip --redact
```

`--redact`는 GPS, 작성자·수정자, EXIF 촬영 시각 등 개인정보 필드를 번들의 스캔과
리포트에서 제거합니다. 원본에는 같은 정보가 남을 수 있으므로 `--redact`와
`--include-files`는 함께 사용할 수 없습니다.

### 번들 검증

```bash
metaxtract verify-bundle case.zip
metaxtract verify-bundle case.zip --files-base evidence
```

검증기는 필수 멤버, 중복·위험 ZIP 경로, JSONL 레코드, 매니페스트 인벤토리,
`hashes.txt`, 리포트, 모든 산출물 해시와 포함 원본을 함께 검사합니다. 원본이 없는
번들은 `--files-base`로 외부 파일까지 대조할 수 있습니다.

신뢰하지 않는 입력에 적용되는 기본 상한은 다음과 같습니다.

| 대상 | 상한 |
|---|---:|
| JSONL 전체 / 한 줄 / 레코드 수 | 64 MiB / 1 MiB / 100,000 |
| ZIP 파일 / 멤버 수 | 8 GiB / 10,000 |
| ZIP 개별 멤버 / 압축 해제 합계 | 4 GiB / 16 GiB |
| ZIP 제어 파일 / 압축률 | 64 MiB / 200:1 |

### GUI

```bash
metaxtract gui
```

GUI에는 Python의 Tk 지원이 필요합니다. 스캔은 백그라운드 스레드에서 실행되며,
진행률과 현재 파일을 표시합니다. 취소 요청은 현재 파일 처리가 끝나는 즉시 반영되고
스캔·내보내기 오류는 대화상자로 표시됩니다.

## 프로젝트 구조

| 경로 | 역할 |
|---|---|
| `src/metaxtract/core/` | 스캔, 캐시, 파일·JSONL 처리, 레코드 검증 |
| `src/metaxtract/extractors/` | 포맷별 추출과 메타데이터 정규화 |
| `src/metaxtract/case/` | 번들 생성, redaction, 경로·무결성 검증 |
| `src/metaxtract/reporting/` | JSON·HTML 리포트, findings, diff |
| `src/metaxtract/cli.py` | CLI 라우팅과 공통 실패 처리 |
| `src/metaxtract/gui.py` | 데스크톱 GUI와 백그라운드 작업 제어 |
| `tests/` | 핵심 회귀 테스트와 소형 실제 포맷 fixture |
| `pyproject.toml` | 패키지, 의존성, pytest, Ruff 설정 |
| `.github/workflows/ci.yml` | 빌드, 테스트, 설치형 CLI 플랫폼 검증 |

별도 `scripts/` 디렉터리나 중복 설정 파일은 두지 않습니다. 개발 명령과 도구 설정은
`pyproject.toml`에 모았습니다.

## 개발과 테스트

```bash
python -m pip install -e ".[dev]"
python -m ruff check .
python -m pytest
python -m build --wheel
```

`tests/`는 스캔·추출·캐시·검증·CLI 간 계약과 보안 경계를 지키는 데 필요한 최소
회귀 범위이므로 저장소에 유지합니다. 테스트 및 fixture는 wheel에는 포함되지 않습니다.
실행 중 fixture나 기대 결과를 다시 만드는 생성 스크립트도 사용하지 않습니다.

CI는 먼저 린트·47개 회귀 테스트를 통과한 wheel을 만든 뒤, 그 wheel만 새 환경에
설치합니다. 설치형 CLI 전체 흐름은 Linux·Windows·macOS의 Python 3.11과 Linux의
Python 3.14에서 검사합니다.

## 제한 사항

- 메타데이터 추출 범위는 파일 상태, 사용 라이브러리, 포맷 구현에 따라 달라집니다.
- `ffprobe`가 없거나 30초 안에 끝나지 않으면 비디오 상세 메타데이터를 건너뛰고
  경고를 기록합니다.
- redaction은 번들 내부의 구조화된 메타데이터에만 적용되며 원본 파일을 수정하지
  않습니다.
- 결정적 ZIP과 SHA-256 검증은 무결성 확인 수단입니다. 전자서명, 신뢰 시각,
  작성자 신원 증명을 제공하지는 않습니다.
- 매니페스트는 다른 모든 번들 산출물을 해시하지만 자기 자신은 해시하지 않습니다.

변경 내역은 [CHANGELOG.md](CHANGELOG.md)를 참고하세요.
