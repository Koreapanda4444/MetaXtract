
# MetaXtract (WIP)

텍스트/파일에서 메타데이터를 추출하고(Extract), 비교(Diff), 정리(Sanitize), 검증(Verify)하는 도구를 목표로 하는 CLI 프로젝트입니다.

현재는 CLI UX/정책을 먼저 고정하는 단계이며, 대부분의 명령은 아직 미구현입니다.

## 실행

- 권장: `python -m metaxtract <command> [args...]`

예시:

- `python -m metaxtract version`
- `python -m metaxtract -v version`

## 전역 옵션

- `-v/--verbose`: 로그 상세도 증가
	- `0` (기본): WARNING 이상
	- `1` (`-v`): INFO 이상
	- `2+` (`-vv`, `-vvv`...): DEBUG 이상
- `--no-color`: stderr ANSI 컬러 비활성화

## 종료 코드 규약

| 코드 | 의미 |
|---:|---|
| 0 | 성공 |
| 2 | 사용법 오류 / 미구현 기능 (예: 잘못된 인자, 지원되지 않는 명령) |
| 3 | 처리 실패 (예: 파일 처리/검증 실패 등) |
| 1 | 내부 오류 (예기치 못한 예외) |

## 에러 출력 정책

- 사용자에게 보여줄 메시지는 항상 `ERROR: <메시지>` 형태로 stderr에 출력합니다.
- 내부 상세(예외 repr/traceback 등)는 `-vv`(DEBUG) 이상에서만 stderr에 출력합니다.
- `version` 같은 성공 출력은 stdout을 사용합니다.

## 명령 목록 (목적/예시/지원 상태)

| 명령 | 목적 | 예시 | 상태 |
|---|---|---|---|
| `scan` | 경로를 스캔해 파일을 열거(현재는 열거/필터/집계만) | `python -m metaxtract scan . --recursive --include .jpg,.png,.pdf --exclude "node_modules" --exclude ".git"` | 부분 지원 |
| `report` | 인덱스를 사람이 읽기 좋은 형태로 출력 | `python -m metaxtract report index.json` | 지원 |
| `diff` | 두 인덱스/스캔 결과 비교 | `python -m metaxtract diff before.jsonl after.jsonl --key path` | 지원 |
| `sanitize` | 입력에서 민감정보 마스킹/정리 | `python -m metaxtract sanitize input/ --outdir out/ --mode redact` | 부분 지원 |
| `verify` | 인덱스 무결성/체인 오브 커스터디 검증 | `python -m metaxtract verify index.jsonl --summary` | 지원 |
| `gui` | GUI 실행(옵션) | `python -m metaxtract gui` | 예정 |
| `version` | 버전 정보를 JSON으로 출력 | `python -m metaxtract version` | 지원 |

## scan (현재 지원: 파일 열거)

현재 `scan`은 공통 메타(stat) + (옵션) 해시 + 일부 파일 타입(이미지)의 최소 메타(EXIF/GPS)를 추출합니다.

옵션:

- `--recursive`: 하위 폴더까지 재귀 탐색
- `--include .jpg,.png,.pdf`: 확장자 allowlist(쉼표로 구분). 지정하지 않으면 모든 확장자를 허용
- `--exclude pattern`: 제외 패턴(여러 번 지정 가능). `*`/`?`/`[]`가 있으면 glob처럼, 없으면 부분 문자열 매칭
- `--hash sha256|md5|none`: 해시 계산(기본 `none`)
- `--redact`: 출력에서 일부 민감 메타(GPS/author)를 마스킹
- `--out <path>.jsonl`: 결과를 JSONL 인덱스 파일로 저장(기본은 stdout 출력)

예시:

- `python -m metaxtract scan . --recursive --exclude ".git" --exclude "__pycache__"`

출력:

- stdout에 JSON 레코드가 줄 단위로 출력됩니다(JSONL)
- 단일 파일 입력 시 JSON 1개 레코드가 출력됩니다

`--out` 사용 시:

- 지정한 JSONL 파일이 생성됩니다
- 첫 줄은 세션 헤더 레코드입니다
- 이후 줄은 파일별 정규화 레코드가 append 됩니다

## diff

두 JSONL 인덱스 파일(before/after)을 비교하여 변화를 분석합니다.

형식:

- `python -m metaxtract diff <before.jsonl> <after.jsonl> --key path|sha256 --out diff.txt|diff.json`

옵션:

- `--key`: 비교 키 필드 (기본: `path`, 또는 `sha256`)
- `--out`: 출력 파일 (`.txt` 또는 `.json`). 미지정 시 stdout

