# Rust 앱

[English](application.md) | [한국어](application_KR.md)

명령행 앱과 전용 파서를 함께 빌드합니다.

```bash
cargo build -p metaxtract-cli -p metaxtract-worker --locked
cargo run -p metaxtract-cli --locked -- --help
cargo run -p metaxtract-cli --locked -- scan /path/to/evidence --case investigation.mxcase --progress
cargo run -p metaxtract-cli --locked -- case history investigation.mxcase
cargo run -p metaxtract-cli --locked -- verify investigation.mxcase
```

Windows에서는 `.exe`를 붙입니다. Cargo가 빌드한 `metaxtract-worker`와 검토된 네이티브
라이브러리를 앱 옆에 유지합니다. `inspect FILE`은 독립된 임시 케이스를 만들고 `compare CASE
OLD_SCAN NEW_SCAN`은 저장된 스캔을 비교합니다. 케이스 명령으로 생성·정보·이력·노트를
다룹니다. 출력은 JSON이며 스캔 진행은 stderr로 보냅니다. 운영체제 고유 경로 단위를 보존합니다.

결과는 단일 파일 케이스에 점진적으로 저장하고 실행 중 체크포인트는 별도 `.scan.sqlite`
저널에 유지합니다. 일시정지 중에는 저널도 보존해야 합니다. 같은 원본·케이스와
`--resume RUN --scan CASE_SCAN_ID`로 재개하고 저널 경로를 바꿨다면 `--journal PATH`도
지정합니다. 완료 결과와 원자적 영수증을 대조해 중단 구간을 복구하며 중복 저장을 방지합니다.
저널과 케이스는 다른 파일이어야 하며 출력 파일은 탐색에서 제외합니다.

## 분석과 결과 전달

`scan` 또는 `case history`가 반환하는 스캔 ID를 사용합니다.

```bash
cargo run -p metaxtract-cli -- search investigation.mxcase SCAN_ID --query 'mime:png' --limit 128
cargo run -p metaxtract-cli -- export investigation.mxcase SCAN_ID selected.csv --format csv
cargo run -p metaxtract-cli -- redact investigation.mxcase SCAN_ID private.jsonl
cargo run -p metaxtract-cli -- report investigation.mxcase SCAN_ID report.html --analyst Analyst
cargo run -p metaxtract-cli -- analyze investigation.mxcase SCAN_ID --view timeline
cargo run -p metaxtract-cli -- bundle export investigation.mxcase snapshot.zip
cargo run -p metaxtract-cli -- bundle open snapshot.zip reopened.mxcase
```

검색 문법은 [분석 안내](analysis_KR.md)를 참고하세요. `--cursor`에는 이전 페이지의 `next` JSON을
넣습니다. `--save NAME`은 구조화된 필터를 저장하고 `--order path|size|insertion`은 검색·내보내기
정렬을 지정합니다. `redact --preview RECORD_ID`는 파일을 쓰지 않고 변경 내용을 보여 줍니다.
`--profile FILE`로 기존 비식별화 JSON 프로필을 적용할 수 있으며 원본은 바꾸지 않습니다.
`import CASE INPUT --kind jsonl|python|python-bundle`로 이전 스캔과 Python 기준판 케이스를 가져옵니다.

`keygen PRIVATE PUBLIC --name NAME --password-stdin`은 암호화된 Ed25519 개인키를 만듭니다.
비밀번호는 표준 입력으로 전달하며 명령 인수로는 받지 않습니다. 자동화에서는
`METAXTRACT_KEY_PASSWORD`도 사용할 수 있습니다. 공개키 PEM은 독립적으로 신뢰할 수 있게 보관하세요.
`sign INPUT SIGNATURE --key PRIVATE --password-stdin`은 정확한 원본 바이트를 서명하고
`verify-signature INPUT SIGNATURE PUBLIC`은 이를 검증합니다. 번들 내보내기와 보고서에도 같은 키
옵션을 사용합니다. 서명한 보고서는 `--receipt FILE`이 필수이며 다이제스트 목록과 서명을 별도
보관합니다. 기존 출력 파일은 덮어쓰지 않습니다. 번들의 내부 무결성과 서명자 신뢰는 구분하며,
독립적으로 신뢰하는 키로 인증하려면 `--public-key PUBLIC`을 지정합니다.

