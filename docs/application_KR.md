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