비교 항목:

- **Privacy flags 변화**: `has_gps`, `has_author`, `has_device_model`, `has_software_trace`, `has_precise_time`의 추가/제거
- **Geo 변화**: `lat`, `lon`, `alt_m`, `precision_flag`
- **Identity 변화**: `author`, `title`, `last_modified_by`
- **Capture 변화**: `make`, `model`, `software`, `datetime_original`
- **Meta times 변화**: `created`, `modified`, `digitized`

변화 요약 카테고리:

- `"GPS removed"` / `"GPS added"`
- `"Author changed"` / `"Author removed"` / `"Author added"`
- `"Device model removed"` / `"Device model added"`
- `"Software trace removed"` / `"Software trace added"`
- `"Precise time removed"` / `"Precise time added"`

출력 형식:

- **txt**: 사람이 읽기 좋은 텍스트 포맷
- **json**: 구조화된 JSON 포맷 (자동화에 적합)

예시:

```bash
# 경로 기준으로 비교
python -m metaxtract diff before.jsonl after.jsonl

# SHA256 해시 기준으로 비교 (파일명이 변경된 경우)
python -m metaxtract diff before.jsonl after.jsonl --key sha256

# 결과를 파일로 저장
python -m metaxtract diff before.jsonl after.jsonl --out diff.txt
python -m metaxtract diff before.jsonl after.jsonl --out diff.json --key sha256
```

## report

JSONL 인덱스에서 리포트를 생성합니다.

형식:

- `python -m metaxtract report <index.jsonl> --format json|csv|txt --template privacy|forensics|content`

옵션:

- `--format`: 출력 포맷
- `--template`: 템플릿
- `--redact`: 출력에서 GPS/author 등 일부 민감 메타 마스킹

CSV 평탄화 규칙:

- 중첩 키를 `a.b.c` 형태로 펼칩니다.

세션 헤더 예시(필드):

- `type=session`
- `tool.version`, `timestamp`, `platform.*`
- `scan.root`, `scan.recursive`, `scan.include`, `scan.exclude`, `scan.hash`

## sanitize (pipeline v1)

입력 파일을 `--outdir`로 복사/가공하여 **민감 메타데이터를 제거/축소**합니다.

형식:

- `python -m metaxtract sanitize <path> --outdir <outdir> --mode redact|minimal`

모드:

- `redact` (기본): 이미지에서 GPS/Author/Software 흔적을 우선 제거(가능한 범위부터)
- `minimal`: 이미지 메타데이터를 가능한 한 제거(픽셀은 유지하려고 시도)

현재 적용 범위(v1):

- JPEG/PNG 이미지부터 단계 적용
- 그 외 포맷은 **그대로 복사**(no-op)

처리 로그(파일별):

- `--outdir` 내부에 `__metaxtract_logs/sanitize/` 아래로 파일별 로그가 생성됩니다.
- 포함 내용: 입력/출력 경로, 제거/마스킹 액션 목록, 전/후 SHA256

예시 워크플로우(스캔→sanitize→재스캔→diff):

```bash
python -m metaxtract scan input/ --recursive --hash sha256 --out before.jsonl
python -m metaxtract sanitize input/ --outdir out/ --mode redact
python -m metaxtract scan out/ --recursive --hash sha256 --exclude "__metaxtract_logs" --out after.jsonl
python -m metaxtract diff before.jsonl after.jsonl --key path --out diff.txt
```

## verify (chain-of-custody v1)

JSONL 인덱스의 무결성을 검증합니다.

형식:

- `python -m metaxtract verify <index.jsonl>`

검증 항목:

- **세션 헤더 존재/형식**: 첫 레코드가 `type=session`인지, `tool/timestamp/platform/scan` 형태가 맞는지
- **레코드 스키마**: 최상위 키가 스키마 v1(`file/os_times/hashes/meta_times/identity/capture/geo/media/signals/raw`)와 일치하는지
- **해시 필드 존재**: 세션에서 `scan.hash`가 `sha256`/`md5`라면 각 레코드의 `hashes.<algo>`가 존재하는지
- **변조/누락 탐지**: JSON 파싱 실패, 레코드 타입 불일치, 필수 키 누락 등

옵션:

- `--summary`: 인덱스 전체 digest(sha256) + 결과를 JSON으로 출력
- `--summary-out <path>`: summary JSON을 파일로 저장

완료 기준 예시(고의 변조):

- JSONL의 일부 레코드에서 키를 삭제하거나 잘못된 형식으로 바꾸면 `verify`가 실패(종료 코드 3)합니다.

