# MetaXtract

MetaXtract is a small, deterministic metadata scanner that can extract basic metadata from:

- Images (JPEG/PNG) via Pillow (EXIF + basic properties)
- PDFs via PyPDF2
- DOCX via python-docx
- Videos (optional) via ffprobe if available

It supports a CLI (`python cli.py scan <path>`) and a minimal GUI (`python gui.py`).


## 필수/선택 의존성

MetaXtract는 아래 의존성에 따라 기능이 제한될 수 있습니다:

- **외부 바이너리**
	- ffprobe: 영상 메타데이터 추출 (없으면 영상 분석 제한)
	- ffmpeg: 영상/오디오 처리 (권장)
	- exiftool: 이미지/문서 메타데이터 추출 (권장)
- **파이썬 패키지**
	- Pillow: 이미지 처리 (없으면 이미지 분석 제한)
	- python-docx: docx 문서 추출
	- PyPDF2/pypdf: PDF 추출

## 환경 진단 (doctor)

환경 및 의존성 문제를 진단하려면:

```bash
python cli.py doctor
# 또는
python -m metaxtract doctor
```

예시 출력:
```
[환경 정보]
OS: Windows-10-10.0.19045-SP0
Python: 3.11.7 (main, Jan  1 2026, ...)
실행 파일: C:/Users/user/miniconda3/python.exe

[외부 바이너리]
O ffprobe: 영상 메타데이터 분석에 필요 (C:/ffmpeg/bin/ffprobe.exe)
X exiftool: 이미지/문서 메타데이터 추출에 권장 (없음)

[파이썬 패키지]
O Pillow: 이미지 처리에 필요
X PyPDF2: PDF 추출에 권장

[경고/권장사항]
[경고] ffprobe가 없으므로 영상 메타데이터 분석이 제한됩니다.
[경고] Pillow가 없으므로 이미지 추출이 제한됩니다.
```

## Quick start

```bash
python cli.py scan tests/fixtures --out scan.jsonl
python cli.py report scan.jsonl --out report.json
python -m pytest
```

## 개발자 가이드 (How to contribute)

### 개발 커맨드

| 커맨드 | 설명 |
|---|---|
| `make lint` | flake8 lint 검사 |
| `make test` | pytest 전체 테스트 실행 |
| `make regen-fixtures` | `tests/fixtures/` 샘플 파일 재생성 |
| `make regen-golden` | `tests/golden/` golden 파일 재생성 |

### 빠른 시작

```bash
# 의존성 설치
pip install -r requirements.txt

# lint 검사
make lint

# 테스트 실행
make test

# fixture 재생성 (테스트용 샘플 파일 갱신)
make regen-fixtures

# golden 재생성 (스캔 결과 기준값 갱신)
make regen-golden
```

### 디렉토리 구조

```
tests/
├── fixtures/        # 테스트용 샘플 파일 (jpg, pdf, docx 등)
├── golden/          # 스캔 결과 기준값 (.jsonl)
├── gen_fixtures.py  # fixture 생성 로직
├── gen_golden.py    # golden 생성 로직
└── test_*.py        # pytest 테스트
scripts/
├── regen_fixtures.py  # make regen-fixtures 진입점
└── regen_golden.py    # make regen-golden 진입점
```
