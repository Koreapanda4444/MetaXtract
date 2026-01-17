
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
| `report` | 인덱스를 사람이 읽기 좋은 형태로 출력 | `python -m metaxtract report index.json` | 예정 |
| `diff` | 두 인덱스/스캔 결과 비교 | `python -m metaxtract diff before.json after.json` | 예정 |
| `sanitize` | 입력에서 민감정보 마스킹/정리 | `python -m metaxtract sanitize input/ --outdir out/` | 예정 |
| `verify` | 인덱스 무결성/규칙 검증 | `python -m metaxtract verify index.json` | 예정 |
| `gui` | GUI 실행(옵션) | `python -m metaxtract gui` | 예정 |
| `version` | 버전 정보를 JSON으로 출력 | `python -m metaxtract version` | 지원 |

## scan (현재 지원: 파일 열거)

현재 `scan`은 메타 추출 없이 공통 메타(stat) + (옵션) 해시만 출력합니다.

옵션:

- `--recursive`: 하위 폴더까지 재귀 탐색
- `--include .jpg,.png,.pdf`: 확장자 allowlist(쉼표로 구분). 지정하지 않으면 모든 확장자를 허용
- `--exclude pattern`: 제외 패턴(여러 번 지정 가능). `*`/`?`/`[]`가 있으면 glob처럼, 없으면 부분 문자열 매칭
- `--hash sha256|md5|none`: 해시 계산(기본 `none`)
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

세션 헤더 예시(필드):

- `type=session`
- `tool.version`, `timestamp`, `platform.*`
- `scan.root`, `scan.recursive`, `scan.include`, `scan.exclude`, `scan.hash`

## Normalized schema v1

스캔/추출 결과는 어떤 파일이든 최상위 키셋이 동일하도록 유지합니다.

최상위 키:

- `file`, `os_times`, `hashes`, `meta_times`, `identity`, `capture`, `geo`, `media`, `signals`, `raw`

현재 매핑(일부만 채움):

- `file`: `path`, `name`, `ext`, `size_bytes`
- `os_times`: `atime`, `mtime`, `ctime`
- `hashes`: `{algo: hex}` 형태(옵션)
- 나머지(`meta_times`/`identity`/`capture`/`geo`/`media`/`signals`)는 v1에서 빈 오브젝트로 시작
- `raw`: 정규화 이전 레코드 원본

레코드 필드(현재):

- `path`, `name`, `ext`, `size_bytes`
- `os_times.atime|mtime|ctime`
- `hash_algo`, `hash_hex` (해시 옵션 사용 시)

주의:

- `ctime`은 OS/파일시스템에 따라 의미가 다를 수 있습니다(Windows: 생성 시간, Unix 계열: 상태 변경 시간).

## 다음 작업(예정)

- `scan`: 인덱스 스키마 정의 + 최소 스캐너 구현
- `report`: 인덱스 요약/필터 출력
- `diff`: 변경 요약(추가/삭제/수정) 출력
- `sanitize`/`verify`: 정책(룰) 정의 후 단계적으로 구현