## Normalized schema v1

스캔/추출 결과는 어떤 파일이든 최상위 키셋이 동일하도록 유지합니다.

최상위 키:

- `file`, `os_times`, `hashes`, `meta_times`, `identity`, `capture`, `geo`, `media`, `signals`, `raw`

현재 매핑(일부만 채움):

- `file`: `path`, `name`, `ext`, `size_bytes`
- `os_times`: `atime`, `mtime`, `ctime`
- `hashes`: `{algo: hex}` 형태(옵션)
- 이미지(JPEG/PNG)인 경우 일부 필드가 채워질 수 있음(아래 참고)
- 나머지(`identity`/`media`/`signals`)는 v1에서 빈 오브젝트로 시작
- `raw`: 정규화 이전 레코드 원본

### Privacy intelligence v1

모든 레코드에서 `signals.privacy_flags`와 `signals.risk_summary`를 생성합니다.

flags:

- `has_gps`
- `has_author`
- `has_device_model`
- `has_software_trace`
- `has_precise_time`

`signals.risk_summary`는 사람이 읽는 2~4줄 요약 문자열이며, 예를 들어 GPS가 있으면 “위치 노출” 문구가 포함됩니다.

`--redact` 옵션을 사용하면 출력 레코드의 `geo.lat/lon/alt_m`과 `identity.author`를 마스킹합니다. `raw`는 그대로 유지됩니다.

### Timeline consistency v1

OS 시간(`os_times`)과 메타 시간(`meta_times.created/modified/digitized`)을 비교해 불일치/역전 징후를 감지합니다.

결과는 `signals.timeline_flags`에 기록됩니다:

- `time_mismatch`: 불일치 감지 여부
- `reason_codes`: 감지 사유 코드 리스트
- `short_explain`: 짧은 요약 문자열

### Image extractor v1 (JPEG/PNG)

가능한 경우 EXIF/GPS를 추출하고, 정규화 레코드에 다음 필드를 채웁니다:

- `capture.make`, `capture.model`, `capture.software`
- `capture.datetime_original` (EXIF `DateTimeOriginal` 파싱 성공 시)
- `meta_times.digitized` (EXIF `DateTimeDigitized` 파싱 성공 시)
- `geo.lat`, `geo.lon`, `geo.alt_m` (EXIF GPS가 있는 경우)
- `geo.precision_flag`: `"dop"`(GPSDOP 존재) 또는 `"unknown"`

EXIF가 없거나 파싱 실패 시 `raw.extract_error`에 간단한 에러 코드가 들어갈 수 있습니다(예: `no_exif`).

### PDF extractor v1

가능한 경우 PDF 속성(문서 프로퍼티)을 추출하고, 정규화 레코드에 다음 필드를 채웁니다:

- `identity.author` (PDF `Author`)
- `capture.software` (PDF `Creator`/`Producer`를 결합)
- `meta_times.created` (PDF `CreationDate`)
- `meta_times.modified` (PDF `ModDate`)

PDF 메타가 없거나 읽기 실패 시 `raw.extract_error`에 에러 코드가 들어갈 수 있습니다(예: `no_pdf_meta`).

### DOCX extractor v1

가능한 경우 DOCX core properties를 추출하고, 정규화 레코드에 다음 필드를 채웁니다:

- `identity.author` (creator)
- `meta_times.created`, `meta_times.modified`

추가로 일부 코어 필드는 `raw.docx`에 남고, `identity.last_modified_by`, `identity.title`도 채워질 수 있습니다.

DOCX 메타가 없거나 읽기 실패 시 `raw.extract_error`에 에러 코드가 들어갈 수 있습니다(예: `no_docx_meta`).

### Video extractor v1 (MP4)

가능한 경우 MP4 컨테이너에서 기본 미디어 정보를 추출하고 `media.*`에 매핑합니다:

- `media.duration_sec`
- `media.width`, `media.height`
- `media.codec`
- `media.container`
- `media.container_created` (가능한 경우)

영상 메타를 찾지 못하면 `raw.extract_error`에 에러 코드가 들어갈 수 있습니다(예: `no_moov`).

레코드 필드(현재):

- `path`, `name`, `ext`, `size_bytes`
- `os_times.atime|mtime|ctime`
- `hash_algo`, `hash_hex` (해시 옵션 사용 시)

주의:

- `ctime`은 OS/파일시스템에 따라 의미가 다를 수 있습니다(Windows: 생성 시간, Unix 계열: 상태 변경 시간).

## 다음 작업(예정)

- `sanitize`/`verify`: 정책(룰) 정의 후 단계적으로 구현