## 명령 출력 규약

성공하면 표준 출력에 `{"ok":true,"command":"search","result":...}` JSON 객체 하나를 씁니다.
`--pretty`는 공백만 바꿉니다. 도움말은 사람이 읽는 텍스트입니다. 오류는 표준 오류에
`ok:false`, `error.kind`, `error.message`, `error.exit_code`가 있는 JSON 객체로 출력하며
표준 출력은 비웁니다. `--progress`는 `{"event":"scan","data":...}` JSON을 표준 오류에 한 줄씩
출력합니다. 알 수 없는 옵션·중복 옵션·초과 인수는 케이스를 수정하기 전에 거부합니다.
파일 경로는 운영체제 원래 표현을 지원하며 검색 문구는 UTF-8이어야 합니다.

종료 코드: **0** 완료, **2** 잘못된 인수, **3** 입출력·애플리케이션 오류, **4** 데이터·무결성
검증 오류, **5** 실패·불안정 파일이 있는 스캔 완료, **6** 안전하게 일시 정지, **130** 취소입니다.
부분 완료·정지도 성공 결과 객체에 집계와 체크포인트 ID를 남깁니다. 출력 파이프가 닫히면
정상 종료합니다. `case history`와 `search`는 `items`와 `next`를 반환하며 케이스가 변경되면
페이지 탐색을 처음부터 다시 시작해야 합니다. `--pause-after N`은 최소 N개 파일 완료 후
협력적으로 정지하므로 이미 처리 중이던 파일은 추가로 완료될 수 있습니다. 저널을 보관하고
반환된 run·scan ID로 재개하세요. 운영체제에서 강제 중단되어도 저널로 복구할 수 있습니다.

## Rust 데스크톱

```bash
cargo build -p metaxtract-worker --locked
cargo run -p metaxtract-gui --locked
cargo run -p metaxtract-gui --locked -- investigation.mxcase
```

네이티브 데스크톱에서 케이스 생성·열기, 최근 케이스, 읽기 전용 열기, 스캔 기록, 제목·노트를
사용합니다. 수정은 로컬 케이스에 저장됩니다. 복사 저장은 일관된 휴대용 스냅샷을 만들고
검증한 뒤 복사본을 열며 원래 케이스를 보존합니다. 제한된 경로 선택기는 다른 프로그램을
실행하지 않고 직접 입력과 운영체제 원래 경로 표현의 항목 선택을 지원합니다. 최근 경로는
운영체제 MetaXtract 설정 디렉터리에 저장합니다.

한국어 표시는 Windows의 맑은 고딕, macOS의 Apple SD Gothic Neo, Linux의 Noto Sans CJK·
나눔고딕을 사용합니다. 필요하면 설치된 TrueType/OpenType 폰트 경로를 `METAXTRACT_FONT_PATH`로
지정하세요. Linux는 데스크톱 디스플레이와 OpenGL이 필요합니다. `--smoke`는 실제 창을 띄워
여러 프레임을 렌더링한 후 종료하는 설치 확인용 옵션입니다. 케이스 작업은 백그라운드에서
수행해 화면 응답을 유지하며 기존 앱 아이콘을 보존합니다.

개발 시 `cargo run`은 네이티브 라이브러리 검색 경로를 자동 설정합니다. 독립 실행 파일을
배포할 때는 `metaxtract-worker`와 [의존성 안내](dependencies_KR.md)의 공유 라이브러리·고지를
함께 구성해야 합니다.
