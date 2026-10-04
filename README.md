# MetaXtract

MetaXtract는 로컬 파일의 메타데이터와 SHA-256 해시를 추출해 JSONL로 기록하고,
리포트·검증 가능한 케이스 ZIP을 만드는 도구입니다. 이미지, PDF, DOCX, 비디오를
지원하며 CLI와 간단한 데스크톱 GUI를 제공합니다.

현재 `main`에는 v0.1.0 이후의 미출시 변경 사항이 포함되어 있습니다.

## 요구 사항

- Python 3.11 이상
- Pillow, pypdf, python-docx
- 비디오 분석 시 `ffprobe` 선택 설치
- 테스트 fixture 재생성 시 `ffmpeg` 선택 설치

## 설치

```bash
git clone https://github.com/Koreapanda4444/MetaXtract.git
cd MetaXtract
python -m venv .venv
```

Windows:

```powershell
.venv\Scripts\activate
python -m pip install --upgrade pip
python -m pip install -r requirements.txt
```

Linux 또는 macOS:

```bash
source .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install -r requirements.txt
```

환경과 선택 기능의 사용 가능 여부는 다음 명령으로 확인합니다.

```bash
python cli.py doctor
```

## 지원 포맷

| 포맷 | 확장자 | 추출 내용 |
|---|---|---|
| 이미지 | `.jpg`, `.jpeg`, `.png` | 크기, 형식, EXIF 기기·촬영 시각·GPS |
| PDF | `.pdf` | 페이지 수, 제목, 작성자, 생성 도구 |
| DOCX | `.docx` | 제목, 작성자, 최종 수정자, 문단 수 |
| 비디오 | `.mp4`, `.mov`, `.m4v` | 길이, 코덱, 해상도, 컨테이너 |

지원하지 않는 형식도 파일 크기, 수정 시각, MIME 기본값, SHA-256은 기록합니다.

## CLI

### 스캔

```bash
python cli.py scan evidence --out scan.jsonl
```

기본 캐시는 `.metaxtract_cache/`에 저장됩니다. `.git`, `__pycache__`, 자체 캐시는
스캔에서 제외되고 한 번에 최대 5,000개 파일을 처리합니다.

```bash
python cli.py scan evidence --cache off --max-files 1000
python cli.py scan evidence --include-hidden --out scan.jsonl
python cli.py cache purge
```

### 리포트

```bash
python cli.py report scan.jsonl --out report.json
python cli.py report scan.jsonl --format html --out report.html
```

### 원본 검증

```bash
python cli.py verify scan.jsonl evidence
```

JSONL의 경로, 크기, SHA-256을 현재 원본과 대조합니다. 누락, 변경, 중복 경로,
심볼릭 링크, 기준 디렉터리 이탈을 구분해 출력하며 문제가 있으면 종료 코드 2를
반환합니다.

### 케이스 번들

메타데이터 전용 번들:

```bash
python cli.py export-case scan.jsonl case.zip --case-id CASE-001
```

원본 포함 번들:

```bash
python cli.py export-case scan.jsonl case.zip \
  --include-files --files-base evidence
```

`--files-base`를 생략하면 `scan.jsonl`이 있는 디렉터리를 기준으로 원본을 찾습니다.
누락 파일, 디렉터리, 심볼릭 링크, 절대 경로, `..` 경로, 중복 경로는 오류로
처리합니다. 원본 해시가 스캔 이후 달라졌다면 번들을 생성하지 않습니다.

개인정보 제거 번들:

```bash
python cli.py export-case scan.jsonl redacted.zip --redact
```

`--redact`는 GPS, 작성자, 최종 수정자, EXIF 촬영 시각을 `scan.jsonl`과 리포트에서
실제로 제거합니다. 원본 파일에는 같은 정보가 남아 있을 수 있으므로
`--redact`와 `--include-files`는 함께 사용할 수 없습니다.

### 번들 검증

```bash
python cli.py verify-bundle case.zip
```

필수 항목, ZIP 중복·위험 경로, JSONL, manifest, 리포트, `hashes.txt`, 포함 원본의
SHA-256을 함께 검사합니다. 원본이 포함되지 않은 번들은 외부 기준 디렉터리까지
대조할 수 있습니다.

```bash
python cli.py verify-bundle case.zip --files-base evidence
```

### 기타

```bash
python cli.py diff old.jsonl new.jsonl --out diff.json
python cli.py gui
```

GUI 실행에는 Python의 Tk 지원이 필요합니다.

## 개발

개발 의존성을 설치한 뒤 lint와 테스트를 실행합니다.

```bash
python -m pip install pytest flake8
make lint
make test
```

Windows에서는 저장소의 `make.bat`로 같은 명령을 실행할 수 있습니다. 테스트는
체크인된 최소 fixture를 사용하며, 실행 중 기대 결과를 자동 생성하지 않습니다.

```bash
make regen-fixtures
```

fixture를 다시 만들었다면 메타데이터·GPS·CLI 통합 테스트를 모두 통과하는지
확인해야 합니다. CI는 `flake8`, CLI smoke test, 전체 `pytest`를 실행합니다.

## 제한 사항

- 파일 형식별 추출 범위는 사용 라이브러리와 원본 상태에 따라 달라집니다.
- `ffprobe`가 없으면 비디오 메타데이터 추출은 건너뜁니다.
- redaction은 번들 안의 스캔 및 리포트 데이터에 적용되며 원본 파일을 재작성하지
  않습니다.
- 번들 검증은 내부 일관성과 해시 일치를 확인합니다. 전자서명이나 신뢰 시각을
  제공하지는 않습니다.
- 아직 PyPI 패키지나 정식 v0.2.0 릴리스로 배포된 상태는 아닙니다.

변경 내역은 [CHANGELOG.md](CHANGELOG.md)를 참고하세요.
