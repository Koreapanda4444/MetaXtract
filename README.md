# MetaXtract

MetaXtract v0.1.0 is a deterministic metadata scanner for local files. It focuses on repeatable extraction, simple JSONL output, and lightweight reporting for small forensic or archival workflows.

It currently ships with a CLI and a minimal GUI entry point.

## 설치

### 요구 사항

- Python 3.11+
- pip
- Optional: ffprobe for video metadata
- Optional: ffmpeg for generating local test fixtures

### 설치 방법

```bash
git clone https://github.com/Koreapanda4444/MetaXtract.git
cd MetaXtract
python -m venv .venv
.venv\Scripts\activate
python -m pip install --upgrade pip
python -m pip install -r requirements.txt
python -m pip install pytest flake8
```

Windows에서는 저장소에 포함된 make.bat 덕분에 별도 GNU Make 없이도 make lint, make test를 그대로 사용할 수 있습니다.

### 버전 확인

```bash
python -c "import metaxtract; print(metaxtract.__version__)"
```

## 지원 포맷

| 포맷 | 확장자 예시 | 처리 방식 | 비고 |
|---|---|---|---|
| 이미지 | jpg, jpeg, png | Pillow | EXIF 및 기본 속성 추출 |
| PDF | pdf | PyPDF2 | 문서 메타데이터 추출 |
| DOCX | docx | python-docx | core properties 추출 |
| 비디오 | mp4 등 | ffprobe | ffprobe가 있어야 분석 가능 |

## CLI 사용 예시

### 1. 스캔

```bash
python cli.py scan tests/fixtures --out scan.jsonl
```

### 2. JSON 리포트 생성

```bash
python cli.py report scan.jsonl --out report.json
```

### 3. HTML 리포트 생성

```bash
python cli.py report scan.jsonl --format html --out report.html
```

### 4. 케이스 번들 생성

```bash
python cli.py export-case scan.jsonl case_bundle.zip --case-id CASE-001 --notes "first release sample"
```

원본 파일까지 포함하려면:

```bash
python cli.py export-case scan.jsonl case_bundle.zip --include-files --files-base tests/fixtures
```

### 5. 환경 진단

```bash
python cli.py doctor
```

## 출력물

- scan: JSONL 레코드 목록
- report: JSON 또는 HTML 요약 리포트
- export-case: manifest, hashes, report, optional files를 포함한 ZIP 번들
- verify: JSONL 해시와 실제 파일 일치 여부 검증

## 제한 사항

- 비디오 메타데이터는 ffprobe가 설치되어 있어야 합니다.
- 테스트용 mp4 fixture 생성은 ffmpeg가 없으면 건너뜁니다.
- 추출 가능한 메타데이터 범위는 각 라이브러리와 원본 파일 상태에 따라 달라집니다.
- 현재 패키징 배포보다는 저장소 실행 방식에 맞춰져 있습니다.

## 빠른 시작

```bash
python cli.py scan tests/fixtures --out scan.jsonl
python cli.py report scan.jsonl --out report.json
python cli.py doctor
```

## 개발

### 개발 커맨드

| 커맨드 | 설명 |
|---|---|
| make lint | flake8 실행 |
| make test | pytest 실행 |
| make regen-fixtures | tests/fixtures 재생성 |
| make regen-golden | tests/golden 재생성 |

### 기여용 체크리스트

```bash
make lint
make test
```

fixture 또는 golden을 갱신해야 하는 변경이라면 아래도 함께 실행합니다.

```bash
make regen-fixtures
make regen-golden
```

## CI

GitHub Actions workflow는 lint와 pytest를 실행합니다. 릴리스 태그를 달기 전에는 로컬에서 make lint, make test가 동일하게 통과하는 상태를 기준으로 삼습니다.

## 변경 이력

v0.1.0 변경 내역은 CHANGELOG.md를 참고하세요.
